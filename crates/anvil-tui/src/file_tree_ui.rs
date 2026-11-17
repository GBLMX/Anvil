//! Workspace tree and fuzzy file finder state.

use super::{FuzzyState, Model, PreviewState, TreeEntry, TreeFindState};
use anvil_ipc::DirEntry;
use anvil_session::ui::{CreateKind, InputMode};
use anvil_workspace::{FileEvent, FileRequest, FileWorker, MAX_TREE_WATCH_DIRS, PreviewData};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

mod fuzzy;
mod mutations;
mod snapshot;
#[cfg(test)]
mod tests;
mod tree;

const TREE_WRITE_TEMP_PREFIX: &str = ".anvil-write-";

const MAX_TREE_FIND_MATCHES: usize = 1024;

fn tree_entry_ignored(name: &str) -> bool {
    name == ".git" || name == "target" || name.starts_with(TREE_WRITE_TEMP_PREFIX)
}

fn lowercase_char(character: char) -> char {
    character.to_lowercase().next().unwrap_or(character)
}

fn natural_cmp(left: &str, right: &str) -> Ordering {
    let mut left_chars = left.chars().peekable();
    let mut right_chars = right.chars().peekable();
    loop {
        match (left_chars.peek().copied(), right_chars.peek().copied()) {
            (Some(left_char), Some(right_char))
                if left_char.is_ascii_digit() && right_char.is_ascii_digit() =>
            {
                let mut left_digits = String::new();
                let mut right_digits = String::new();
                while let Some(digit) = left_chars.peek().copied().filter(char::is_ascii_digit) {
                    left_digits.push(digit);
                    left_chars.next();
                }
                while let Some(digit) = right_chars.peek().copied().filter(char::is_ascii_digit) {
                    right_digits.push(digit);
                    right_chars.next();
                }
                let left_number = left_digits.trim_start_matches('0');
                let right_number = right_digits.trim_start_matches('0');
                let order = left_number
                    .len()
                    .cmp(&right_number.len())
                    .then_with(|| left_number.cmp(right_number));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(left_char), Some(right_char)) => {
                let order = lowercase_char(left_char).cmp(&lowercase_char(right_char));
                if order != Ordering::Equal {
                    return order;
                }
                left_chars.next();
                right_chars.next();
            }
            (None, None) => break,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
        }
    }
    left.cmp(right)
}

fn sort_dir_entries(entries: &mut [DirEntry]) {
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| natural_cmp(&left.name, &right.name))
    });
}

fn normalize_listing(result: Result<Vec<DirEntry>, String>) -> Result<Vec<DirEntry>, String> {
    result.map(|mut entries| {
        entries.retain(|entry| !tree_entry_ignored(&entry.name));
        sort_dir_entries(&mut entries);
        entries
    })
}

impl Model {
    pub(crate) fn ensure_file_worker(&mut self) -> bool {
        if self.workspace.file_worker.is_none() {
            let Some(session) = self.workspace.gateway_session.as_mut() else {
                return false;
            };
            let Ok(client) = session.connect_lsp() else {
                return false;
            };
            let worker = FileWorker::start_with_services(
                client,
                self.workspace.path.clone(),
                self.workspace.git_service.clone(),
                self.workspace.workspace_index_service.clone(),
            );
            let watch = self.config.settings.watch;
            let _ = worker.request(FileRequest::SetWatchConfig {
                mode: watch.mode.as_str().to_owned(),
                max_dirs: watch.max_dirs,
                cross_mounts: watch.cross_mounts,
            });
            let _ = worker.request(FileRequest::SetWatchEnabled(
                self.config.settings.editor.file_watching
                    && watch.mode != crate::settings::WatchMode::Off,
            ));
            self.workspace.file_worker = Some(worker);
        }
        self.ensure_root_file_workers();
        true
    }

    pub(crate) fn ensure_root_file_workers(&mut self) {
        let roots = self
            .workspace
            .roots
            .list()
            .iter()
            .filter(|root| root.trusted && root.path != self.workspace.path)
            .map(|root| root.path.clone())
            .collect::<Vec<_>>();
        for root in roots {
            if self.workspace.root_file_workers.contains_key(&root) {
                continue;
            }
            let worker = self
                .workspace
                .root_gateways
                .get_mut(&root)
                .and_then(|(session, _)| session.connect_lsp().ok())
                .map(|client| {
                    FileWorker::start_with_services(
                        client,
                        root.clone(),
                        self.workspace.git_service.clone(),
                        self.workspace.workspace_index_service.clone(),
                    )
                });
            if let Some(worker) = worker {
                let watch = self.config.settings.watch;
                let _ = worker.request(FileRequest::SetWatchConfig {
                    mode: watch.mode.as_str().to_owned(),
                    max_dirs: watch.max_dirs,
                    cross_mounts: watch.cross_mounts,
                });
                let _ = worker.request(FileRequest::SetWatchEnabled(
                    self.config.settings.editor.file_watching
                        && watch.mode != crate::settings::WatchMode::Off,
                ));
                self.workspace.root_file_workers.insert(root, worker);
            }
        }
    }

    pub(crate) fn stop_root_file_worker(&mut self, root: &Path) {
        self.workspace.root_file_workers.remove(root);
        if self.project.search_pending.is_some() {
            self.cancel_project_search();
        }
    }

    fn secondary_root(&self, path: &Path) -> Option<PathBuf> {
        self.workspace
            .roots
            .owning_root(path)
            .filter(|root| root.path != self.workspace.path)
            .map(|root| root.path.clone())
    }

    pub(crate) fn tree_entry_untrusted(&self, entry: &TreeEntry) -> bool {
        entry.depth == 0
            && entry.path != self.workspace.path
            && self
                .workspace
                .roots
                .list()
                .iter()
                .any(|root| root.path == entry.path && !root.trusted)
    }

    pub(crate) fn tree_entry_label(&self, entry: &TreeEntry) -> String {
        let is_root = entry.depth == 0
            && entry.path != self.workspace.path
            && self
                .workspace
                .roots
                .list()
                .iter()
                .any(|root| root.path == entry.path);
        if !is_root {
            return entry
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        let label = self.workspace.root_label(&entry.path);
        if self.tree_entry_untrusted(entry) {
            format!("[untrusted] {label}")
        } else {
            label
        }
    }

    fn ensure_root_tree_nodes(&mut self) {
        let roots = self
            .workspace
            .roots
            .list()
            .iter()
            .filter(|root| root.path != self.workspace.path)
            .map(|root| root.path.clone())
            .collect::<Vec<_>>();
        for root in roots {
            if self
                .workspace
                .tree
                .entries
                .iter()
                .any(|entry| entry.path == root)
            {
                continue;
            }
            self.workspace.tree.entries.push(TreeEntry {
                path: root,
                depth: 0,
                is_dir: true,
                ino: 0,
                expanded: false,
            });
        }
    }

    pub(crate) fn file_worker_for_path(&self, path: &Path) -> Option<&FileWorker> {
        match self.secondary_root(path) {
            Some(root) => self.workspace.root_file_workers.get(&root),
            None => self.workspace.file_worker.as_ref(),
        }
    }

    fn refresh_root_tree(&mut self, root: &Path) {
        let mut paths = vec![root.to_path_buf()];
        paths.extend(
            self.workspace
                .tree
                .entries
                .iter()
                .filter(|entry| entry.expanded && entry.path.starts_with(root))
                .map(|entry| entry.path.clone()),
        );
        paths.sort();
        paths.dedup();
        let generation = self.workspace.tree.refresh_generation;
        if let Some(worker) = self.workspace.root_file_workers.get(root) {
            for path in paths {
                let _ = worker.request(FileRequest::List { path, generation });
            }
        } else {
            for path in paths {
                let result = self.tree_entries(&path);
                self.handle_tree_listing(generation, path, result);
            }
        }
    }

    fn handle_tree_listing(
        &mut self,
        generation: u64,
        path: PathBuf,
        result: Result<Vec<DirEntry>, String>,
    ) {
        if generation != self.workspace.tree.refresh_generation {
            return;
        }
        if self.workspace.roots.owning_root(&path).is_none() {
            return;
        }
        let entries = match normalize_listing(result) {
            Ok(entries) => entries,
            Err(error) => {
                self.drop_stale_tree_path(&path);
                self.status.text = format!("E: tree: {error}");
                return;
            }
        };
        let entries: Vec<DirEntry> = entries
            .into_iter()
            .filter(|entry| self.workspace.tree.entry_visible(&entry.name, entry.is_dir))
            .collect();
        let expanded = self
            .workspace
            .tree
            .entries
            .iter()
            .map(|entry| (entry.path.clone(), entry.expanded))
            .collect::<BTreeMap<_, _>>();
        if path == self.workspace.path {
            let preserved = self
                .workspace
                .tree
                .entries
                .iter()
                .filter(|entry| self.secondary_root(&entry.path).is_some())
                .cloned()
                .collect::<Vec<_>>();
            self.workspace.tree.entries = entries
                .into_iter()
                .map(|entry| {
                    let child = path.join(entry.name);
                    TreeEntry {
                        expanded: expanded.get(&child).copied().unwrap_or(false),
                        path: child,
                        depth: 0,
                        is_dir: entry.is_dir,
                        ino: entry.ino,
                    }
                })
                .collect();
            self.workspace.tree.entries.extend(preserved);
            self.ensure_root_tree_nodes();
            if self.workspace.tree.selected.is_none() && !self.workspace.tree.entries.is_empty() {
                self.workspace.tree.selected = Some(0);
            }
            self.apply_pending_tree_selection();
        } else if let Some(index) = self
            .workspace
            .tree
            .entries
            .iter()
            .position(|entry| entry.path == path)
            && self.workspace.tree.entries[index].expanded
        {
            let depth = self.workspace.tree.entries[index].depth + 1;
            let parent_depth = self.workspace.tree.entries[index].depth;
            let children: Vec<TreeEntry> = entries
                .into_iter()
                .map(|entry| {
                    let child = path.join(entry.name);
                    TreeEntry {
                        expanded: expanded.get(&child).copied().unwrap_or(false),
                        path: child,
                        depth,
                        is_dir: entry.is_dir,
                        ino: entry.ino,
                    }
                })
                .collect();
            let mut end = index + 1;
            while self
                .workspace
                .tree
                .entries
                .get(end)
                .is_some_and(|child| child.depth > parent_depth)
            {
                end += 1;
            }
            self.workspace.tree.entries.splice(index + 1..end, children);
        }
        self.status.text = "file tree ready".to_owned();
        self.sync_tree_watches();
        self.apply_pending_tree_selection();
        self.refresh_tree_find();
    }

    pub(crate) fn schedule_git_status_refresh(&mut self) {
        if self.workspace.git_service.is_none() {
            return;
        }
        self.workspace.git.schedule_status_refresh(Instant::now());
    }

    pub(crate) fn tick_git_status(&mut self) {
        if self.workspace.git.take_due_status_refresh(Instant::now()) {
            self.request_git_status();
        }
    }

    pub(crate) fn request_git_status(&mut self) {
        if self.workspace.git_service.is_none() {
            return;
        }
        self.workspace.git.status_generation = self.workspace.git.status_generation.wrapping_add(1);
        let generation = self.workspace.git.status_generation;
        let mut expected = 0usize;
        let mut error = None;
        if let Some(worker) = self.workspace.file_worker.as_ref() {
            if let Err(request_error) = worker.request(FileRequest::GitStatus {
                root: self.workspace.path.clone(),
                generation,
            }) {
                error = Some(request_error);
            } else {
                expected += 1;
            }
        }
        for (root, worker) in &self.workspace.root_file_workers {
            match worker.request(FileRequest::GitStatus {
                root: root.clone(),
                generation,
            }) {
                Ok(()) => expected += 1,
                Err(request_error) => error = error.or(Some(request_error)),
            }
        }
        if expected == 0 {
            if let Some(error) = error {
                self.status.text = format!("E: git status: {error}");
                self.file.dirty = true;
            }
            return;
        }
        self.workspace.git.status.clear();
        self.workspace.git.status_pending = Some(generation);
        self.workspace.git.status_expected = expected;
        self.workspace.git.status_received = 0;
    }

    pub(crate) fn apply_watch_status(
        &mut self,
        root: String,
        status: crate::state::WatchRootStatus,
    ) {
        self.workspace.tree.watch.insert(root, status);
        self.file.dirty = true;
        self.render_watch_status();
    }

    pub(crate) fn show_watch_status(&mut self) {
        self.request_watch_status();
        self.render_watch_status();
    }

    fn request_watch_status(&mut self) {
        if let Some(worker) = &self.workspace.file_worker {
            let _ = worker.request(FileRequest::WatchStatus);
        }
        for worker in self.workspace.root_file_workers.values() {
            let _ = worker.request(FileRequest::WatchStatus);
        }
    }

    fn render_watch_status(&mut self) {
        let watch = self.config.settings.watch;
        let enabled = self.config.settings.editor.file_watching
            && watch.mode != crate::settings::WatchMode::Off;
        let scope = if !enabled {
            "off"
        } else if self.workspace.git_service.is_some() {
            "open buffers + visible tree + git metadata"
        } else {
            "open buffers + visible tree"
        };
        let mut text = format!(
            "watch: {scope}; mode={}; max_dirs={}; cross_mounts={}",
            watch.mode.as_str(),
            watch.max_dirs,
            if watch.cross_mounts { "yes" } else { "no" }
        );
        if enabled {
            text.push_str(&format!(
                "; roots={}; tree_watch={}; stale={}",
                self.workspace.roots.list().len(),
                if self.workspace.tree.watch_limited {
                    "limited to 256 directories"
                } else {
                    "bounded"
                },
                if self.workspace.tree.stale {
                    "yes"
                } else {
                    "no"
                }
            ));
            if !self.workspace.tree.watch.is_empty() {
                let mut coverage = "full";
                let mut watched = 0;
                let mut skipped = 0;
                let mut overflow = 0;
                let mut reasons = std::collections::BTreeSet::new();
                for status in self.workspace.tree.watch.values() {
                    watched += status.watched_dirs;
                    skipped += status.skipped_dirs;
                    overflow += status.overflow_count;
                    if status.coverage == "limited" {
                        coverage = "limited";
                    } else if status.coverage == "starting" && coverage == "full" {
                        coverage = "starting";
                    } else if status.coverage == "off" {
                        coverage = "off";
                    }
                    if let Some(reason) = &status.reason {
                        reasons.insert(reason.as_str());
                    }
                }
                text.push_str(&format!(
                    "; native={coverage}; watched={watched}; skipped={skipped}; overflow={overflow}"
                ));
                if !reasons.is_empty() {
                    text.push_str(&format!(
                        "; reason={}",
                        reasons.into_iter().collect::<Vec<_>>().join(", ")
                    ));
                }
            }
        }
        self.status.text = text;
        self.file.dirty = true;
    }

    pub(crate) fn refresh_tree_from_command(&mut self) {
        self.refresh_tree(None);
        self.status.text = "tree refresh requested".to_owned();
        self.file.dirty = true;
    }

    fn handle_git_status(
        &mut self,
        generation: u64,
        result: Result<std::collections::HashMap<PathBuf, char>, String>,
    ) {
        if self.workspace.git.status_pending != Some(generation) {
            return;
        }
        self.workspace.git.status_received = self.workspace.git.status_received.saturating_add(1);
        match result {
            Ok(statuses) => self.workspace.git.status.extend(statuses),
            Err(error) => {
                self.status.text = format!("E: git status: {error}");
                self.file.dirty = true;
            }
        }
        if self.workspace.git.status_received >= self.workspace.git.status_expected {
            self.workspace.git.status_pending = None;
            self.workspace.git.status_expected = 0;
            self.workspace.git.status_received = 0;
            self.file.dirty = true;
            self.refresh_changed_files_picker();
        }
    }

    pub(crate) fn poll_file_worker(&mut self) {
        let poll = self.workspace.poll();
        for event in poll.file_events {
            match event {
                FileEvent::Listed {
                    generation,
                    path,
                    result,
                } => self.handle_tree_listing(generation, path, result),
                FileEvent::CompletionListed { generation, result } => {
                    self.handle_path_completion(generation, result);
                }
                FileEvent::Files { generation, result } => {
                    self.handle_fuzzy_files(None, generation, result);
                }
                FileEvent::ProjectSearch {
                    query,
                    generation,
                    result,
                } => self.handle_project_search(None, query, generation, result),
                FileEvent::ProjectReplacePreview { generation, result } => {
                    self.handle_project_replace_preview(generation, result)
                }
                FileEvent::ProjectReplaceApplied { generation, result } => {
                    self.handle_project_replace_applied(generation, result)
                }
                FileEvent::Preview {
                    path,
                    generation,
                    result,
                } => self.handle_preview(path, generation, result),
                FileEvent::Format {
                    path,
                    revision,
                    generation,
                    result,
                } => self.handle_format(path, revision, generation, result),
                FileEvent::GitDiff {
                    path,
                    revision,
                    result,
                } => self.handle_git_diff(path, revision, result),
                FileEvent::GitDiffText {
                    relative,
                    staged,
                    generation,
                    result,
                } => self.handle_git_diff_text(relative, staged, generation, result),
                FileEvent::GitApplied { generation, result } => {
                    self.handle_git_applied(generation, result)
                }
                FileEvent::GitBlameText { generation, result } => {
                    self.handle_git_blame_text(generation, result)
                }
                FileEvent::GitConflictStageText {
                    relative,
                    stage,
                    generation,
                    result,
                } => self.handle_git_conflict_stage(relative, stage, generation, result),
                FileEvent::GitResolveConflict {
                    relative,
                    stage,
                    generation,
                    result,
                } => self.handle_git_resolved(relative, stage, generation, result),
                FileEvent::GitConflictsText {
                    root,
                    generation,
                    result,
                } => self.handle_git_conflicts_text(root, generation, result),
                FileEvent::Transferred {
                    from,
                    to,
                    copy,
                    result,
                } => {
                    self.status.busy = None;
                    match result {
                        Ok(()) => {
                            if !copy {
                                self.remap_open_path(&from, &to);
                            }
                            self.status.text = if copy {
                                format!("copied {}", to.display())
                            } else {
                                format!("moved {}", to.display())
                            };
                            self.refresh_tree(Some(to));
                            self.finish_busy_progress(None);
                        }
                        Err(error) => {
                            self.status.text = format!("E: {error}");
                            self.file.dirty = true;
                            self.finish_busy_progress(Some(&error));
                        }
                    }
                }
                FileEvent::Removed { path, result } => {
                    self.status.busy = None;
                    match result {
                        Ok(()) => {
                            self.mark_missing(&path);
                            self.status.text = format!("deleted {}", path.display());
                            self.refresh_tree(None);
                            self.finish_busy_progress(None);
                        }
                        Err(error) => {
                            self.status.text = format!("E: {error}");
                            self.file.dirty = true;
                            self.finish_busy_progress(Some(&error));
                        }
                    }
                }
                FileEvent::ExternalChanged { path } => self.handle_external_change(path),
                FileEvent::TreeChanged { paths } => {
                    self.handle_tree_changed(None, paths);
                }
                FileEvent::WatchStatus {
                    root,
                    mode: _,
                    coverage,
                    watched_dirs,
                    skipped_dirs,
                    overflow_count,
                    reason,
                } => self.apply_watch_status(
                    root,
                    crate::state::WatchRootStatus {
                        coverage,
                        watched_dirs,
                        skipped_dirs,
                        overflow_count,
                        reason,
                    },
                ),
                FileEvent::TreeWatchUnavailable(error) => {
                    self.workspace.tree.stale = true;
                    self.status.text = format!(
                        "E: tree watcher: {error}; tree is stale, :tree-refresh reloads it"
                    );
                }
                FileEvent::Oversized { message } => {
                    self.workspace.tree.stale = true;
                    self.status.text = format!(
                        "E: file worker: {message}; tree is stale, :tree-refresh reloads it"
                    );
                }
                FileEvent::TreeSnapshot {
                    generation,
                    listings,
                } => self.apply_tree_snapshot(generation, listings),
                FileEvent::TreeListings {
                    generation,
                    listings,
                } => self.apply_tree_listings(generation, listings),
                FileEvent::GitStatus {
                    generation, result, ..
                } => {
                    self.handle_git_status(generation, result);
                }
            }
            self.file.dirty = true;
        }
        for (root, event) in poll.root_events {
            match event {
                FileEvent::Files { generation, result } => {
                    self.handle_fuzzy_files(Some(root.clone()), generation, result);
                }
                FileEvent::Preview {
                    path,
                    generation,
                    result,
                } => {
                    let absolute = root.join(path).to_string_lossy().into_owned();
                    self.handle_preview(absolute, generation, result);
                }
                FileEvent::Format {
                    path,
                    revision,
                    generation,
                    result,
                } => self.handle_format(path, revision, generation, result),
                FileEvent::ProjectSearch {
                    query,
                    generation,
                    result,
                } => {
                    self.handle_project_search(Some(root.clone()), query, generation, result);
                }
                FileEvent::Listed {
                    generation,
                    path,
                    result,
                } => self.handle_tree_listing(generation, path, result),
                FileEvent::GitStatus {
                    generation, result, ..
                } => self.handle_git_status(generation, result),
                FileEvent::ExternalChanged { path } => self.handle_external_change(path),
                FileEvent::TreeChanged { paths } => {
                    self.handle_tree_changed(Some(&root), paths);
                }
                FileEvent::TreeListings {
                    generation,
                    listings,
                } => self.apply_tree_listings(generation, listings),
                FileEvent::WatchStatus {
                    root,
                    mode: _,
                    coverage,
                    watched_dirs,
                    skipped_dirs,
                    overflow_count,
                    reason,
                } => self.apply_watch_status(
                    root,
                    crate::state::WatchRootStatus {
                        coverage,
                        watched_dirs,
                        skipped_dirs,
                        overflow_count,
                        reason,
                    },
                ),
                FileEvent::TreeWatchUnavailable(error) => {
                    self.workspace.tree.stale = true;
                    self.status.text = format!(
                        "E: tree watcher: {error}; tree is stale, :tree-refresh reloads it"
                    );
                }
                FileEvent::Oversized { message } => {
                    self.workspace.tree.stale = true;
                    self.status.text = format!(
                        "E: file worker: {message}; tree is stale, :tree-refresh reloads it"
                    );
                }
                _ => {}
            }
        }
    }

    fn tree_entries(&mut self, path: &Path) -> Result<Vec<DirEntry>, String> {
        let (root_path, trusted) = if let Some(root) = self.secondary_root(path) {
            let trusted = self
                .workspace
                .roots
                .owning_root(path)
                .is_some_and(|root| root.trusted);
            (root, trusted)
        } else if path.starts_with(&self.workspace.path) {
            (self.workspace.path.clone(), true)
        } else {
            return Err(format!(
                "path is outside workspace roots: {}",
                path.display()
            ));
        };
        if !trusted {
            return Err(format!(
                "workspace root is untrusted: {}",
                root_path.display()
            ));
        }
        let relative = if path == root_path {
            ".".to_owned()
        } else {
            path.strip_prefix(&root_path)
                .map_err(|_| format!("path is outside workspace root {}", root_path.display()))?
                .to_string_lossy()
                .into_owned()
        };
        if root_path == self.workspace.path {
            if let Some(gateway) = self.workspace.gateway.as_mut() {
                return gateway
                    .read_dir(&relative)
                    .map_err(|error| error.to_string());
            }
        } else if let Some((_, gateway)) = self.workspace.root_gateways.get_mut(&root_path) {
            return gateway
                .read_dir(&relative)
                .map_err(|error| error.to_string());
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if tree_entry_ignored(&name) {
                continue;
            }
            entries.push(DirEntry {
                is_dir: entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false),
                name,
                ino: 0,
            });
        }
        sort_dir_entries(&mut entries);
        Ok(entries)
    }

    pub(crate) fn toggle_hidden(&mut self) {
        self.workspace.tree.show_hidden = !self.workspace.tree.show_hidden;
        if self.workspace.tree.visible {
            self.workspace.tree.refresh_generation =
                self.workspace.tree.refresh_generation.wrapping_add(1);
            let generation = self.workspace.tree.refresh_generation;
            self.workspace.tree.entries.clear();
            self.workspace.tree.selected = None;
            let workspace = self.workspace.path.clone();
            if self.ensure_file_worker() {
                if let Some(worker) = self.workspace.file_worker.as_ref() {
                    let _ = worker.request(FileRequest::List {
                        path: workspace,
                        generation,
                    });
                }
                self.ensure_root_tree_nodes();
                let roots = self
                    .workspace
                    .root_file_workers
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>();
                for root in roots {
                    if let Some(worker) = self.workspace.root_file_workers.get(&root) {
                        let _ = worker.request(FileRequest::List {
                            path: root,
                            generation,
                        });
                    }
                }
            } else if let Ok(entries) = self.tree_entries(&workspace) {
                self.workspace.tree.entries = entries
                    .into_iter()
                    .filter(|entry| self.workspace.tree.entry_visible(&entry.name, entry.is_dir))
                    .map(|entry| TreeEntry {
                        path: workspace.join(entry.name),
                        depth: 0,
                        is_dir: entry.is_dir,
                        ino: entry.ino,
                        expanded: false,
                    })
                    .collect();
                self.ensure_root_tree_nodes();
            }
            self.workspace.tree.selected = (!self.workspace.tree.entries.is_empty()).then_some(0);
        }
        self.status.text = if self.workspace.tree.show_hidden {
            "hidden files shown".to_owned()
        } else {
            "hidden files hidden".to_owned()
        };
        self.sync_tree_watches();
        self.request_git_status();
        self.file.dirty = true;
    }

    fn tree_target_directory(&self) -> PathBuf {
        let Some(index) = self.workspace.tree.selected else {
            return self.workspace.path.clone();
        };
        let Some(entry) = self.workspace.tree.entries.get(index) else {
            return self.workspace.path.clone();
        };
        if entry.is_dir {
            entry.path.clone()
        } else {
            entry
                .path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.workspace.path.clone())
        }
    }

    fn tree_entry_exists(&mut self, directory: &Path, name: &str) -> Result<bool, String> {
        self.tree_entries(directory)
            .map(|entries| entries.iter().any(|entry| entry.name == name))
    }

    fn drop_stale_tree_path(&mut self, path: &Path) {
        self.workspace.tree.stale = true;
        let Some(index) = self
            .workspace
            .tree
            .entries
            .iter()
            .position(|entry| entry.path == path)
        else {
            return;
        };
        let depth = self.workspace.tree.entries[index].depth;
        let mut end = index + 1;
        while self
            .workspace
            .tree
            .entries
            .get(end)
            .is_some_and(|entry| entry.depth > depth)
        {
            end += 1;
        }
        self.workspace.tree.entries.drain(index..end);
        if self
            .workspace
            .tree
            .selected
            .is_some_and(|selected| selected >= index)
        {
            self.workspace.tree.selected = (!self.workspace.tree.entries.is_empty()).then_some(
                index
                    .saturating_sub(1)
                    .min(self.workspace.tree.entries.len() - 1),
            );
        }
        let root = self
            .workspace
            .roots
            .owning_root(path)
            .map(|root| root.path.clone())
            .unwrap_or_else(|| self.workspace.path.clone());
        let parent = path
            .parent()
            .filter(|parent| parent.starts_with(&root))
            .map(Path::to_path_buf);
        let generation = self.workspace.tree.refresh_generation;
        let requested = parent.as_ref().is_some_and(|parent| {
            self.file_worker_for_path(parent).is_some_and(|worker| {
                worker
                    .request(FileRequest::List {
                        path: parent.clone(),
                        generation,
                    })
                    .is_ok()
            })
        });
        if !requested {
            if let Some(parent) = parent {
                let result = self.tree_entries(&parent);
                self.handle_tree_listing(generation, parent, result);
            } else if root == self.workspace.path {
                self.refresh_tree_background();
            } else {
                self.refresh_root_tree(&root);
            }
        }
    }
    pub(crate) fn reveal_tree_path(&mut self, path: PathBuf) {
        let allowed = if let Some(root) = self.workspace.roots.owning_root(&path) {
            if !root.trusted {
                self.status.text = format!(
                    "E: tree reveal: workspace root is untrusted: {}",
                    root.path.display()
                );
                self.file.dirty = true;
                return;
            }
            true
        } else {
            path.starts_with(&self.workspace.path)
        };
        if !allowed {
            self.status.text = format!("E: tree reveal: outside workspace: {}", path.display());
            self.file.dirty = true;
            return;
        }
        if !self.workspace.tree.visible {
            self.open_tree();
        } else {
            self.ui.focus.tree = true;
            self.ui.focus.split = false;
            self.refresh_wrap_width();
        }
        self.workspace.tree.pending_select = Some(path.clone());
        self.workspace.tree.pending_reveal = Some(path);
        self.workspace.tree.reveal_steps = 0;
        self.advance_pending_reveal();
        self.file.dirty = true;
    }

    fn advance_pending_reveal(&mut self) {
        let Some(target) = self.workspace.tree.pending_reveal.clone() else {
            return;
        };
        if self.workspace.tree.entries.is_empty() {
            return;
        }
        if let Some(index) = self
            .workspace
            .tree
            .entries
            .iter()
            .position(|entry| entry.path == target)
        {
            self.workspace.tree.pending_select = None;
            self.workspace.tree.pending_reveal = None;
            self.workspace.tree.selected = Some(index);
            self.status.text = format!("revealed {}", target.display());
            return;
        }
        let mut next = None;
        for ancestor in target.ancestors().skip(1) {
            if let Some(index) = self
                .workspace
                .tree
                .entries
                .iter()
                .position(|entry| entry.path == ancestor)
            {
                if self.workspace.tree.entries[index].is_dir
                    && !self.workspace.tree.entries[index].expanded
                {
                    next = Some((index, ancestor.to_path_buf()));
                }
                break;
            }
        }
        let Some((index, ancestor)) = next else {
            self.workspace.tree.pending_select = None;
            self.workspace.tree.pending_reveal = None;
            self.status.text = format!("E: tree reveal: {} is not visible", target.display());
            self.file.dirty = true;
            return;
        };
        if self.workspace.tree.reveal_steps >= 128 {
            self.workspace.tree.pending_select = None;
            self.workspace.tree.pending_reveal = None;
            self.status.text = "E: tree reveal: depth limit reached".to_owned();
            self.file.dirty = true;
            return;
        }
        self.workspace.tree.reveal_steps += 1;
        self.workspace.tree.entries[index].expanded = true;
        let generation = self.workspace.tree.refresh_generation;
        let requested = self.file_worker_for_path(&ancestor).is_some_and(|worker| {
            worker
                .request(FileRequest::List {
                    path: ancestor.clone(),
                    generation,
                })
                .is_ok()
        });
        if !requested {
            let result = self.tree_entries(&ancestor);
            self.handle_tree_listing(generation, ancestor, result);
        }
    }

    pub(crate) fn start_tree_find(&mut self) {
        self.workspace.tree.find = Some(TreeFindState::default());
        self.ui.input_mode = InputMode::TreeFind {
            query: anvil_session::ui::InputBuffer::new(""),
        };
        self.ui.focus.tree = true;
        self.ui.focus.split = false;
        self.status.text = "tree find".to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn cancel_tree_find(&mut self) {
        self.workspace.tree.find = None;
        self.ui.input_mode = InputMode::Normal;
        self.status.text = "tree find cancelled".to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn commit_tree_find(&mut self) {
        self.ui.input_mode = InputMode::Normal;
        self.refresh_tree_find();
        self.file.dirty = true;
    }

    pub(crate) fn tree_find_move(&mut self, delta: isize) {
        let Some(find) = self.workspace.tree.find.as_mut() else {
            return;
        };
        if find.matches.is_empty() {
            return;
        }
        let length = find.matches.len();
        let current = find.current as isize + delta;
        find.current = current.rem_euclid(length as isize) as usize;
        self.workspace.tree.selected = Some(find.matches[find.current]);
        self.status.text = format!("tree find: {}/{}", find.current + 1, find.matches.len());
        self.file.dirty = true;
    }

    pub(crate) fn refresh_tree_find(&mut self) {
        let query = match &self.ui.input_mode {
            InputMode::TreeFind { query } => query.text().to_owned(),
            _ => self
                .workspace
                .tree
                .find
                .as_ref()
                .map(|find| find.query.clone())
                .unwrap_or_default(),
        };
        let query = query.to_lowercase();
        if query.is_empty() {
            if let Some(find) = self.workspace.tree.find.as_mut() {
                find.query.clear();
                find.matches.clear();
                find.current = 0;
            }
            return;
        }
        let previous = self
            .workspace
            .tree
            .selected
            .and_then(|index| self.workspace.tree.entries.get(index))
            .map(|entry| entry.path.clone());
        let mut matches = Vec::new();
        for (index, entry) in self.workspace.tree.entries.iter().enumerate() {
            if self.tree_entry_label(entry).to_lowercase().contains(&query) {
                matches.push(index);
                if matches.len() >= MAX_TREE_FIND_MATCHES {
                    break;
                }
            }
        }
        let current = previous
            .as_ref()
            .and_then(|path| {
                matches
                    .iter()
                    .position(|index| self.workspace.tree.entries[*index].path == *path)
            })
            .unwrap_or(0);
        if let Some(index) = matches.get(current) {
            self.workspace.tree.selected = Some(*index);
        }
        self.workspace.tree.find = Some(TreeFindState {
            query,
            matches: matches.clone(),
            current,
        });
        self.status.text = if matches.is_empty() {
            "tree find: no matches".to_owned()
        } else if matches.len() >= MAX_TREE_FIND_MATCHES {
            format!("tree find: {MAX_TREE_FIND_MATCHES}+")
        } else {
            format!("tree find: {}/{}", current + 1, matches.len())
        };
    }

    fn apply_pending_tree_selection(&mut self) {
        if self.workspace.tree.pending_reveal.is_some() {
            self.advance_pending_reveal();
            return;
        }
        let Some(path) = self.workspace.tree.pending_select.clone() else {
            return;
        };
        if let Some(index) = self
            .workspace
            .tree
            .entries
            .iter()
            .position(|entry| entry.path == path)
        {
            self.workspace.tree.pending_select = None;
            self.workspace.tree.selected = Some(index);
        }
    }
}

//! Workspace tree and fuzzy file finder state.

use super::{FuzzyState, Model, PreviewState, TreeEntry};
use crate::file_worker::{FileEvent, FileRequest, FileWorker, MAX_TREE_WATCH_DIRS, PreviewData};
use anvil_ipc::DirEntry;
use anvil_session::ui::{CreateKind, InputMode};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

mod fuzzy;
mod mutations;
mod tree;

impl Model {
    fn tree_entry_visible(&self, name: &str, is_dir: bool) -> bool {
        !is_dir
            || self.tree.show_hidden
            || !name.starts_with('.')
            || matches!(name, ".github" | ".cargo")
    }

    pub(crate) fn ensure_file_worker(&mut self) -> bool {
        if self.workspace.file_worker.is_none() {
            let Some(session) = self.workspace.gateway_session.as_mut() else {
                return false;
            };
            let Ok(client) = session.connect_lsp() else {
                return false;
            };
            self.workspace.file_worker =
                Some(FileWorker::start(client, self.workspace.path.clone()));
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
                .map(|client| FileWorker::start(client, root.clone()));
            if let Some(worker) = worker {
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
    pub(crate) fn root_label(&self, root: &Path) -> String {
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let duplicates = self
            .workspace
            .roots
            .list()
            .iter()
            .filter(|candidate| candidate.path.file_name() == root.file_name())
            .count();
        if duplicates <= 1 {
            return name;
        }
        let Some(parent) = root.parent().and_then(Path::file_name) else {
            return root.display().to_string();
        };
        let short = format!("{}/{}", parent.to_string_lossy(), name);
        let short_duplicates = self
            .workspace
            .roots
            .list()
            .iter()
            .filter(|candidate| {
                candidate.path.file_name() == root.file_name()
                    && candidate.path.parent().and_then(Path::file_name) == Some(parent)
            })
            .count();
        if short_duplicates > 1 {
            root.display().to_string()
        } else {
            short
        }
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
        let label = self.root_label(&entry.path);
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
            if self.tree.entries.iter().any(|entry| entry.path == root) {
                continue;
            }
            self.tree.entries.push(TreeEntry {
                path: root,
                depth: 0,
                is_dir: true,
                ino: 0,
                expanded: false,
            });
        }
    }

    fn file_worker_for_path(&self, path: &Path) -> Option<&FileWorker> {
        match self.secondary_root(path) {
            Some(root) => self.workspace.root_file_workers.get(&root),
            None => self.workspace.file_worker.as_ref(),
        }
    }

    fn refresh_root_tree(&mut self, root: &Path) {
        let mut paths = vec![root.to_path_buf()];
        paths.extend(
            self.tree
                .entries
                .iter()
                .filter(|entry| entry.expanded && entry.path.starts_with(root))
                .map(|entry| entry.path.clone()),
        );
        paths.sort();
        paths.dedup();
        if let Some(worker) = self.workspace.root_file_workers.get(root) {
            for path in paths {
                let _ = worker.request(FileRequest::List(path));
            }
        } else {
            for path in paths {
                let result = self.tree_entries(&path);
                self.handle_tree_listing(path, result);
            }
        }
    }

    fn handle_tree_listing(&mut self, path: PathBuf, result: Result<Vec<DirEntry>, String>) {
        if self.workspace.roots.owning_root(&path).is_none() {
            return;
        }
        let entries = match result {
            Ok(entries) => entries,
            Err(error) => {
                self.status.text = format!("E: tree: {error}");
                return;
            }
        };
        let entries: Vec<DirEntry> = entries
            .into_iter()
            .filter(|entry| self.tree_entry_visible(&entry.name, entry.is_dir))
            .collect();
        if path == self.workspace.path {
            let preserved = self
                .tree
                .entries
                .iter()
                .filter(|entry| self.secondary_root(&entry.path).is_some())
                .cloned()
                .collect::<Vec<_>>();
            self.tree.entries = entries
                .into_iter()
                .map(|entry| TreeEntry {
                    path: path.join(entry.name),
                    depth: 0,
                    is_dir: entry.is_dir,
                    ino: entry.ino,
                    expanded: false,
                })
                .collect();
            self.tree.entries.extend(preserved);
            self.ensure_root_tree_nodes();
            if self.tree.selected.is_none() && !self.tree.entries.is_empty() {
                self.tree.selected = Some(0);
            }
            self.apply_pending_tree_selection();
        } else if let Some(index) = self
            .tree
            .entries
            .iter()
            .position(|entry| entry.path == path)
            && self.tree.entries[index].expanded
        {
            let depth = self.tree.entries[index].depth + 1;
            let parent_depth = self.tree.entries[index].depth;
            let children: Vec<TreeEntry> = entries
                .into_iter()
                .map(|entry| TreeEntry {
                    path: path.join(entry.name),
                    depth,
                    is_dir: entry.is_dir,
                    ino: entry.ino,
                    expanded: false,
                })
                .collect();
            let mut end = index + 1;
            while self
                .tree
                .entries
                .get(end)
                .is_some_and(|child| child.depth > parent_depth)
            {
                end += 1;
            }
            self.tree.entries.splice(index + 1..end, children);
        }
        self.status.text = "file tree ready".to_owned();
        self.sync_tree_watches();
        self.apply_pending_tree_selection();
    }

    pub(crate) fn request_git_status(&mut self) {
        self.git.status_generation = self.git.status_generation.wrapping_add(1);
        let generation = self.git.status_generation;
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
        self.git.status.clear();
        self.git.status_pending = Some(generation);
        self.git.status_expected = expected;
        self.git.status_received = 0;
    }

    fn handle_git_status(
        &mut self,
        generation: u64,
        result: Result<std::collections::HashMap<PathBuf, char>, String>,
    ) {
        if self.git.status_pending != Some(generation) {
            return;
        }
        self.git.status_received = self.git.status_received.saturating_add(1);
        match result {
            Ok(statuses) => self.git.status.extend(statuses),
            Err(error) => {
                self.status.text = format!("E: git status: {error}");
                self.file.dirty = true;
            }
        }
        if self.git.status_received >= self.git.status_expected {
            self.git.status_pending = None;
            self.git.status_expected = 0;
            self.git.status_received = 0;
            self.file.dirty = true;
        }
    }

    pub(crate) fn poll_file_worker(&mut self) {
        let events = self
            .workspace
            .file_worker
            .as_ref()
            .map(|worker| worker.poll())
            .unwrap_or_default();
        for event in events {
            match event {
                FileEvent::Listed(path, result) => self.handle_tree_listing(path, result),
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
                FileEvent::GitConflictsText { generation, result } => {
                    self.handle_git_conflicts_text(generation, result)
                }
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
                    if !paths.is_empty() {
                        self.refresh_tree_background();
                    }
                }
                FileEvent::TreeWatchUnavailable(error) => {
                    self.tree.stale = true;
                    self.status.text = format!("E: tree watcher: {error}");
                }
                FileEvent::TreeSnapshot {
                    generation,
                    listings,
                } => self.apply_tree_snapshot(generation, listings),
                FileEvent::GitStatus {
                    generation, result, ..
                } => {
                    self.handle_git_status(generation, result);
                }
            }
            self.file.dirty = true;
        }
        let roots = self
            .workspace
            .root_file_workers
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for root in roots {
            let events = self
                .workspace
                .root_file_workers
                .get_mut(&root)
                .map(|worker| worker.poll())
                .unwrap_or_default();
            for event in events {
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
                    FileEvent::ProjectSearch {
                        query,
                        generation,
                        result,
                    } => {
                        self.handle_project_search(Some(root.clone()), query, generation, result);
                    }
                    FileEvent::Listed(path, result) => self.handle_tree_listing(path, result),
                    FileEvent::GitStatus {
                        generation, result, ..
                    } => self.handle_git_status(generation, result),
                    FileEvent::TreeChanged { .. } => self.refresh_root_tree(&root),
                    FileEvent::TreeWatchUnavailable(error) => {
                        self.tree.stale = true;
                        self.status.text = format!("E: tree watcher: {error}");
                    }
                    _ => {}
                }
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
            if name == ".git" || name == "target" {
                continue;
            }
            entries.push(DirEntry {
                is_dir: entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false),
                name,
                ino: 0,
            });
        }
        entries.sort_by(|left, right| {
            right
                .is_dir
                .cmp(&left.is_dir)
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(entries)
    }

    pub(crate) fn toggle_hidden(&mut self) {
        self.tree.show_hidden = !self.tree.show_hidden;
        if self.tree.visible {
            self.tree.entries.clear();
            self.tree.selected = None;
            let workspace = self.workspace.path.clone();
            if self.ensure_file_worker() {
                if let Some(worker) = self.workspace.file_worker.as_ref() {
                    let _ = worker.request(FileRequest::List(workspace));
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
                        let _ = worker.request(FileRequest::List(root));
                    }
                }
            } else if let Ok(entries) = self.tree_entries(&workspace) {
                self.tree.entries = entries
                    .into_iter()
                    .filter(|entry| self.tree_entry_visible(&entry.name, entry.is_dir))
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
            self.tree.selected = (!self.tree.entries.is_empty()).then_some(0);
        }
        self.status.text = if self.tree.show_hidden {
            "hidden files shown".to_owned()
        } else {
            "hidden files hidden".to_owned()
        };
        self.sync_tree_watches();
        self.request_git_status();
        self.file.dirty = true;
    }

    fn valid_tree_name(name: &str) -> bool {
        !name.is_empty()
            && name != "."
            && name != ".."
            && !name.contains('/')
            && !name.contains('\\')
            && !name.chars().any(char::is_control)
    }

    fn tree_target_directory(&self) -> PathBuf {
        let Some(index) = self.tree.selected else {
            return self.workspace.path.clone();
        };
        let Some(entry) = self.tree.entries.get(index) else {
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

    fn apply_pending_tree_selection(&mut self) {
        let Some(path) = self.tree.pending_select.take() else {
            return;
        };
        if let Some(index) = self
            .tree
            .entries
            .iter()
            .position(|entry| entry.path == path)
        {
            self.tree.selected = Some(index);
        }
    }

    fn expanded_tree_paths(&self) -> BTreeSet<PathBuf> {
        self.tree
            .entries
            .iter()
            .filter(|entry| entry.is_dir && entry.expanded)
            .map(|entry| entry.path.clone())
            .collect()
    }

    fn sync_tree_watches(&mut self) {
        let workspace = self.workspace.path.clone();
        let mut directories: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
        directories
            .entry(workspace.clone())
            .or_default()
            .push(workspace.clone());
        for root in self.workspace.roots.list() {
            if root.trusted && root.path != workspace {
                directories
                    .entry(root.path.clone())
                    .or_default()
                    .push(root.path.clone());
            }
        }
        for path in self.expanded_tree_paths() {
            if path.starts_with(&workspace) {
                directories.entry(workspace.clone()).or_default().push(path);
            } else if let Some(root) = self.secondary_root(&path)
                && self
                    .workspace
                    .roots
                    .owning_root(&path)
                    .is_some_and(|root| root.trusted)
            {
                directories.entry(root).or_default().push(path);
            }
        }
        let per_root = (MAX_TREE_WATCH_DIRS / directories.len().max(1)).max(1);
        let mut limited = false;
        for paths in directories.values_mut() {
            paths.sort();
            paths.dedup();
            if paths.len() > per_root {
                paths.truncate(per_root);
                limited = true;
            }
        }
        if limited && !self.tree.watch_limited {
            self.status.text =
                format!("tree watch limit reached ({MAX_TREE_WATCH_DIRS}); auto-refresh limited");
        }
        self.tree.watch_limited = limited;
        if !self.ensure_file_worker() {
            self.tree.stale = true;
            return;
        }
        let mut sent = 0usize;
        let mut expected = 0usize;
        for (root, paths) in directories {
            expected += 1;
            let request = if root == workspace {
                self.workspace
                    .file_worker
                    .as_ref()
                    .map(|worker| worker.request(FileRequest::WatchDirs(paths)))
            } else {
                self.workspace
                    .root_file_workers
                    .get(&root)
                    .map(|worker| worker.request(FileRequest::WatchDirs(paths)))
            };
            if matches!(request, Some(Ok(()))) {
                sent += 1;
            }
        }
        self.tree.stale = sent < expected;
    }

    fn request_tree_refresh(&mut self, select: Option<PathBuf>) {
        if select.is_some() {
            self.tree.pending_select = select;
        }
        if self.tree.refresh_in_flight {
            self.tree.refresh_queued = true;
            return;
        }
        let expanded = self
            .expanded_tree_paths()
            .into_iter()
            .filter(|path| path.starts_with(&self.workspace.path))
            .collect();
        if self.ensure_file_worker() {
            self.tree.refresh_generation = self.tree.refresh_generation.wrapping_add(1);
            let generation = self.tree.refresh_generation;
            let request = FileRequest::TreeSnapshot {
                generation,
                root: self.workspace.path.clone(),
                expanded,
            };
            let sent = self
                .workspace
                .file_worker
                .as_ref()
                .is_some_and(|worker| worker.request(request).is_ok());
            self.tree.refresh_in_flight = sent;
            self.tree.stale = !sent;
            return;
        }
        self.rebuild_tree_from_disk();
    }

    fn refresh_tree_background(&mut self) {
        self.request_tree_refresh(None);
    }

    fn rebuild_tree_from_disk(&mut self) {
        let expanded = self
            .expanded_tree_paths()
            .into_iter()
            .filter(|path| path.starts_with(&self.workspace.path))
            .collect::<BTreeSet<_>>();
        let workspace = self.workspace.path.clone();
        let mut listings = Vec::new();
        listings.push((workspace.clone(), self.tree_entries(&workspace)));
        for path in expanded {
            if let Ok(entries) = self.tree_entries(&path) {
                listings.push((path, Ok(entries)));
            }
        }
        self.tree.refresh_generation = self.tree.refresh_generation.wrapping_add(1);
        let generation = self.tree.refresh_generation;
        self.apply_tree_snapshot(generation, listings);
    }

    fn build_tree_entries(
        &self,
        directory: &Path,
        depth: usize,
        listings: &BTreeMap<PathBuf, Result<Vec<DirEntry>, String>>,
        expanded: &BTreeSet<PathBuf>,
        out: &mut Vec<TreeEntry>,
    ) -> bool {
        let Some(Ok(entries)) = listings.get(directory) else {
            return false;
        };
        let mut needs_listing = false;
        for entry in entries {
            if !self.tree_entry_visible(&entry.name, entry.is_dir) {
                continue;
            }
            let path = directory.join(&entry.name);
            let listing = listings.get(&path);
            let is_expanded =
                entry.is_dir && expanded.contains(&path) && !matches!(listing, Some(Err(_)));
            out.push(TreeEntry {
                path: path.clone(),
                depth,
                is_dir: entry.is_dir,
                ino: entry.ino,
                expanded: is_expanded,
            });
            if is_expanded {
                match listing {
                    Some(Ok(_)) => {
                        needs_listing |=
                            self.build_tree_entries(&path, depth + 1, listings, expanded, out);
                    }
                    Some(Err(_)) => {}
                    None => needs_listing = true,
                }
            }
        }
        needs_listing
    }

    fn select_tree_path(
        tree: &[TreeEntry],
        target: Option<&Path>,
        fallback_index: Option<usize>,
        identity: Option<u64>,
    ) -> Option<usize> {
        if let Some(path) = target {
            if let Some(index) = tree.iter().position(|entry| entry.path == path) {
                return Some(index);
            }
            if let Some(ino) = identity
                && let Some(index) = tree.iter().position(|entry| entry.ino == ino)
            {
                return Some(index);
            }
            for ancestor in path.ancestors().skip(1) {
                if let Some(index) = tree.iter().position(|entry| entry.path == ancestor) {
                    return Some(index);
                }
            }
        }
        fallback_index
            .map(|index| index.min(tree.len().saturating_sub(1)))
            .filter(|_| !tree.is_empty())
            .or_else(|| (!tree.is_empty()).then_some(0))
    }

    fn apply_tree_snapshot(
        &mut self,
        generation: u64,
        listings: Vec<(PathBuf, Result<Vec<DirEntry>, String>)>,
    ) {
        if generation != self.tree.refresh_generation {
            return;
        }
        self.tree.refresh_in_flight = false;
        let previous = self
            .tree
            .selected
            .and_then(|index| self.tree.entries.get(index));
        let previous_path = previous.map(|entry| entry.path.clone());
        let previous_ino = previous.map(|entry| entry.ino).filter(|ino| *ino != 0);
        let previous_index = self.tree.selected;
        let expanded = self.expanded_tree_paths();
        let listings: BTreeMap<_, _> = listings.into_iter().collect();
        match listings.get(&self.workspace.path) {
            None => {
                self.status.text = "E: tree: missing root listing".to_owned();
                self.tree.stale = true;
                self.finish_tree_refresh();
                return;
            }
            Some(Err(error)) => {
                self.status.text = format!("E: tree: {error}");
                self.tree.stale = true;
                self.finish_tree_refresh();
                return;
            }
            Some(Ok(_)) => {}
        }
        let preserved = self
            .tree
            .entries
            .iter()
            .filter(|entry| self.secondary_root(&entry.path).is_some())
            .cloned()
            .collect::<Vec<_>>();
        let mut tree = Vec::new();
        let needs_listing =
            self.build_tree_entries(&self.workspace.path, 0, &listings, &expanded, &mut tree);
        tree.extend(preserved);
        let target = self.tree.pending_select.take().or(previous_path);
        self.tree.entries = tree;
        self.ensure_root_tree_nodes();
        self.tree.selected = Self::select_tree_path(
            &self.tree.entries,
            target.as_deref(),
            previous_index,
            previous_ino,
        );
        self.tree.stale = false;
        if needs_listing {
            self.tree.refresh_queued = true;
        }
        self.sync_tree_watches();
        self.request_git_status();
        self.finish_tree_refresh();
    }

    fn finish_tree_refresh(&mut self) {
        if self.tree.refresh_queued {
            self.tree.refresh_queued = false;
            self.refresh_tree_background();
        }
    }

    pub(crate) fn refresh_tree(&mut self, select: Option<PathBuf>) {
        self.tree.visible = true;
        self.ui.focus.tree = true;
        let refresh_all = select.is_none();
        let secondary = select.as_deref().and_then(|path| self.secondary_root(path));
        if let Some(root) = secondary {
            self.tree.pending_select = select;
            self.refresh_root_tree(&root);
        } else {
            self.request_tree_refresh(select);
            if refresh_all {
                let roots = self
                    .workspace
                    .roots
                    .list()
                    .iter()
                    .filter(|root| root.trusted && root.path != self.workspace.path)
                    .map(|root| root.path.clone())
                    .collect::<Vec<_>>();
                for root in roots {
                    self.refresh_root_tree(&root);
                }
            }
        }
        self.request_git_status();
        self.file.dirty = true;
    }

    fn tree_busy(&mut self) -> bool {
        if self.status.busy.is_some() {
            self.status.text = "E: file operation already in progress".to_owned();
            self.file.dirty = true;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EditorState;

    #[test]
    fn snapshot_keeps_expansion_missing_from_listing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().to_path_buf();
        let src = workspace.join("src");
        fs::create_dir(&src).expect("src");
        let mut model = Model::new(EditorState::from_text(""), None);
        model.workspace.path = workspace.clone();
        model.tree.entries = vec![TreeEntry {
            path: src.clone(),
            depth: 0,
            is_dir: true,
            ino: 0,
            expanded: true,
        }];
        model.tree.selected = Some(0);
        model.tree.refresh_generation = 1;
        model.apply_tree_snapshot(
            1,
            vec![(
                workspace,
                Ok(vec![DirEntry {
                    name: "src".to_owned(),
                    is_dir: true,
                    ino: 0,
                }]),
            )],
        );
        assert!(
            model
                .tree
                .entries
                .iter()
                .any(|entry| entry.path == src && entry.expanded)
        );
    }

    #[test]
    fn snapshot_follows_external_rename_by_inode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().to_path_buf();
        let mut model = Model::new(EditorState::from_text(""), None);
        model.workspace.path = workspace.clone();
        model.tree.entries = vec![TreeEntry {
            path: workspace.join("old.rs"),
            depth: 0,
            is_dir: false,
            ino: 42,
            expanded: false,
        }];
        model.tree.selected = Some(0);
        model.tree.refresh_generation = 1;
        model.apply_tree_snapshot(
            1,
            vec![(
                workspace.clone(),
                Ok(vec![DirEntry {
                    name: "new.rs".to_owned(),
                    is_dir: false,
                    ino: 42,
                }]),
            )],
        );
        let selected = model.tree.selected.expect("selection");
        assert_eq!(model.tree.entries[selected].path, workspace.join("new.rs"));
    }

    #[test]
    fn git_status_merges_primary_and_secondary_roots() {
        let dir = tempfile::tempdir().expect("tempdir");
        let primary = dir.path().join("primary.rs");
        let secondary = dir.path().join("secondary.rs");
        let mut model = Model::new(EditorState::from_text(""), None);
        model.git.status_pending = Some(7);
        model.git.status_expected = 2;
        model.git.status_received = 0;
        model.git.status.clear();

        model.handle_git_status(
            7,
            Ok(std::collections::HashMap::from([(secondary.clone(), 'M')])),
        );
        assert_eq!(model.git.status_pending, Some(7));

        model.handle_git_status(
            7,
            Ok(std::collections::HashMap::from([(primary.clone(), 'A')])),
        );
        assert_eq!(model.git.status_pending, None);
        assert_eq!(model.git.status.get(&primary), Some(&'A'));
        assert_eq!(model.git.status.get(&secondary), Some(&'M'));
    }

    #[test]
    fn untrusted_root_cannot_expand() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("primary");
        let root = dir.path().join("secondary");
        std::fs::create_dir_all(&workspace).expect("mkdir");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::write(root.join("secret.rs"), "").expect("write");
        let workspace = workspace.canonicalize().expect("canonical");
        let root = root.canonicalize().expect("canonical");
        let mut model = Model::new(EditorState::from_text(""), None);
        model.workspace.path = workspace;
        model.workspace.roots.add(&root, false).expect("add root");
        model.open_tree();
        let index = model
            .tree
            .entries
            .iter()
            .position(|entry| entry.path == root)
            .expect("root node");
        model.tree.selected = Some(index);
        model.tree_toggle();
        assert!(
            !model
                .tree
                .entries
                .iter()
                .any(|entry| entry.path == root.join("secret.rs"))
        );
        assert!(model.status.text.contains("untrusted workspace root"));
        let entry = &model.tree.entries[index];
        assert!(model.tree_entry_untrusted(entry));
        assert!(model.tree_entry_label(entry).contains("[untrusted]"));
    }

    #[test]
    fn root_labels_disambiguate_duplicate_basenames() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("a/repo");
        let second = dir.path().join("b/repo");
        let unique = dir.path().join("unique");
        for path in [&first, &second, &unique] {
            std::fs::create_dir_all(path).expect("mkdir");
        }
        let first = first.canonicalize().expect("first");
        let second = second.canonicalize().expect("second");
        let unique = unique.canonicalize().expect("unique");
        let mut model = Model::new(EditorState::from_text(""), None);
        model.workspace.path = first.clone();
        model.workspace.roots = crate::workspace_roots::WorkspaceRoots::new(&first).expect("roots");
        model
            .workspace
            .roots
            .add(&second, true)
            .expect("second root");
        model
            .workspace
            .roots
            .add(&unique, true)
            .expect("unique root");

        assert_eq!(model.root_label(&first), "a/repo");
        assert_eq!(model.root_label(&second), "b/repo");
        assert_eq!(model.root_label(&unique), "unique");
    }

    #[test]
    fn removing_a_root_cancels_pending_project_search() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.project.search_pending = Some(9);
        model.project.search_expected = 2;
        model.stop_root_file_worker(Path::new("/tmp/removed-root"));
        assert!(model.project.search_pending.is_none());
    }

    #[test]
    fn primary_refresh_preserves_expanded_secondary_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("primary");
        let root = dir.path().join("secondary");
        std::fs::create_dir_all(&workspace).expect("mkdir");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::write(workspace.join("main.rs"), "").expect("write");
        std::fs::write(root.join("lib.rs"), "").expect("write");
        let workspace = workspace.canonicalize().expect("canonical");
        let root = root.canonicalize().expect("canonical");
        let mut model = Model::new(EditorState::from_text(""), None);
        model.workspace.path = workspace.clone();
        model.workspace.roots.add(&root, true).expect("add root");
        model.open_tree();
        let root_index = model
            .tree
            .entries
            .iter()
            .position(|entry| entry.path == root)
            .expect("root node");
        model.tree.selected = Some(root_index);
        model.tree_toggle();
        assert!(
            model
                .tree
                .entries
                .iter()
                .any(|entry| entry.path == root.join("lib.rs"))
        );

        model.refresh_tree(None);

        assert!(model.tree.entries.iter().any(|entry| entry.path == root));
        assert!(
            model
                .tree
                .entries
                .iter()
                .any(|entry| entry.path == root.join("lib.rs"))
        );
        assert!(
            model
                .tree
                .entries
                .iter()
                .any(|entry| entry.path == workspace.join("main.rs"))
        );
    }

    #[test]
    fn tree_watch_cap_is_reported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut model = Model::new(EditorState::from_text(""), None);
        model.workspace.path = dir.path().to_path_buf();
        let workspace = model.workspace.path.clone();
        model.tree.entries = (0..MAX_TREE_WATCH_DIRS + 1)
            .map(|index| TreeEntry {
                path: workspace.join(format!("d{index}")),
                depth: 0,
                is_dir: true,
                ino: 0,
                expanded: true,
            })
            .collect();
        model.sync_tree_watches();
        assert!(model.tree.watch_limited);
        assert!(model.status.text.contains("watch limit"));
    }
}

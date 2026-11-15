//! Workspace tree and fuzzy file finder state.

use super::{FuzzyState, Model, PreviewState, TreeEntry};
use anvil_ipc::DirEntry;
use anvil_session::ui::{CreateKind, InputMode};
use anvil_workspace::{FileEvent, FileRequest, FileWorker, MAX_TREE_WATCH_DIRS, PreviewData};
use std::{
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
            let _ = worker.request(FileRequest::SetWatchEnabled(
                self.config.settings.editor.file_watching,
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
                let _ = worker.request(FileRequest::SetWatchEnabled(
                    self.config.settings.editor.file_watching,
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
        }
    }

    pub(crate) fn poll_file_worker(&mut self) {
        let poll = self.workspace.poll();
        for event in poll.file_events {
            match event {
                FileEvent::Listed(path, result) => self.handle_tree_listing(path, result),
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
                    if !paths.is_empty() {
                        self.refresh_tree_background();
                    }
                }
                FileEvent::TreeWatchUnavailable(error) => {
                    self.workspace.tree.stale = true;
                    self.status.text = format!("E: tree watcher: {error}");
                }
                FileEvent::Oversized { message } => {
                    self.workspace.tree.stale = true;
                    self.status.text = format!("E: file worker: {message}");
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
                FileEvent::Listed(path, result) => self.handle_tree_listing(path, result),
                FileEvent::GitStatus {
                    generation, result, ..
                } => self.handle_git_status(generation, result),
                FileEvent::ExternalChanged { path } => self.handle_external_change(path),
                FileEvent::TreeChanged { .. } => self.refresh_root_tree(&root),
                FileEvent::TreeWatchUnavailable(error) => {
                    self.workspace.tree.stale = true;
                    self.status.text = format!("E: tree watcher: {error}");
                }
                FileEvent::Oversized { message } => {
                    self.workspace.tree.stale = true;
                    self.status.text = format!("E: file worker: {message}");
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
        self.workspace.tree.show_hidden = !self.workspace.tree.show_hidden;
        if self.workspace.tree.visible {
            self.workspace.tree.entries.clear();
            self.workspace.tree.selected = None;
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

    fn apply_pending_tree_selection(&mut self) {
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

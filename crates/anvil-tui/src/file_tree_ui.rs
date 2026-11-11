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
            || self.show_hidden
            || !name.starts_with('.')
            || matches!(name, ".github" | ".cargo")
    }

    pub(crate) fn ensure_file_worker(&mut self) -> bool {
        if self.file_worker.is_some() {
            return true;
        }
        let Some(session) = self.gateway_session.as_mut() else {
            return false;
        };
        let Ok(client) = session.connect_lsp() else {
            return false;
        };
        self.file_worker = Some(FileWorker::start(client, self.workspace.clone()));
        true
    }

    pub(crate) fn poll_file_worker(&mut self) {
        let events = self
            .file_worker
            .as_ref()
            .map(|worker| worker.poll())
            .unwrap_or_default();
        for event in events {
            match event {
                FileEvent::Listed(path, Ok(entries)) => {
                    let entries: Vec<DirEntry> = entries
                        .into_iter()
                        .filter(|entry| self.tree_entry_visible(&entry.name, entry.is_dir))
                        .collect();
                    if path == self.workspace {
                        self.tree = entries
                            .into_iter()
                            .map(|entry| TreeEntry {
                                path: path.join(entry.name),
                                depth: 0,
                                is_dir: entry.is_dir,
                                ino: entry.ino,
                                expanded: false,
                            })
                            .collect();
                        if self.tree_selected.is_none() && !self.tree.is_empty() {
                            self.tree_selected = Some(0);
                        }
                        self.apply_pending_tree_selection();
                    } else if let Some(index) =
                        self.tree.iter().position(|entry| entry.path == path)
                    {
                        if self.tree[index].expanded {
                            let depth = self.tree[index].depth + 1;
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
                            self.tree.splice(index + 1..index + 1, children);
                        }
                    }
                    self.status = "file tree ready".to_owned();
                    self.sync_tree_watches();
                }
                FileEvent::Listed(_, Err(error)) => self.status = format!("E: tree: {error}"),
                FileEvent::Files(Ok(files)) => {
                    if let Some(fuzzy) = self.fuzzy.as_mut() {
                        fuzzy.entries = files;
                        self.fuzzy_refresh();
                    }
                    self.status = "files ready".to_owned();
                }
                FileEvent::Files(Err(error)) => self.status = format!("E: files: {error}"),
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
                FileEvent::GitDiffText { generation, result } => {
                    self.handle_git_diff_text(generation, result)
                }
                FileEvent::Transferred {
                    from,
                    to,
                    copy,
                    result,
                } => {
                    self.busy = None;
                    match result {
                        Ok(()) => {
                            if !copy {
                                self.remap_open_path(&from, &to);
                            }
                            self.status = if copy {
                                format!("copied {}", to.display())
                            } else {
                                format!("moved {}", to.display())
                            };
                            self.refresh_tree(Some(to));
                            self.finish_busy_progress(None);
                        }
                        Err(error) => {
                            self.status = format!("E: {error}");
                            self.dirty = true;
                            self.finish_busy_progress(Some(&error));
                        }
                    }
                }
                FileEvent::Removed { path, result } => {
                    self.busy = None;
                    match result {
                        Ok(()) => {
                            self.mark_missing(&path);
                            self.status = format!("deleted {}", path.display());
                            self.refresh_tree(None);
                            self.finish_busy_progress(None);
                        }
                        Err(error) => {
                            self.status = format!("E: {error}");
                            self.dirty = true;
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
                    self.tree_stale = true;
                    self.status = format!("E: tree watcher: {error}");
                }
                FileEvent::TreeSnapshot {
                    generation,
                    listings,
                } => self.apply_tree_snapshot(generation, listings),
            }
            self.dirty = true;
        }
    }

    fn tree_entries(&mut self, path: &Path) -> Result<Vec<DirEntry>, String> {
        if let Some(gateway) = self.gateway.as_mut() {
            let relative = if path == self.workspace {
                ".".to_owned()
            } else {
                path.strip_prefix(&self.workspace)
                    .map_err(|_| format!("path is outside workspace {}", path.display()))?
                    .to_string_lossy()
                    .into_owned()
            };
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
        self.show_hidden = !self.show_hidden;
        if self.tree_visible {
            self.tree.clear();
            self.tree_selected = None;
            let workspace = self.workspace.clone();
            if self.ensure_file_worker() {
                if let Some(worker) = self.file_worker.as_ref() {
                    let _ = worker.request(FileRequest::List(workspace));
                }
            } else if let Ok(entries) = self.tree_entries(&workspace) {
                self.tree = entries
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
            }
            self.tree_selected = (!self.tree.is_empty()).then_some(0);
        }
        self.status = if self.show_hidden {
            "hidden files shown".to_owned()
        } else {
            "hidden files hidden".to_owned()
        };
        self.sync_tree_watches();
        self.dirty = true;
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
        let Some(index) = self.tree_selected else {
            return self.workspace.clone();
        };
        let Some(entry) = self.tree.get(index) else {
            return self.workspace.clone();
        };
        if entry.is_dir {
            entry.path.clone()
        } else {
            entry
                .path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.workspace.clone())
        }
    }

    fn tree_entry_exists(&mut self, directory: &Path, name: &str) -> Result<bool, String> {
        self.tree_entries(directory)
            .map(|entries| entries.iter().any(|entry| entry.name == name))
    }

    fn apply_pending_tree_selection(&mut self) {
        let Some(path) = self.tree_pending_select.take() else {
            return;
        };
        if let Some(index) = self.tree.iter().position(|entry| entry.path == path) {
            self.tree_selected = Some(index);
        }
    }

    fn expanded_tree_paths(&self) -> BTreeSet<PathBuf> {
        self.tree
            .iter()
            .filter(|entry| entry.is_dir && entry.expanded)
            .map(|entry| entry.path.clone())
            .collect()
    }

    fn sync_tree_watches(&mut self) {
        let mut directories = vec![self.workspace.clone()];
        directories.extend(self.expanded_tree_paths());
        let limited = directories.len() > MAX_TREE_WATCH_DIRS;
        if limited {
            directories.truncate(MAX_TREE_WATCH_DIRS);
        }
        if limited && !self.tree_watch_limited {
            self.status =
                format!("tree watch limit reached ({MAX_TREE_WATCH_DIRS}); auto-refresh limited");
        }
        self.tree_watch_limited = limited;
        if !self.ensure_file_worker() {
            self.tree_stale = true;
            return;
        }
        let sent = self
            .file_worker
            .as_ref()
            .is_some_and(|worker| worker.request(FileRequest::WatchDirs(directories)).is_ok());
        self.tree_stale = !sent;
    }

    fn request_tree_refresh(&mut self, select: Option<PathBuf>) {
        if select.is_some() {
            self.tree_pending_select = select;
        }
        if self.tree_refresh_in_flight {
            self.tree_refresh_queued = true;
            return;
        }
        let expanded = self.expanded_tree_paths().into_iter().collect();
        if self.ensure_file_worker() {
            self.tree_refresh_generation = self.tree_refresh_generation.wrapping_add(1);
            let generation = self.tree_refresh_generation;
            let request = FileRequest::TreeSnapshot {
                generation,
                root: self.workspace.clone(),
                expanded,
            };
            let sent = self
                .file_worker
                .as_ref()
                .is_some_and(|worker| worker.request(request).is_ok());
            self.tree_refresh_in_flight = sent;
            self.tree_stale = !sent;
            return;
        }
        self.rebuild_tree_from_disk();
    }

    fn refresh_tree_background(&mut self) {
        self.request_tree_refresh(None);
    }

    fn rebuild_tree_from_disk(&mut self) {
        let expanded = self.expanded_tree_paths();
        let workspace = self.workspace.clone();
        let mut listings = Vec::new();
        listings.push((workspace.clone(), self.tree_entries(&workspace)));
        for path in expanded {
            if let Ok(entries) = self.tree_entries(&path) {
                listings.push((path, Ok(entries)));
            }
        }
        self.tree_refresh_generation = self.tree_refresh_generation.wrapping_add(1);
        let generation = self.tree_refresh_generation;
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
        if generation != self.tree_refresh_generation {
            return;
        }
        self.tree_refresh_in_flight = false;
        let previous = self.tree_selected.and_then(|index| self.tree.get(index));
        let previous_path = previous.map(|entry| entry.path.clone());
        let previous_ino = previous.map(|entry| entry.ino).filter(|ino| *ino != 0);
        let previous_index = self.tree_selected;
        let expanded = self.expanded_tree_paths();
        let listings: BTreeMap<_, _> = listings.into_iter().collect();
        match listings.get(&self.workspace) {
            None => {
                self.status = "E: tree: missing root listing".to_owned();
                self.tree_stale = true;
                self.finish_tree_refresh();
                return;
            }
            Some(Err(error)) => {
                self.status = format!("E: tree: {error}");
                self.tree_stale = true;
                self.finish_tree_refresh();
                return;
            }
            Some(Ok(_)) => {}
        }
        let mut tree = Vec::new();
        let needs_listing =
            self.build_tree_entries(&self.workspace, 0, &listings, &expanded, &mut tree);
        let target = self.tree_pending_select.take().or(previous_path);
        self.tree_selected =
            Self::select_tree_path(&tree, target.as_deref(), previous_index, previous_ino);
        self.tree = tree;
        self.tree_stale = false;
        if needs_listing {
            self.tree_refresh_queued = true;
        }
        self.sync_tree_watches();
        self.finish_tree_refresh();
    }

    fn finish_tree_refresh(&mut self) {
        if self.tree_refresh_queued {
            self.tree_refresh_queued = false;
            self.refresh_tree_background();
        }
    }

    pub(crate) fn refresh_tree(&mut self, select: Option<PathBuf>) {
        self.tree_visible = true;
        self.ui.focus.tree = true;
        self.request_tree_refresh(select);
        self.dirty = true;
    }

    fn tree_busy(&mut self) -> bool {
        if self.busy.is_some() {
            self.status = "E: file operation already in progress".to_owned();
            self.dirty = true;
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
        model.workspace = workspace.clone();
        model.tree = vec![TreeEntry {
            path: src.clone(),
            depth: 0,
            is_dir: true,
            ino: 0,
            expanded: true,
        }];
        model.tree_selected = Some(0);
        model.tree_refresh_generation = 1;
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
                .iter()
                .any(|entry| entry.path == src && entry.expanded)
        );
    }

    #[test]
    fn snapshot_follows_external_rename_by_inode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().to_path_buf();
        let mut model = Model::new(EditorState::from_text(""), None);
        model.workspace = workspace.clone();
        model.tree = vec![TreeEntry {
            path: workspace.join("old.rs"),
            depth: 0,
            is_dir: false,
            ino: 42,
            expanded: false,
        }];
        model.tree_selected = Some(0);
        model.tree_refresh_generation = 1;
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
        let selected = model.tree_selected.expect("selection");
        assert_eq!(model.tree[selected].path, workspace.join("new.rs"));
    }

    #[test]
    fn tree_watch_cap_is_reported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut model = Model::new(EditorState::from_text(""), None);
        model.workspace = dir.path().to_path_buf();
        let workspace = model.workspace.clone();
        model.tree = (0..MAX_TREE_WATCH_DIRS + 1)
            .map(|index| TreeEntry {
                path: workspace.join(format!("d{index}")),
                depth: 0,
                is_dir: true,
                ino: 0,
                expanded: true,
            })
            .collect();
        model.sync_tree_watches();
        assert!(model.tree_watch_limited);
        assert!(model.status.contains("watch limit"));
    }
}

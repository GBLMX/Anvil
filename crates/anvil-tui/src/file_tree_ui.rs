//! Workspace tree and fuzzy file finder state.

use super::{CreateKind, FuzzyState, InputMode, Model, TreeEntry};
use crate::file_worker::{FileEvent, FileRequest, FileWorker};
use anvil_ipc::DirEntry;
use std::{
    fs,
    path::{Path, PathBuf},
};

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
                                    expanded: false,
                                })
                                .collect();
                            self.tree.splice(index + 1..index + 1, children);
                        }
                    }
                    self.status = "file tree ready".to_owned();
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
                FileEvent::GitDiff {
                    path,
                    revision,
                    result,
                } => self.handle_git_diff(path, revision, result),
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
                        }
                        Err(error) => {
                            self.status = format!("E: {error}");
                            self.dirty = true;
                        }
                    }
                }
                FileEvent::Removed { path, result } => {
                    self.busy = None;
                    match result {
                        Ok(()) => {
                            self.status = format!("deleted {}", path.display());
                            self.refresh_tree(None);
                        }
                        Err(error) => {
                            self.status = format!("E: {error}");
                            self.dirty = true;
                        }
                    }
                }
                FileEvent::ExternalChanged { path } => self.handle_external_change(path),
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

    pub(crate) fn open_tree(&mut self) {
        if self.tree_visible && self.tree_focus {
            self.tree_focus = false;
            self.tree_visible = false;
            self.dirty = true;
            return;
        }
        if self.tree.is_empty() {
            let workspace = self.workspace.clone();
            if self.ensure_file_worker() {
                if let Some(worker) = self.file_worker.as_ref() {
                    let _ = worker.request(FileRequest::List(workspace));
                }
                self.status = "loading file tree".to_owned();
            } else {
                match self.tree_entries(&workspace) {
                    Ok(entries) => {
                        self.tree = entries
                            .into_iter()
                            .filter(|entry| self.tree_entry_visible(&entry.name, entry.is_dir))
                            .map(|entry| TreeEntry {
                                path: workspace.join(entry.name),
                                depth: 0,
                                is_dir: entry.is_dir,
                                expanded: false,
                            })
                            .collect();
                    }
                    Err(error) => self.status = format!("E: tree: {error}"),
                }
            }
        }
        self.tree_visible = true;
        self.tree_focus = true;
        if self.tree_selected.is_none() {
            self.tree_selected = (!self.tree.is_empty()).then_some(0);
        }
        self.dirty = true;
    }

    pub(crate) fn tree_move(&mut self, delta: isize) {
        if self.tree.is_empty() {
            return;
        }
        let current = self.tree_selected.unwrap_or(0);
        self.tree_selected = Some(
            current
                .saturating_add_signed(delta)
                .min(self.tree.len().saturating_sub(1)),
        );
        self.dirty = true;
    }

    pub(crate) fn tree_toggle(&mut self) {
        let Some(index) = self.tree_selected else {
            return;
        };
        let Some(entry) = self.tree.get(index).cloned() else {
            return;
        };
        if !entry.is_dir {
            match self.open_path(&entry.path) {
                Ok(()) => {
                    self.tree_focus = false;
                    self.tree_visible = false;
                }
                Err(error) => self.status = format!("E: {error}"),
            }
            self.dirty = true;
            return;
        }
        if entry.expanded {
            self.tree[index].expanded = false;
            while self
                .tree
                .get(index + 1)
                .is_some_and(|child| child.depth > entry.depth)
            {
                self.tree.remove(index + 1);
            }
        } else if self.file_worker.is_some() || self.ensure_file_worker() {
            self.tree[index].expanded = true;
            if let Some(worker) = self.file_worker.as_ref() {
                let _ = worker.request(FileRequest::List(entry.path));
            }
            self.status = "loading directory".to_owned();
        } else {
            match self.tree_entries(&entry.path) {
                Ok(children) => {
                    self.tree[index].expanded = true;
                    let children: Vec<TreeEntry> = children
                        .into_iter()
                        .filter(|child| self.tree_entry_visible(&child.name, child.is_dir))
                        .map(|child| TreeEntry {
                            path: entry.path.join(child.name),
                            depth: entry.depth + 1,
                            is_dir: child.is_dir,
                            expanded: false,
                        })
                        .collect();
                    self.tree.splice(index + 1..index + 1, children);
                }
                Err(error) => self.status = format!("E: tree: {error}"),
            }
        }
        self.dirty = true;
    }

    pub(crate) fn tree_collapse(&mut self) {
        let Some(index) = self.tree_selected else {
            return;
        };
        if self.tree.get(index).is_some_and(|entry| entry.expanded) {
            self.tree_toggle();
        } else if self.tree.get(index).is_some_and(|entry| entry.depth > 0) {
            while self.tree_selected.unwrap_or(0) > 0
                && self.tree[self.tree_selected.unwrap_or(0)].depth > 0
            {
                self.tree_move(-1);
            }
            self.tree_selected = self.tree_selected.map(|index| index.saturating_sub(1));
            self.dirty = true;
        }
    }

    fn collect_files(
        &mut self,
        directory: &Path,
        prefix: &str,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        if out.len() >= 5000 {
            return Ok(());
        }
        let entries = self.tree_entries(directory)?;
        for entry in entries {
            if !self.tree_entry_visible(&entry.name, entry.is_dir) {
                continue;
            }
            let relative = if prefix.is_empty() {
                entry.name.clone()
            } else {
                format!("{prefix}/{}", entry.name)
            };
            if entry.is_dir {
                self.collect_files(&directory.join(&entry.name), &relative, out)?;
                if out.len() >= 5000 {
                    break;
                }
            } else {
                out.push(relative);
            }
        }
        Ok(())
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
        self.dirty = true;
    }

    pub(crate) fn start_fuzzy(&mut self) {
        self.cancel_completion();
        self.clear_lsp_requests();
        self.hover = None;
        self.diagnostic_popup = None;
        self.code_actions = None;
        self.document_symbols = None;
        let workspace = self.workspace.clone();
        if self.ensure_file_worker() {
            if let Some(worker) = self.file_worker.as_ref() {
                let _ = worker.request(FileRequest::Files {
                    hidden: self.show_hidden,
                });
            }
            self.fuzzy = Some(FuzzyState {
                query: String::new(),
                entries: Vec::new(),
                filtered: Vec::new(),
                selected: 0,
            });
            self.status = "loading files".to_owned();
            self.dirty = true;
            return;
        }
        let mut entries = Vec::new();
        if let Err(error) = self.collect_files(&workspace, "", &mut entries) {
            self.status = format!("E: files: {error}");
            self.dirty = true;
            return;
        }
        entries.sort();
        let filtered = (0..entries.len()).collect();
        self.fuzzy = Some(FuzzyState {
            query: String::new(),
            entries,
            filtered,
            selected: 0,
        });
        self.dirty = true;
    }

    pub(crate) fn fuzzy_refresh(&mut self) {
        let Some(fuzzy) = self.fuzzy.as_mut() else {
            return;
        };
        let query = fuzzy.query.to_lowercase();
        fuzzy.filtered = fuzzy
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.to_lowercase().contains(&query))
            .map(|(index, _)| index)
            .collect();
        fuzzy.selected = fuzzy.selected.min(fuzzy.filtered.len().saturating_sub(1));
    }

    pub(crate) fn fuzzy_move(&mut self, delta: isize) {
        let Some(fuzzy) = self.fuzzy.as_mut() else {
            return;
        };
        fuzzy.selected = fuzzy
            .selected
            .saturating_add_signed(delta)
            .min(fuzzy.filtered.len().saturating_sub(1));
        self.dirty = true;
    }

    pub(crate) fn fuzzy_open(&mut self) {
        let Some(fuzzy) = self.fuzzy.take() else {
            return;
        };
        let Some(index) = fuzzy.filtered.get(fuzzy.selected) else {
            self.dirty = true;
            return;
        };
        let path = self.workspace.join(&fuzzy.entries[*index]);
        match self.open_path(&path) {
            Ok(()) => {}
            Err(error) => self.status = format!("E: {error}"),
        }
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

    pub(crate) fn refresh_tree(&mut self, select: Option<PathBuf>) {
        self.tree_pending_select = select;
        self.tree.clear();
        self.tree_selected = None;
        self.tree_visible = true;
        self.tree_focus = true;
        let workspace = self.workspace.clone();
        if self.ensure_file_worker() {
            if let Some(worker) = self.file_worker.as_ref() {
                let _ = worker.request(FileRequest::List(workspace));
            }
        } else {
            match self.tree_entries(&workspace) {
                Ok(entries) => {
                    self.tree = entries
                        .into_iter()
                        .map(|entry| TreeEntry {
                            path: workspace.join(entry.name),
                            depth: 0,
                            is_dir: entry.is_dir,
                            expanded: false,
                        })
                        .collect();
                    self.apply_pending_tree_selection();
                }
                Err(error) => self.status = format!("E: tree: {error}"),
            }
        }
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

    pub(crate) fn begin_tree_create(&mut self, kind: CreateKind) {
        if self.tree_busy() {
            return;
        }
        let directory = self.tree_target_directory();
        self.tree_focus = false;
        self.tree_visible = true;
        self.input_mode = InputMode::Create {
            directory,
            name: String::new(),
            kind,
        };
        self.dirty = true;
    }

    pub(crate) fn begin_tree_rename(&mut self) {
        if self.tree_busy() {
            return;
        }
        let Some(index) = self.tree_selected else {
            return;
        };
        let Some(entry) = self.tree.get(index) else {
            return;
        };
        let Some(name) = entry.path.file_name().and_then(|name| name.to_str()) else {
            self.status = "E: invalid entry name".to_owned();
            self.dirty = true;
            return;
        };
        self.tree_focus = false;
        self.input_mode = InputMode::RenameEntry {
            path: entry.path.clone(),
            name: name.to_owned(),
        };
        self.dirty = true;
    }

    pub(crate) fn begin_tree_delete(&mut self) {
        if self.tree_busy() {
            return;
        }
        let Some(index) = self.tree_selected else {
            return;
        };
        let Some(entry) = self.tree.get(index) else {
            return;
        };
        if self.path_is_open(&entry.path) {
            self.status = "E: close the open buffer before deleting".to_owned();
            self.dirty = true;
            return;
        }
        self.tree_focus = false;
        self.input_mode = InputMode::ConfirmDelete {
            path: entry.path.clone(),
        };
        self.dirty = true;
    }

    pub(crate) fn begin_tree_transfer(&mut self, copy: bool) {
        if self.tree_busy() {
            return;
        }
        let Some(index) = self.tree_selected else {
            return;
        };
        let Some(entry) = self.tree.get(index) else {
            return;
        };
        if copy && entry.is_dir {
            self.status = "E: copy supports files only".to_owned();
            self.dirty = true;
            return;
        }
        self.tree_focus = false;
        self.input_mode = InputMode::TransferEntry {
            path: entry.path.clone(),
            copy,
            directory: String::new(),
        };
        self.dirty = true;
    }

    pub(crate) fn finish_tree_transfer(&mut self, path: PathBuf, copy: bool, directory: String) {
        if self.tree_busy() {
            return;
        }
        self.tree_focus = true;
        let directory = directory.trim();
        let target_directory = if directory.is_empty() {
            self.workspace.clone()
        } else {
            match self.absolute_path(Path::new(directory)) {
                Ok(path) if path.starts_with(&self.workspace) => path,
                _ => {
                    self.status = "E: destination is outside the workspace".to_owned();
                    self.dirty = true;
                    return;
                }
            }
        };
        if self.tree_entries(&target_directory).is_err() {
            self.status = "E: destination directory does not exist".to_owned();
            self.dirty = true;
            return;
        }
        let Some(name) = path.file_name() else {
            self.status = "E: invalid entry name".to_owned();
            self.dirty = true;
            return;
        };
        let target = target_directory.join(name);
        if target == path {
            self.status = "E: source and destination are the same".to_owned();
            self.dirty = true;
            return;
        }
        match self.tree_entry_exists(&target_directory, &name.to_string_lossy()) {
            Ok(true) => {
                self.status = format!("E: already exists: {}", name.to_string_lossy());
                self.dirty = true;
                return;
            }
            Ok(false) => {}
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                return;
            }
        }
        if let Some(worker) = self.file_worker.as_ref() {
            let label = if copy { "copying" } else { "moving" };
            match worker.request(FileRequest::Transfer {
                from: path.clone(),
                to: target.clone(),
                copy,
            }) {
                Ok(()) => {
                    self.busy = Some(label.to_owned());
                    self.status = format!("{label} {}…", name.to_string_lossy());
                    self.dirty = true;
                    return;
                }
                Err(error) => {
                    self.status = format!("E: {error}");
                    self.dirty = true;
                    return;
                }
            }
        }
        let result = if copy {
            self.copy_path(&path, &target)
        } else {
            self.rename_path(&path, &target)
        };
        match result {
            Ok(()) => {
                if !copy {
                    self.remap_open_path(&path, &target);
                }
                self.status = format!(
                    "{} {} to {}",
                    if copy { "copied" } else { "moved" },
                    name.to_string_lossy(),
                    target_directory
                        .strip_prefix(&self.workspace)
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|_| ".".to_owned())
                );
                self.refresh_tree(Some(target));
            }
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
            }
        }
    }

    pub(crate) fn finish_tree_create(
        &mut self,
        directory: PathBuf,
        name: String,
        kind: CreateKind,
    ) {
        if self.tree_busy() {
            return;
        }
        let name = name.trim().to_owned();
        if !Self::valid_tree_name(&name) {
            self.status = "E: invalid entry name".to_owned();
            self.dirty = true;
            return;
        }
        match self.tree_entry_exists(&directory, &name) {
            Ok(true) => {
                self.status = format!("E: already exists: {name}");
                self.dirty = true;
                return;
            }
            Ok(false) => {}
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                return;
            }
        }
        let target = directory.join(&name);
        let result = match kind {
            CreateKind::File => self.write_file(&target, ""),
            CreateKind::Directory => self.create_dir(&target),
        };
        match result {
            Ok(()) => {
                if matches!(kind, CreateKind::File)
                    && let Err(error) = self.open_path(&target)
                {
                    self.status = format!("E: {error}");
                    self.dirty = true;
                    return;
                }
                self.refresh_tree(Some(target));
            }
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
            }
        }
    }

    pub(crate) fn finish_tree_rename(&mut self, path: PathBuf, name: String) {
        self.tree_focus = true;
        if self.tree_busy() {
            return;
        }
        let name = name.trim().to_owned();
        if !Self::valid_tree_name(&name) {
            self.status = "E: invalid entry name".to_owned();
            self.dirty = true;
            return;
        }
        let Some(parent) = path.parent() else {
            self.status = "E: invalid entry path".to_owned();
            self.dirty = true;
            return;
        };
        let target = parent.join(&name);
        if target == path {
            self.refresh_tree(Some(path));
            return;
        }
        match self.tree_entry_exists(parent, &name) {
            Ok(true) => {
                self.status = format!("E: already exists: {name}");
                self.dirty = true;
                return;
            }
            Ok(false) => {}
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                return;
            }
        }
        match self.rename_path(&path, &target) {
            Ok(()) => {
                self.remap_open_path(&path, &target);
                self.refresh_tree(Some(target));
            }
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
            }
        }
    }

    pub(crate) fn finish_tree_delete(&mut self, path: PathBuf) {
        self.tree_focus = true;
        if self.tree_busy() {
            return;
        }
        if self.path_is_open(&path) {
            self.status = "E: close the open buffer before deleting".to_owned();
            self.dirty = true;
            return;
        }
        if let Some(worker) = self.file_worker.as_ref() {
            match worker.request(FileRequest::RemoveAll(path.clone())) {
                Ok(()) => {
                    self.busy = Some("deleting".to_owned());
                    self.status = format!(
                        "deleting {}…",
                        path.file_name()
                            .map(|name| name.to_string_lossy())
                            .unwrap_or_default()
                    );
                    self.dirty = true;
                    return;
                }
                Err(error) => {
                    self.status = format!("E: {error}");
                    self.dirty = true;
                    return;
                }
            }
        }
        match self.remove_path(&path) {
            Ok(()) => self.refresh_tree(None),
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
            }
        }
    }

    fn path_is_open(&self, path: &Path) -> bool {
        self.path
            .as_ref()
            .is_some_and(|open| open == path || open.starts_with(path))
            || self.buffers.iter().any(|buffer| {
                buffer
                    .path
                    .as_ref()
                    .is_some_and(|open| open == path || open.starts_with(path))
            })
            || self.split.as_ref().is_some_and(|pane| {
                pane.buffer
                    .path
                    .as_ref()
                    .is_some_and(|open| open == path || open.starts_with(path))
            })
    }

    fn remap_open_path(&mut self, from: &Path, to: &Path) {
        let remap = |path: &Path| {
            if path == from {
                to.to_path_buf()
            } else if let Ok(rest) = path.strip_prefix(from) {
                to.join(rest)
            } else {
                path.to_path_buf()
            }
        };
        if let Some(path) = self.path.as_mut() {
            *path = remap(path);
        }
        for buffer in &mut self.buffers {
            if let Some(path) = buffer.path.as_mut() {
                *path = remap(path);
            }
        }
        if let Some(pane) = self.split.as_mut()
            && let Some(path) = pane.buffer.path.as_mut()
        {
            *path = remap(path);
        }
        self.sync_watch_paths();
    }
}

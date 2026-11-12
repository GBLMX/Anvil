//! Tree create, rename, transfer, and delete operations.

use super::*;

impl Model {
    pub(crate) fn begin_tree_create(&mut self, kind: CreateKind) {
        if self.tree_busy() {
            return;
        }
        let directory = self.tree_target_directory();
        self.ui.focus.tree = false;
        self.tree.visible = true;
        self.ui.input_mode = InputMode::Create {
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
        let Some(index) = self.tree.selected else {
            return;
        };
        let Some(entry) = self.tree.entries.get(index) else {
            return;
        };
        let Some(name) = entry.path.file_name().and_then(|name| name.to_str()) else {
            self.status = "E: invalid entry name".to_owned();
            self.dirty = true;
            return;
        };
        self.ui.focus.tree = false;
        self.ui.input_mode = InputMode::RenameEntry {
            path: entry.path.clone(),
            name: name.to_owned(),
        };
        self.dirty = true;
    }

    pub(crate) fn begin_tree_delete(&mut self) {
        if self.tree_busy() {
            return;
        }
        let Some(index) = self.tree.selected else {
            return;
        };
        let Some(entry) = self.tree.entries.get(index) else {
            return;
        };
        if self.path_is_open(&entry.path) {
            self.status = "E: close the open buffer before deleting".to_owned();
            self.dirty = true;
            return;
        }
        self.ui.focus.tree = false;
        self.ui.input_mode = InputMode::ConfirmDelete {
            path: entry.path.clone(),
        };
        self.dirty = true;
    }

    pub(crate) fn begin_tree_transfer(&mut self, copy: bool) {
        if self.tree_busy() {
            return;
        }
        let Some(index) = self.tree.selected else {
            return;
        };
        let Some(entry) = self.tree.entries.get(index) else {
            return;
        };
        if copy && entry.is_dir {
            self.status = "E: copy supports files only".to_owned();
            self.dirty = true;
            return;
        }
        self.ui.focus.tree = false;
        self.ui.input_mode = InputMode::TransferEntry {
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
        self.ui.focus.tree = true;
        let directory = directory.trim();
        let target_directory = if directory.is_empty() {
            self.workspace.path.clone()
        } else {
            match self.absolute_path(Path::new(directory)) {
                Ok(path)
                    if path.starts_with(&self.workspace.path)
                        || self.secondary_root(&path).is_some() =>
                {
                    path
                }
                _ => {
                    self.status = "E: destination is outside the workspace".to_owned();
                    self.dirty = true;
                    return;
                }
            }
        };
        let source_root = self
            .workspace
            .roots
            .owning_root(&path)
            .map(|root| root.path.clone())
            .unwrap_or_else(|| self.workspace.path.clone());
        let target_root = self
            .workspace
            .roots
            .owning_root(&target_directory)
            .map(|root| root.path.clone())
            .unwrap_or_else(|| self.workspace.path.clone());
        if source_root != target_root {
            self.status = "E: cannot transfer across workspace roots".to_owned();
            self.dirty = true;
            return;
        }
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
        if let Some(worker) = self.file_worker_for_path(&path) {
            let label = if copy { "copying" } else { "moving" };
            match worker.request(FileRequest::Transfer {
                from: path.clone(),
                to: target.clone(),
                copy,
            }) {
                Ok(()) => {
                    self.busy = Some(label.to_owned());
                    self.busy_progress = Some(self.ui.progress.start(
                        label,
                        name.to_string_lossy().into_owned(),
                        false,
                    ));
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
                        .strip_prefix(&self.workspace.path)
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|_| ".".to_owned())
                );
                self.refresh_tree(Some(target.clone()));
                if let Some(root) = self.secondary_root(&target) {
                    self.refresh_root_tree(&root);
                }
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
        self.ui.focus.tree = true;
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
                self.refresh_tree(Some(target.clone()));
                if let Some(root) = self.secondary_root(&target) {
                    self.refresh_root_tree(&root);
                }
            }
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
            }
        }
    }

    pub(crate) fn finish_tree_delete(&mut self, path: PathBuf) {
        self.ui.focus.tree = true;
        if self.tree_busy() {
            return;
        }
        if self.path_is_open(&path) {
            self.status = "E: close the open buffer before deleting".to_owned();
            self.dirty = true;
            return;
        }
        if let Some(worker) = self.file_worker_for_path(&path) {
            match worker.request(FileRequest::RemoveAll(path.clone())) {
                Ok(()) => {
                    self.busy = Some("deleting".to_owned());
                    self.busy_progress = Some(self.ui.progress.start(
                        "deleting",
                        path.display().to_string(),
                        false,
                    ));
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
            Ok(()) => {
                self.refresh_tree(None);
                if let Some(root) = self.secondary_root(&path) {
                    self.refresh_root_tree(&root);
                }
            }
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

    pub(crate) fn remap_open_path(&mut self, from: &Path, to: &Path) {
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

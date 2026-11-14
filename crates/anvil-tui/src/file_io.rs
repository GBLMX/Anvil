//! Workspace file reading, writing, opening, and saving.

use super::*;
use std::io;
use std::time::Instant;

impl Model {
    pub fn open_path(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let timing = std::env::var_os("ANVIL_OPEN_TIMING").is_some();
        let start = Instant::now();
        let absolute = self.absolute_path(path.as_ref())?;
        let recent_path = absolute.clone();
        if self.session.path.as_ref() == Some(&absolute) {
            return Ok(());
        }
        if self.session.path.is_none() && self.file.modified {
            self.status.text =
                "E: save or discard the unnamed buffer before opening another file".to_owned();
            self.file.dirty = true;
            return Err("unnamed buffer is modified".to_owned());
        }
        let existing = self
            .buffers
            .entries
            .iter()
            .position(|buffer| buffer.path.as_ref() == Some(&absolute));
        if existing.is_none() {
            self.ensure_buffer_capacity()?;
        }
        if !self.auto_save_before_focus_change() {
            let error = self
                .status
                .text
                .strip_prefix("E: ")
                .unwrap_or("auto-save failed")
                .to_owned();
            return Err(error);
        }
        if let Some(index) = existing {
            if self.session.path.is_some() {
                self.sync_active();
            }
            self.load_buffer(index);
            self.record_recent_file(&absolute);
            return Ok(());
        }
        if let Some(mut buffer) = self.buffers.background.remove(&absolute) {
            if self.session.path.is_some() {
                self.ensure_current_buffer();
            }
            buffer.syntax = Default::default();
            self.buffers.entries.push(buffer);
            let index = self.buffers.entries.len() - 1;
            self.load_buffer(index);
            self.sync_watch_paths();
            self.record_recent_file(&absolute);
            self.file.stdin_buffer = false;
            self.file.dirty = true;
            return Ok(());
        }
        let text = self.read_file(&absolute)?;
        let read_done = start.elapsed();
        if self.session.path.is_some() {
            self.sync_active();
            if self.buffers.entries.is_empty() {
                self.buffers.entries.push(self.snapshot());
                self.buffers.active = 0;
            }
            self.buffers.entries.push(Buffer {
                editor: EditorState::from_text(&text).into(),
                path: Some(absolute),
                visual_anchor: None,
                visual_line: false,
                cursor: 0,
                scroll: 0,
                hscroll: 0,
                modified: false,
                missing: false,
                search_query: String::new(),
                syntax: Default::default(),
            });
            let index = self.buffers.entries.len() - 1;
            self.load_buffer(index);
        } else {
            self.session.editor = EditorState::from_text(&text).into();
            self.session.path = Some(absolute);
            self.session.cursor = 0;
            self.view.scroll = 0;
            self.view.hscroll = 0;
            self.file.modified = false;
            self.file.missing = false;
            self.syntax_cache
                .highlights
                .borrow_mut()
                .switch_path(self.session.path.as_deref(), true);
        }
        if timing {
            eprintln!("open_path read={read_done:?} index={:?}", start.elapsed());
        }
        self.start_page.welcome = false;
        self.file.stdin_buffer = false;
        self.start_page.welcome_back = false;
        self.sync_lsp_active();
        self.sync_watch_paths();
        self.record_recent_file(&recent_path);
        self.file.dirty = true;
        Ok(())
    }

    pub fn open_with_gateway(
        path: impl AsRef<Path>,
        gateway: GatewayClient,
        workspace: PathBuf,
    ) -> Result<Self, String> {
        let mut model = Self::with_gateway(gateway, workspace);
        model.open_path(path)?;
        Ok(model)
    }

    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let text = fs::read_to_string(&path)?;
        let path = path.canonicalize().unwrap_or(path);
        Ok(Self::new(EditorState::from_text(&text), Some(path)))
    }

    pub(crate) fn absolute_path(&self, path: &Path) -> Result<PathBuf, String> {
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace.path.join(path)
        };
        Ok(joined.canonicalize().unwrap_or(joined))
    }

    pub(crate) fn relative_path(&self, path: &Path) -> Result<String, String> {
        let relative = path.strip_prefix(&self.workspace.path).map_err(|_| {
            format!(
                "path is outside workspace {}",
                self.workspace.path.display()
            )
        })?;
        let relative = relative
            .to_str()
            .filter(|relative| !relative.is_empty() && *relative != ".")
            .ok_or_else(|| "invalid file path".to_owned())?;
        Ok(relative.to_owned())
    }

    fn root_relative(&self, path: &Path) -> Result<(PathBuf, String), String> {
        let root_path = if let Some(root) = self.workspace.roots.owning_root(path) {
            root.path.clone()
        } else if path.starts_with(&self.workspace.path) {
            self.workspace.path.clone()
        } else {
            return Err(format!(
                "path is outside workspace roots: {}",
                path.display()
            ));
        };
        let relative = path
            .strip_prefix(&root_path)
            .map_err(|_| format!("path is outside workspace root {}", root_path.display()))?;
        let relative = relative
            .to_str()
            .filter(|relative| !relative.is_empty() && *relative != ".")
            .ok_or_else(|| "invalid file path".to_owned())?;
        Ok((root_path, relative.to_owned()))
    }

    fn gateway_for_root(&mut self, root: &Path) -> Result<&mut GatewayClient, String> {
        if root == self.workspace.path {
            return self
                .workspace
                .gateway
                .as_mut()
                .ok_or_else(|| "gateway unavailable".to_owned());
        }
        self.workspace
            .root_gateways
            .get_mut(root)
            .map(|(_, gateway)| gateway)
            .ok_or_else(|| format!("root gateway unavailable or untrusted: {}", root.display()))
    }

    pub(crate) fn read_file(&mut self, path: &Path) -> Result<String, String> {
        if !self.workspace.require_gateway {
            let data = fs::read(path).map_err(|error| error.to_string())?;
            return String::from_utf8(data)
                .map_err(|_| format!("{}: invalid UTF-8", path.display()));
        }
        let (root, relative) = self.root_relative(path)?;
        let gateway = self.gateway_for_root(&root)?;
        let size = gateway
            .file_size(&relative)
            .map_err(|error| error.to_string())?;
        let window = std::env::var("ANVIL_TUI_READ_WINDOW")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|window| *window > 0)
            .unwrap_or(4);
        let data = gateway
            .read_all_binary_pipelined(&relative, size, MAX_OPEN_BYTES, window)
            .map_err(|error| error.to_string())?;
        String::from_utf8(data).map_err(|_| format!("{relative}: invalid UTF-8"))
    }

    pub(crate) fn write_file(&mut self, path: &Path, text: &str) -> Result<(), String> {
        if !self.workspace.require_gateway {
            return fs::write(path, text).map_err(|error| error.to_string());
        }
        let (root, relative) = self.root_relative(path)?;
        let gateway = self.gateway_for_root(&root)?;
        gateway
            .write_all_binary(&relative, text.as_bytes())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn create_dir(&mut self, path: &Path) -> Result<(), String> {
        if !self.workspace.require_gateway {
            return fs::create_dir(path).map_err(|error| error.to_string());
        }
        let (root, relative) = self.root_relative(path)?;
        let gateway = self.gateway_for_root(&root)?;
        gateway
            .create_dir(&relative)
            .map_err(|error| error.to_string())
    }

    fn mutation_relatives(
        &self,
        from: &Path,
        to: &Path,
    ) -> Result<(PathBuf, String, String), String> {
        let (from_root, from_relative) = self.root_relative(from)?;
        let (to_root, to_relative) = self.root_relative(to)?;
        if from_root != to_root {
            return Err(format!(
                "cannot mutate across workspace roots: {} -> {}",
                from_root.display(),
                to_root.display()
            ));
        }
        Ok((from_root, from_relative, to_relative))
    }

    fn reject_cross_root(&self, from: &Path, to: &Path) -> Result<(), String> {
        let from_root = self
            .workspace
            .roots
            .owning_root(from)
            .map(|root| &root.path);
        let to_root = self.workspace.roots.owning_root(to).map(|root| &root.path);
        if let (Some(from_root), Some(to_root)) = (from_root, to_root)
            && from_root != to_root
        {
            return Err(format!(
                "cannot mutate across workspace roots: {} -> {}",
                from_root.display(),
                to_root.display()
            ));
        }
        Ok(())
    }

    pub(crate) fn rename_path(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        if !self.workspace.require_gateway {
            self.reject_cross_root(from, to)?;
            return fs::rename(from, to).map_err(|error| error.to_string());
        }
        let (root, from_relative, to_relative) = self.mutation_relatives(from, to)?;
        let gateway = self.gateway_for_root(&root)?;
        gateway
            .rename(&from_relative, &to_relative)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn remove_path(&mut self, path: &Path) -> Result<(), String> {
        if !self.workspace.require_gateway {
            let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
            return if metadata.is_dir() {
                fs::remove_dir_all(path).map_err(|error| error.to_string())
            } else {
                fs::remove_file(path).map_err(|error| error.to_string())
            };
        }
        let (root, relative) = self.root_relative(path)?;
        let gateway = self.gateway_for_root(&root)?;
        gateway
            .remove_all(&relative)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn copy_path(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        if !self.workspace.require_gateway {
            self.reject_cross_root(from, to)?;
            fs::copy(from, to)
                .map(|_| ())
                .map_err(|error| error.to_string())
        } else {
            let (root, from_relative, to_relative) = self.mutation_relatives(from, to)?;
            let gateway = self.gateway_for_root(&root)?;
            gateway
                .copy(&from_relative, &to_relative)
                .map_err(|error| error.to_string())
        }
    }

    pub(crate) fn sync_watch_paths(&mut self) {
        if !self.ensure_file_worker() {
            return;
        }
        let mut paths = Vec::new();
        if let Some(path) = &self.session.path {
            paths.push(path.clone());
        }
        for buffer in &self.buffers.entries {
            if let Some(path) = &buffer.path {
                paths.push(path.clone());
            }
        }
        for buffer in self.buffers.background.values() {
            if let Some(path) = &buffer.path {
                paths.push(path.clone());
            }
        }
        if let Some(pane) = &self.buffers.split
            && let Some(path) = &pane.buffer.path
        {
            paths.push(path.clone());
        }
        paths.sort();
        paths.dedup();
        if let Some(worker) = self.workspace.file_worker.as_ref() {
            let _ = worker.request(anvil_workspace::FileRequest::Watch(paths));
        }
    }

    pub(crate) fn handle_external_change(&mut self, path: PathBuf) {
        let modified = if self.session.path.as_ref() == Some(&path) {
            self.file.modified
        } else if let Some(buffer) = self
            .buffers
            .entries
            .iter()
            .find(|buffer| buffer.path.as_ref() == Some(&path))
        {
            buffer.modified
        } else {
            self.buffers
                .background
                .get(&path)
                .is_some_and(|buffer| buffer.modified)
        };
        if modified {
            if matches!(
                &self.ui.input_mode,
                InputMode::ConfirmReload { path: pending } if pending.as_path() == path
            ) {
                return;
            }
            self.ui.input_mode = InputMode::ConfirmReload { path: path.clone() };
            self.status.text = format!("File changed on disk: {} [y/N]", path.display());
            self.file.dirty = true;
        } else if let Err(error) = self.reload_external(&path) {
            self.status.text = format!("E: {error}");
            self.file.dirty = true;
        }
    }
    pub(crate) fn mark_missing(&mut self, path: &Path) {
        let affected = |candidate: &Path| candidate == path || candidate.starts_with(path);
        if self.session.path.as_deref().is_some_and(affected) {
            self.file.missing = true;
        }
        for buffer in &mut self.buffers.entries {
            if buffer.path.as_deref().is_some_and(affected) {
                buffer.missing = true;
            }
        }
        for buffer in self.buffers.background.values_mut() {
            if buffer.path.as_deref().is_some_and(affected) {
                buffer.missing = true;
            }
        }
        if let Some(pane) = self.buffers.split.as_mut()
            && pane.buffer.path.as_deref().is_some_and(affected)
        {
            pane.buffer.missing = true;
        }
        self.file.dirty = true;
    }

    pub(crate) fn reload_external(&mut self, path: &Path) -> Result<(), String> {
        let text = match self.read_file(path) {
            Ok(text) => text,
            Err(error) => {
                self.mark_missing(path);
                return Err(error);
            }
        };
        let cursor = self.session.cursor.min(text.len());
        if self.session.path.as_deref() == Some(path) {
            self.session.editor = EditorState::from_text(&text).into();
            self.session.cursor = cursor;
            self.file.modified = false;
            self.file.missing = false;
            self.syntax_cache.highlights.borrow_mut().invalidate();
            self.sync_lsp_active();
        }
        if let Some(buffer) = self
            .buffers
            .entries
            .iter_mut()
            .find(|buffer| buffer.path.as_deref() == Some(path))
        {
            buffer.editor = EditorState::from_text(&text).into();
            buffer.cursor = cursor.min(buffer.editor.text().len_bytes());
            buffer.syntax = Default::default();
            buffer.modified = false;
            buffer.missing = false;
        }
        if let Some(buffer) = self.buffers.background.get_mut(path) {
            buffer.editor = EditorState::from_text(&text).into();
            buffer.cursor = cursor.min(buffer.editor.text().len_bytes());
            buffer.syntax = Default::default();
            buffer.modified = false;
            buffer.missing = false;
        }
        if let Some(pane) = self.buffers.split.as_mut()
            && pane.buffer.path.as_deref() == Some(path)
        {
            pane.buffer.editor = EditorState::from_text(&text).into();
            pane.buffer.syntax = Default::default();
            pane.buffer.cursor = cursor.min(pane.buffer.editor.text().len_bytes());
            pane.buffer.modified = false;
            pane.buffer.missing = false;
        }
        self.sync_active();
        self.sync_watch_paths();
        self.workspace.stale_paths.remove(path);
        self.status.text = format!("reloaded {}", path.display());
        self.file.dirty = true;
        Ok(())
    }

    pub(crate) fn save_to(&mut self, path: PathBuf) -> bool {
        if self.reject_read_only() {
            return false;
        }
        let path = match self.absolute_path(&path) {
            Ok(path) => path,
            Err(error) => {
                if self.file.editor_mode {
                    self.file.exit_code = 1;
                }
                self.status.text = format!("E: {error}");
                self.file.dirty = true;
                return false;
            }
        };
        if self.workspace.stale_paths.contains(&path) {
            self.status.text =
                "E: file changed by project replace; reload before saving".to_owned();
            self.file.dirty = true;
            return false;
        }
        let current_conflict = self
            .session
            .path
            .as_ref()
            .filter(|current| self.workspace.conflict_swaps.contains_key(*current))
            .cloned();
        if let Some(conflict) = &current_conflict {
            if path != *conflict {
                self.status.text =
                    "E: resolve the current swap conflict before saving elsewhere".to_owned();
                self.file.dirty = true;
                return false;
            }
            if !self.file.modified {
                self.status.text =
                    "E: swap conflict; :conflict-recover or :conflict-discard".to_owned();
                self.file.dirty = true;
                return false;
            }
        }
        let target_conflict = self.workspace.conflict_swaps.contains_key(&path);
        if target_conflict && current_conflict.is_none() {
            self.status.text = "E: destination has an unresolved swap conflict".to_owned();
            self.file.dirty = true;
            return false;
        }
        let conflict_path = if target_conflict {
            Some(path.clone())
        } else {
            None
        };
        let text = self
            .session
            .editor
            .text()
            .slice(0..self.session.editor.text().len_bytes());
        let display = path.display().to_string();
        match self.write_file(&path, &text) {
            Ok(()) => {
                if self.file.editor_mode {
                    self.file.exit_code = 0;
                }
                self.session.path = Some(path.clone());
                if let Some(conflict) = conflict_path {
                    self.clear_conflict(&conflict);
                }
                self.file.stdin_buffer = false;
                self.file.modified = false;
                self.file.missing = false;
                self.git.diff_checked = None;
                self.git.diff_due = Some(Instant::now());
                self.sync_lsp_active();
                self.notify_lsp_save(&path, &text);
                self.sync_watch_paths();
                self.status.text = format!("\"{display}\" written");
                self.request_format_after_save();
                self.file.dirty = true;
                true
            }
            Err(error) => {
                if self.file.editor_mode {
                    self.file.exit_code = 1;
                }
                self.status.text = format!("E: {error}");
                self.file.dirty = true;
                false
            }
        }
    }

    /// Saves the active buffer when focus-change auto-save is enabled and safe.
    ///
    /// Returns false only when a save was attempted and failed; callers should
    /// abort the focus change so the user sees the error and the buffer stays
    /// dirty.
    pub(crate) fn auto_save_before_focus_change(&mut self) -> bool {
        if !self.config.settings.editor.auto_save
            || !self.file.modified
            || self.session.path.is_none()
            || self.file.read_only
            || self.file.stdin_buffer
            || self.file.missing
            || self.pager.back.is_some()
            || self.overlays.settings_overlay.is_some()
            || self.overlays.command_palette.is_some()
            || self.overlays.fuzzy.is_some()
            || self.edit.modal_layer.is_some()
            || matches!(
                self.ui.input_mode,
                InputMode::ConfirmClose { .. }
                    | InputMode::ConfirmReload { .. }
                    | InputMode::ConfirmDelete { .. }
            )
        {
            return true;
        }
        if let Some(path) = self.session.path.as_ref()
            && self.workspace.conflict_swaps.contains_key(path)
        {
            return true;
        }
        if self.save() {
            true
        } else {
            if let Some(error) = self.status.text.strip_prefix("E: ") {
                self.status.text = format!("E: auto-save: {error}");
            }
            false
        }
    }

    pub(crate) fn save(&mut self) -> bool {
        if self.reject_read_only() {
            return false;
        }
        let Some(path) = self.session.path.clone() else {
            if self.file.editor_mode {
                self.file.exit_code = 1;
            }
            self.status.text = "E: no file name".to_owned();
            self.file.dirty = true;
            return false;
        };
        self.save_to(path)
    }

    pub(crate) fn save_all(&mut self) -> bool {
        if self.reject_read_only() {
            return false;
        }
        self.sync_active();
        let active_path = self.session.path.clone();
        let mut failed = false;
        if self.file.modified && !self.save() {
            failed = true;
        }
        let mut writes = Vec::new();
        for (index, buffer) in self.buffers.entries.iter().enumerate() {
            if !buffer.modified || buffer.path == active_path {
                continue;
            }
            let Some(path) = &buffer.path else {
                self.status.text = "E: modified buffer has no file name".to_owned();
                failed = true;
                continue;
            };
            let text = buffer
                .editor
                .text()
                .slice(0..buffer.editor.text().len_bytes());
            writes.push((index, path.clone(), text));
        }
        let mut background_writes = Vec::new();
        for (path, buffer) in &self.buffers.background {
            if !buffer.modified || Some(path) == active_path.as_ref() {
                continue;
            }
            let text = buffer
                .editor
                .text()
                .slice(0..buffer.editor.text().len_bytes());
            background_writes.push((path.clone(), text));
        }
        for (index, path, text) in writes {
            match self.write_file(&path, &text) {
                Ok(()) => {
                    if let Some(buffer) = self.buffers.entries.get_mut(index) {
                        buffer.modified = false;
                        buffer.missing = false;
                    }
                    self.clear_conflict(&path);
                }
                Err(error) => {
                    self.status.text = format!("E: {}: {error}", path.display());
                    failed = true;
                }
            }
        }
        for (path, text) in background_writes {
            match self.write_file(&path, &text) {
                Ok(()) => {
                    if let Some(buffer) = self.buffers.background.get_mut(&path) {
                        buffer.modified = false;
                        buffer.missing = false;
                    }
                    self.clear_conflict(&path);
                }
                Err(error) => {
                    self.status.text = format!("E: {}: {error}", path.display());
                    failed = true;
                }
            }
        }
        self.sync_active();
        if !failed {
            self.status.text = "all files written".to_owned();
        }
        self.file.dirty = true;
        !failed
    }

    pub(crate) fn save_as(&mut self, path: &str) -> bool {
        self.save_to(PathBuf::from(path))
    }
}

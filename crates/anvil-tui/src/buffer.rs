//! Buffer snapshots, activation, and quit guards.

use super::*;

impl Model {
    pub(crate) fn snapshot(&self) -> Buffer {
        Buffer {
            editor: self.editor.clone(),
            path: self.path.clone(),
            cursor: self.cursor,
            scroll: self.view.scroll,
            missing: self.missing,
            hscroll: self.view.hscroll,
            visual_anchor: self.visual_anchor,
            visual_line: self.visual_line,
            modified: self.modified,
            search_query: self.search.query.clone(),
            syntax: self.syntax.borrow().active_cache(),
        }
    }

    pub(crate) fn sync_active(&mut self) {
        let snapshot = self.snapshot();
        if let Some(buffer) = self.buffers.entries.get_mut(self.buffers.active) {
            *buffer = snapshot;
        }
    }

    pub(crate) fn switch_to_buffer(&mut self, index: usize) {
        if self.buffers.help_back.is_none() {
            self.sync_active();
        }
        self.load_buffer(index);
    }

    pub(crate) fn load_buffer(&mut self, index: usize) {
        self.clear_lsp_requests();
        self.overlays.lsp_results.hover = None;
        self.ui.diagnostic_peek = None;
        self.overlays.lsp_results.code_actions = None;
        self.overlays.lsp_results.document_symbols = None;
        let Some(buffer) = self.buffers.entries.get(index).cloned() else {
            return;
        };
        let syntax = buffer.syntax.clone();
        self.missing = buffer.missing;
        self.visual_anchor = buffer.visual_anchor;
        self.visual_line = buffer.visual_line;
        self.editor = buffer.editor;
        self.path = buffer.path;
        self.cursor = buffer.cursor;
        self.view.scroll = buffer.scroll;
        self.view.hscroll = buffer.hscroll;
        self.modified = buffer.modified;
        self.search.query = buffer.search_query;
        self.search.state.reset_for_buffer();
        self.welcome = false;
        self.welcome_back = false;
        self.syntax
            .borrow_mut()
            .switch_path_with(self.path.as_deref(), false, Some(syntax));
        self.buffers.active = index;
        self.ui.input_mode = InputMode::Normal;
        self.pending = None;
        self.sync_lsp_active();
        self.dirty = true;
    }

    fn ensure_current_buffer(&mut self) -> usize {
        self.sync_active();
        if self.buffers.entries.is_empty() {
            self.buffers.entries.push(self.snapshot());
            self.buffers.active = 0;
        }
        self.buffers.active
    }

    pub(crate) fn new_buffer(&mut self) {
        if self.buffers.help_back.is_some() {
            let _ = self.close_help();
        }
        if !self.auto_save_before_focus_change() {
            return;
        }
        self.ensure_current_buffer();
        self.buffers.entries.push(Buffer {
            editor: EditorState::from_text("").into(),
            path: None,
            cursor: 0,
            missing: false,
            scroll: 0,
            visual_anchor: None,
            visual_line: false,
            hscroll: 0,
            modified: false,
            search_query: String::new(),
            syntax: Default::default(),
        });
        let index = self.buffers.entries.len() - 1;
        self.load_buffer(index);
        self.status = "new buffer".to_owned();
        self.sync_watch_paths();
        self.dirty = true;
    }

    pub(crate) fn buffer_name(&self, index: usize) -> String {
        if index == self.buffers.active && self.stdin_buffer {
            return "[stdin]".to_owned();
        }
        self.buffers
            .entries
            .get(index)
            .and_then(|buffer| buffer.path.as_ref())
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "[no file]".to_owned())
    }

    pub(crate) fn save_buffer(&mut self, index: usize) -> bool {
        self.sync_active();
        if index >= self.buffers.entries.len() {
            self.status = "E: invalid buffer index".to_owned();
            self.dirty = true;
            return false;
        }
        if index == self.buffers.active {
            return self.save();
        }
        let Some(path) = self.buffers.entries[index].path.clone() else {
            self.status = "E: buffer has no file name".to_owned();
            self.dirty = true;
            return false;
        };
        let text = self.buffers.entries[index]
            .editor
            .text()
            .slice(0..self.buffers.entries[index].editor.text().len_bytes());
        let display = path.display().to_string();
        match self.write_file(&path, &text) {
            Ok(()) => {
                if let Some(buffer) = self.buffers.entries.get_mut(index) {
                    buffer.modified = false;
                    buffer.missing = false;
                }
                self.status = format!("\"{display}\" written");
                self.dirty = true;
                true
            }
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                false
            }
        }
    }

    pub(crate) fn close_buffer(&mut self, index: usize, force: bool) {
        self.ensure_current_buffer();
        if index >= self.buffers.entries.len() {
            self.status = "E: invalid buffer index".to_owned();
            self.dirty = true;
            return;
        }
        if self.buffers.entries[index].modified && !force {
            if index == self.buffers.active && self.settings.editor.auto_save {
                if !self.auto_save_before_focus_change() {
                    return;
                }
                self.sync_active();
            }
            if self.buffers.entries[index].modified {
                self.ui.input_mode = InputMode::ConfirmClose { index };
                self.status = format!("Save {} before closing? [Y/n/c]", self.buffer_name(index));
                self.dirty = true;
                return;
            }
        }
        self.finish_close_buffer(index);
        self.sync_watch_paths();
    }
    fn finish_close_buffer(&mut self, index: usize) {
        let was_active = index == self.buffers.active;
        let target_path = self.buffers.entries[index].path.clone();
        let split_same = self.buffers.split.as_ref().is_some_and(|pane| {
            match (&target_path, &pane.buffer.path) {
                (Some(target), Some(split)) => target == split,
                (None, None) => was_active,
                _ => false,
            }
        });
        if split_same
            && self
                .buffers
                .split
                .as_ref()
                .is_some_and(|pane| pane.buffer.modified)
        {
            self.status = "E: split pane has unsaved changes".to_owned();
            self.dirty = true;
            return;
        }
        if split_same {
            self.buffers.split = None;
            self.ui.focus.split = false;
        }
        self.buffers.entries.remove(index);
        if self.buffers.entries.is_empty() {
            self.reset_empty_buffer();
            self.status = "buffer closed".to_owned();
            self.dirty = true;
            return;
        }
        if was_active {
            let next = index.min(self.buffers.entries.len() - 1);
            self.load_buffer(next);
        } else if index < self.buffers.active {
            self.buffers.active -= 1;
        }
        self.status = format!(
            "buffer {}/{} closed",
            index + 1,
            self.buffers.entries.len() + 1
        );
        self.dirty = true;
    }

    fn reset_empty_buffer(&mut self) {
        self.editor = EditorState::from_text("").into();
        self.path = None;
        self.cursor = 0;
        self.view.scroll = 0;
        self.view.hscroll = 0;
        self.modified = false;
        self.missing = false;
        self.visual_anchor = None;
        self.visual_line = false;
        self.search.query.clear();
        self.search.state.reset_for_buffer();
        self.syntax.borrow_mut().switch_path(None, false);
        self.welcome = false;
        self.welcome_back = false;
        self.ui.input_mode = InputMode::Normal;
        self.pending = None;
        self.buffers.split = None;
        self.ui.focus.split = false;
        self.buffers.active = 0;
        self.sync_lsp_active();
        self.dirty = true;
    }

    pub(crate) fn open_file(&mut self, path: &str) {
        if path.is_empty() {
            self.status = "E: file name required".to_owned();
            self.dirty = true;
            return;
        }
        if self.buffers.help_back.is_some() {
            let _ = self.close_help();
        }
        let path = match self.absolute_path(Path::new(path)) {
            Ok(path) => path,
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                return;
            }
        };
        if self.path.as_ref() == Some(&path) {
            self.status = format!("Already editing {}", path.display());
            self.dirty = true;
            return;
        }
        if self.path.is_none() && self.modified {
            self.status =
                "E: save or discard the unnamed buffer before opening another file".to_owned();
            self.dirty = true;
            return;
        }
        if !self.auto_save_before_focus_change() {
            return;
        }
        if let Some(index) = self
            .buffers
            .entries
            .iter()
            .position(|buffer| buffer.path.as_ref() == Some(&path))
        {
            self.sync_active();
            self.load_buffer(index);
            self.status = format!(
                "buffer {}/{}: {}",
                index + 1,
                self.buffers.entries.len(),
                path.display()
            );
            self.dirty = true;
            return;
        }
        let text = match self.read_file(&path) {
            Ok(text) => text,
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                return;
            }
        };
        self.sync_active();
        if self.buffers.entries.is_empty() {
            self.buffers.entries.push(self.snapshot());
        }
        self.buffers.entries.push(Buffer {
            editor: EditorState::from_text(&text).into(),
            path: Some(path.clone()),
            cursor: 0,
            missing: false,
            visual_anchor: None,
            visual_line: false,
            scroll: 0,
            hscroll: 0,
            modified: false,
            search_query: String::new(),
            syntax: Default::default(),
        });
        let index = self.buffers.entries.len() - 1;
        self.load_buffer(index);
        self.status = format!(
            "buffer {}/{}: {}",
            index + 1,
            self.buffers.entries.len(),
            path.display()
        );
        self.dirty = true;
    }

    pub(crate) fn switch_buffer(&mut self, delta: isize) {
        if self.buffers.help_back.is_some() {
            let _ = self.close_help();
        }
        if self.buffers.entries.is_empty() {
            self.status = "E: only one buffer".to_owned();
            self.dirty = true;
            return;
        }
        let len = self.buffers.entries.len();
        let target = (self.buffers.active as isize + delta).rem_euclid(len as isize) as usize;
        if target == self.buffers.active {
            self.status = "E: only one buffer".to_owned();
            self.dirty = true;
            return;
        }
        if !self.auto_save_before_focus_change() {
            return;
        }
        self.sync_active();
        self.load_buffer(target);
        let name = self
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "[no file]".to_owned());
        self.status = format!("buffer {}/{}: {name}", target + 1, len);
    }

    pub(crate) fn any_modified(&self) -> bool {
        self.modified
            || self
                .buffers
                .split
                .as_ref()
                .is_some_and(|pane| pane.buffer.modified)
            || self
                .buffers
                .entries
                .iter()
                .enumerate()
                .any(|(index, buffer)| index != self.buffers.active && buffer.modified)
    }

    pub(crate) fn request_quit(&mut self) {
        if self.close_help() {
            return;
        }
        if !self.auto_save_before_focus_change() {
            return;
        }
        if self.any_modified() {
            self.status = "No write since last change (:q! to override)".to_owned();
            self.dirty = true;
        } else {
            self.quit = true;
        }
    }

    pub(crate) fn force_quit(&mut self) {
        if !self.close_help() {
            self.quit = true;
        }
    }

    pub(crate) fn set_editor_mode(&mut self, enabled: bool) {
        self.editor_mode = enabled;
    }

    pub(crate) fn exit_code(&self) -> i32 {
        self.exit_code
    }

    pub(crate) fn quit_with_code(&mut self, code: i32) {
        self.exit_code = code;
        self.quit = true;
    }

    pub(crate) fn set_stdin_buffer(&mut self, read_only: bool) {
        self.stdin_buffer = true;
        self.read_only = read_only;
        self.welcome = false;
    }

    pub(crate) fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }
}

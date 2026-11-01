//! Buffer snapshots, activation, and quit guards.

use super::*;

impl Model {
    pub(crate) fn snapshot(&self) -> Buffer {
        Buffer {
            editor: self.editor.clone(),
            path: self.path.clone(),
            cursor: self.cursor,
            scroll: self.scroll,
            hscroll: self.hscroll,
            modified: self.modified,
            search_query: self.search_query.clone(),
        }
    }

    pub(crate) fn sync_active(&mut self) {
        let snapshot = self.snapshot();
        if let Some(buffer) = self.buffers.get_mut(self.active) {
            *buffer = snapshot;
        }
    }

    pub(crate) fn load_buffer(&mut self, index: usize) {
        self.cancel_completion();
        self.clear_lsp_requests();
        self.hover = None;
        self.diagnostic_popup = None;
        self.code_actions = None;
        self.document_symbols = None;
        let Some(buffer) = self.buffers.get(index).cloned() else {
            return;
        };
        self.editor = buffer.editor;
        self.path = buffer.path;
        self.cursor = buffer.cursor;
        self.scroll = buffer.scroll;
        self.hscroll = buffer.hscroll;
        self.modified = buffer.modified;
        self.search_query = buffer.search_query;
        self.welcome = false;
        self.welcome_back = false;
        self.syntax.borrow_mut().invalidate();
        self.active = index;
        self.input_mode = InputMode::Normal;
        self.pending = None;
        self.sync_lsp_active();
        self.dirty = true;
    }

    fn ensure_current_buffer(&mut self) -> usize {
        self.sync_active();
        if self.buffers.is_empty() {
            self.buffers.push(self.snapshot());
            self.active = 0;
        }
        self.active
    }

    pub(crate) fn new_buffer(&mut self) {
        if self.help_back.is_some() {
            let _ = self.close_help();
        }
        self.ensure_current_buffer();
        self.buffers.push(Buffer {
            editor: EditorState::from_text(""),
            path: None,
            cursor: 0,
            scroll: 0,
            hscroll: 0,
            modified: false,
            search_query: String::new(),
        });
        let index = self.buffers.len() - 1;
        self.load_buffer(index);
        self.status = "new buffer".to_owned();
        self.dirty = true;
    }

    pub(crate) fn buffer_name(&self, index: usize) -> String {
        self.buffers
            .get(index)
            .and_then(|buffer| buffer.path.as_ref())
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "[no file]".to_owned())
    }

    pub(crate) fn save_buffer(&mut self, index: usize) -> bool {
        self.sync_active();
        if index >= self.buffers.len() {
            self.status = "E: invalid buffer index".to_owned();
            self.dirty = true;
            return false;
        }
        if index == self.active {
            return self.save();
        }
        let Some(path) = self.buffers[index].path.clone() else {
            self.status = "E: buffer has no file name".to_owned();
            self.dirty = true;
            return false;
        };
        let text = self.buffers[index]
            .editor
            .text()
            .slice(0..self.buffers[index].editor.text().len_bytes());
        let display = path.display().to_string();
        match self.write_file(&path, &text) {
            Ok(()) => {
                if let Some(buffer) = self.buffers.get_mut(index) {
                    buffer.modified = false;
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
        if index >= self.buffers.len() {
            self.status = "E: invalid buffer index".to_owned();
            self.dirty = true;
            return;
        }
        if self.buffers[index].modified && !force {
            self.input_mode = InputMode::ConfirmClose { index };
            self.status = format!("Save {} before closing? [Y/n/c]", self.buffer_name(index));
            self.dirty = true;
            return;
        }
        self.finish_close_buffer(index);
    }
    fn finish_close_buffer(&mut self, index: usize) {
        let was_active = index == self.active;
        let target_path = self.buffers[index].path.clone();
        let split_same =
            self.split
                .as_ref()
                .is_some_and(|pane| match (&target_path, &pane.buffer.path) {
                    (Some(target), Some(split)) => target == split,
                    (None, None) => was_active,
                    _ => false,
                });
        if split_same && self.split.as_ref().is_some_and(|pane| pane.buffer.modified) {
            self.status = "E: split pane has unsaved changes".to_owned();
            self.dirty = true;
            return;
        }
        if split_same {
            self.split = None;
            self.split_active = false;
        }
        self.buffers.remove(index);
        if self.buffers.is_empty() {
            self.reset_empty_buffer();
            self.status = "buffer closed".to_owned();
            self.dirty = true;
            return;
        }
        if was_active {
            let next = index.min(self.buffers.len() - 1);
            self.load_buffer(next);
        } else if index < self.active {
            self.active -= 1;
        }
        self.status = format!("buffer {}/{} closed", index + 1, self.buffers.len() + 1);
        self.dirty = true;
    }

    fn reset_empty_buffer(&mut self) {
        self.editor = EditorState::from_text("");
        self.path = None;
        self.cursor = 0;
        self.scroll = 0;
        self.hscroll = 0;
        self.modified = false;
        self.search_query.clear();
        self.syntax.borrow_mut().invalidate();
        self.welcome = false;
        self.welcome_back = false;
        self.input_mode = InputMode::Normal;
        self.pending = None;
        self.split = None;
        self.split_active = false;
        self.active = 0;
        self.sync_lsp_active();
        self.dirty = true;
    }

    pub(crate) fn open_file(&mut self, path: &str) {
        if path.is_empty() {
            self.status = "E: file name required".to_owned();
            self.dirty = true;
            return;
        }
        if self.help_back.is_some() {
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
        if let Some(index) = self
            .buffers
            .iter()
            .position(|buffer| buffer.path.as_ref() == Some(&path))
        {
            self.sync_active();
            self.load_buffer(index);
            self.status = format!(
                "buffer {}/{}: {}",
                index + 1,
                self.buffers.len(),
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
        if self.buffers.is_empty() {
            self.buffers.push(self.snapshot());
        }
        self.buffers.push(Buffer {
            editor: EditorState::from_text(&text),
            path: Some(path.clone()),
            cursor: 0,
            scroll: 0,
            hscroll: 0,
            modified: false,
            search_query: String::new(),
        });
        let index = self.buffers.len() - 1;
        self.load_buffer(index);
        self.status = format!(
            "buffer {}/{}: {}",
            index + 1,
            self.buffers.len(),
            path.display()
        );
        self.dirty = true;
    }

    pub(crate) fn switch_buffer(&mut self, delta: isize) {
        if self.help_back.is_some() {
            let _ = self.close_help();
        }
        if self.buffers.is_empty() {
            self.status = "E: only one buffer".to_owned();
            self.dirty = true;
            return;
        }
        self.sync_active();
        let len = self.buffers.len();
        let target = (self.active as isize + delta).rem_euclid(len as isize) as usize;
        if target == self.active {
            self.status = "E: only one buffer".to_owned();
            self.dirty = true;
            return;
        }
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
            || self.split.as_ref().is_some_and(|pane| pane.buffer.modified)
            || self
                .buffers
                .iter()
                .enumerate()
                .any(|(index, buffer)| index != self.active && buffer.modified)
    }

    pub(crate) fn request_quit(&mut self) {
        if self.close_help() {
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
}

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
        self.pending_lsp.clear();
        self.hover = None;
        self.diagnostic_popup = None;
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
        self.syntax.borrow_mut().invalidate();
        self.active = index;
        self.input_mode = InputMode::Normal;
        self.pending = None;
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

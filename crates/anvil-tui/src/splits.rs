//! Minimal two-pane vertical split state.

use super::*;

impl Model {
    pub(crate) fn split_vertical(&mut self) {
        if self.split.is_some() {
            self.status = "split already open".to_owned();
            self.dirty = true;
            return;
        }
        self.split = Some(Pane {
            buffer: self.snapshot(),
        });
        self.ui.focus.split = false;
        self.status = "vertical split".to_owned();
        self.sync_watch_paths();
        self.dirty = true;
    }

    pub(crate) fn focus_editor(&mut self) {
        self.ui.focus.tree = false;
        self.ui.focus.split = false;
        self.dirty = true;
    }

    pub(crate) fn focus_split(&mut self, right: bool) {
        if self.split.is_none() {
            return;
        }
        if self.ui.focus.split && self.modified {
            self.status = "No write since last change (:q! to override)".to_owned();
            self.dirty = true;
            return;
        }
        if self.ui.focus.split == right {
            return;
        }
        let current = self.snapshot();
        let Some(other) = self.split.take() else {
            return;
        };
        self.split = Some(Pane { buffer: current });
        self.restore_buffer(other.buffer);
        self.ui.focus.split = right;
        self.sync_watch_paths();
        self.status = if right { "right pane" } else { "left pane" }.to_owned();
        self.dirty = true;
    }

    pub(crate) fn close_split(&mut self) {
        if self.split.is_none() {
            return;
        }
        if self.ui.focus.split && self.modified {
            self.status = "No write since last change (:q! to override)".to_owned();
            self.dirty = true;
            return;
        }
        if self.split.as_ref().is_some_and(|pane| pane.buffer.modified) {
            self.status = "No write since last change in split (:q! to override)".to_owned();
            self.dirty = true;
            return;
        }
        if self.ui.focus.split {
            let other = self.split.take().expect("split");
            self.restore_buffer(other.buffer);
        } else {
            self.split = None;
        }
        self.ui.focus.split = false;
        self.status = "split closed".to_owned();
        self.sync_watch_paths();
        self.dirty = true;
    }

    pub(crate) fn restore_buffer(&mut self, buffer: Buffer) {
        let active = match buffer.path.as_ref() {
            Some(path) => self
                .buffers
                .iter()
                .position(|candidate| candidate.path.as_ref() == Some(path)),
            None => self
                .buffers
                .get(self.active)
                .filter(|candidate| candidate.path.is_none())
                .map(|_| self.active)
                .or_else(|| {
                    self.buffers
                        .iter()
                        .position(|candidate| candidate.path.is_none())
                }),
        };
        self.editor = buffer.editor;
        self.path = buffer.path;
        self.cursor = buffer.cursor;
        self.scroll = buffer.scroll;
        self.hscroll = buffer.hscroll;
        self.modified = buffer.modified;
        self.missing = buffer.missing;
        self.visual_anchor = buffer.visual_anchor;
        self.visual_line = buffer.visual_line;
        self.search_query = buffer.search_query;
        self.search.reset_for_buffer();
        if let Some(active) = active {
            self.active = active;
        }
        self.ui.input_mode = InputMode::Normal;
        self.pending = None;
        self.syntax
            .borrow_mut()
            .switch_path(self.path.as_deref(), false);
        self.sync_lsp_active();
        self.dirty = true;
    }
}

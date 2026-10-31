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
        self.split_active = false;
        self.status = "vertical split".to_owned();
        self.dirty = true;
    }

    pub(crate) fn focus_split(&mut self, right: bool) {
        if self.split.is_none() {
            return;
        }
        if self.split_active && self.modified {
            self.status = "No write since last change (:q! to override)".to_owned();
            self.dirty = true;
            return;
        }
        if self.split_active == right {
            return;
        }
        let current = self.snapshot();
        let Some(other) = self.split.take() else {
            return;
        };
        self.split = Some(Pane { buffer: current });
        self.restore_buffer(other.buffer);
        self.split_active = right;
        self.status = if right { "right pane" } else { "left pane" }.to_owned();
        self.dirty = true;
    }

    pub(crate) fn close_split(&mut self) {
        if self.split.is_none() {
            return;
        }
        if self.split_active && self.modified {
            self.status = "No write since last change (:q! to override)".to_owned();
            self.dirty = true;
            return;
        }
        if self.split.as_ref().is_some_and(|pane| pane.buffer.modified) {
            self.status = "No write since last change in split (:q! to override)".to_owned();
            self.dirty = true;
            return;
        }
        if self.split_active {
            let other = self.split.take().expect("split");
            self.restore_buffer(other.buffer);
        } else {
            self.split = None;
        }
        self.split_active = false;
        self.status = "split closed".to_owned();
        self.dirty = true;
    }

    pub(crate) fn restore_buffer(&mut self, buffer: Buffer) {
        self.editor = buffer.editor;
        self.path = buffer.path;
        self.cursor = buffer.cursor;
        self.scroll = buffer.scroll;
        self.hscroll = buffer.hscroll;
        self.modified = buffer.modified;
        self.search_query = buffer.search_query;
        self.input_mode = InputMode::Normal;
        self.pending = None;
        self.syntax.borrow_mut().invalidate();
        self.sync_lsp_active();
        self.dirty = true;
    }
}

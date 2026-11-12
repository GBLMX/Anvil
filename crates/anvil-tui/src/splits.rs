//! Minimal two-pane vertical split state.

use super::*;

impl Model {
    pub(crate) fn split_vertical(&mut self) {
        if self.buffers.split.is_some() {
            self.status = "split already open".to_owned();
            self.dirty = true;
            return;
        }
        self.buffers.split = Some(Pane {
            buffer: self.snapshot(),
        });
        self.ui.focus.split = false;
        self.status = "vertical split".to_owned();
        self.sync_watch_paths();
        self.dirty = true;
    }

    pub(crate) fn focus_editor(&mut self) {
        if self.ui.focus.split {
            self.focus_split(false);
            if self.ui.focus.split {
                return;
            }
        }
        self.ui.focus.tree = false;
        self.dirty = true;
    }

    pub(crate) fn focus_split(&mut self, right: bool) {
        if self.buffers.split.is_none() {
            return;
        }
        if self.ui.focus.split == right {
            return;
        }
        if !self.auto_save_before_focus_change() {
            return;
        }
        if self.ui.focus.split && self.modified {
            self.status = "No write since last change (:q! to override)".to_owned();
            self.dirty = true;
            return;
        }
        let current = self.snapshot();
        let Some(other) = self.buffers.split.take() else {
            return;
        };
        self.buffers.split = Some(Pane { buffer: current });
        self.restore_buffer(other.buffer);
        self.ui.focus.split = right;
        self.sync_watch_paths();
        self.status = if right { "right pane" } else { "left pane" }.to_owned();
        self.dirty = true;
    }

    pub(crate) fn scroll_split_rows(&mut self, delta: isize) {
        let Some(pane) = self.buffers.split.as_mut() else {
            return;
        };
        let total = pane.buffer.editor.index().len_lines();
        if total == 0 {
            return;
        }
        pane.buffer.scroll = pane
            .buffer
            .scroll
            .saturating_add_signed(delta)
            .min(total.saturating_sub(1));
        self.dirty = true;
    }

    pub(crate) fn close_split(&mut self) {
        if self.buffers.split.is_none() {
            return;
        }
        if !self.auto_save_before_focus_change() {
            return;
        }
        if self.ui.focus.split && self.modified {
            self.status = "No write since last change (:q! to override)".to_owned();
            self.dirty = true;
            return;
        }
        if self
            .buffers
            .split
            .as_ref()
            .is_some_and(|pane| pane.buffer.modified)
        {
            self.status = "No write since last change in split (:q! to override)".to_owned();
            self.dirty = true;
            return;
        }
        if self.ui.focus.split {
            let other = self.buffers.split.take().expect("split");
            self.restore_buffer(other.buffer);
        } else {
            self.buffers.split = None;
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
                .entries
                .iter()
                .position(|candidate| candidate.path.as_ref() == Some(path)),
            None => self
                .buffers
                .entries
                .get(self.buffers.active)
                .filter(|candidate| candidate.path.is_none())
                .map(|_| self.buffers.active)
                .or_else(|| {
                    self.buffers
                        .entries
                        .iter()
                        .position(|candidate| candidate.path.is_none())
                }),
        };
        self.editor = buffer.editor;
        self.path = buffer.path;
        self.cursor = buffer.cursor;
        self.view.scroll = buffer.scroll;
        self.view.hscroll = buffer.hscroll;
        self.modified = buffer.modified;
        self.missing = buffer.missing;
        self.visual_anchor = buffer.visual_anchor;
        self.visual_line = buffer.visual_line;
        self.search.query = buffer.search_query;
        self.search.state.reset_for_buffer();
        if let Some(active) = active {
            self.buffers.active = active;
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

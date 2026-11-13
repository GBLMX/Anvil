//! Minimal two-pane vertical split state.

use super::*;

impl Model {
    pub(crate) fn split_vertical(&mut self) {
        if self.buffers.split.is_some() {
            self.status.text = "split already open".to_owned();
            self.file.dirty = true;
            return;
        }
        self.buffers.split = Some(Pane {
            buffer: self.snapshot(),
        });
        self.ui.focus.split = false;
        self.status.text = "vertical split".to_owned();
        self.sync_watch_paths();
        self.refresh_wrap_width();
        self.file.dirty = true;
    }

    pub(crate) fn focus_editor(&mut self) {
        if self.ui.focus.split {
            self.focus_split(false);
            if self.ui.focus.split {
                return;
            }
        }
        self.ui.focus.tree = false;
        self.file.dirty = true;
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
        if self.ui.focus.split && self.file.modified {
            self.status.text = "No write since last change (:q! to override)".to_owned();
            self.file.dirty = true;
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
        self.refresh_wrap_width();
        self.status.text = if right { "right pane" } else { "left pane" }.to_owned();
        self.file.dirty = true;
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
        self.file.dirty = true;
    }

    pub(crate) fn close_split(&mut self) {
        if self.buffers.split.is_none() {
            return;
        }
        if !self.auto_save_before_focus_change() {
            return;
        }
        if self.ui.focus.split && self.file.modified {
            self.status.text = "No write since last change (:q! to override)".to_owned();
            self.file.dirty = true;
            return;
        }
        if self
            .buffers
            .split
            .as_ref()
            .is_some_and(|pane| pane.buffer.modified)
        {
            self.status.text = "No write since last change in split (:q! to override)".to_owned();
            self.file.dirty = true;
            return;
        }
        if self.ui.focus.split {
            let other = self.buffers.split.take().expect("split");
            self.restore_buffer(other.buffer);
        } else {
            self.buffers.split = None;
        }
        self.ui.focus.split = false;
        self.status.text = "split closed".to_owned();
        self.sync_watch_paths();
        self.refresh_wrap_width();
        self.file.dirty = true;
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
        self.session.editor = buffer.editor;
        self.session.path = buffer.path;
        self.session.cursor = buffer.cursor;
        self.view.scroll = buffer.scroll;
        self.view.hscroll = buffer.hscroll;
        self.file.modified = buffer.modified;
        self.file.missing = buffer.missing;
        self.edit.visual_anchor = buffer.visual_anchor;
        self.edit.visual_line = buffer.visual_line;
        self.search.query = buffer.search_query;
        self.search.state.reset_for_buffer();
        if let Some(active) = active {
            self.buffers.active = active;
        }
        self.ui.input_mode = InputMode::Normal;
        self.input.pending = None;
        self.syntax_cache
            .highlights
            .borrow_mut()
            .switch_path(self.session.path.as_deref(), false);
        self.sync_lsp_active();
        self.file.dirty = true;
    }
}

//! Minimal two-pane vertical split state.

use super::*;

impl Model {
    pub(crate) fn split_vertical(&mut self) {
        self.open_split(crate::state::SplitOrientation::Vertical);
    }

    pub(crate) fn split_horizontal(&mut self) {
        self.open_split(crate::state::SplitOrientation::Horizontal);
    }

    fn open_split(&mut self, orientation: crate::state::SplitOrientation) {
        if self.buffers.split.is_some() {
            self.status.text = "split already open".to_owned();
            self.file.dirty = true;
            return;
        }
        self.ensure_current_buffer();
        self.view.split_orientation = orientation;
        self.buffers.split_left = Some(self.buffers.active);
        let document = self.buffers.active_id;
        let selections = self.current_selections();
        let active = self.buffers.entries.get(self.buffers.active);
        self.buffers.split = Some(Pane {
            buffer: View {
                id: ViewId::next(),
                document,
                cursor: selections
                    .first()
                    .map_or(self.session.cursor, |(_, head)| *head),
                selections,
                scroll: self.view.scroll,
                hscroll: self.view.hscroll,
                visual_anchor: self.edit.visual_anchor,
                visual_line: self.edit.visual_line,
                search_query: active.map_or_else(String::new, |view| view.search_query.clone()),
            },
        });
        self.ui.focus.split = false;
        self.status.text = match orientation {
            crate::state::SplitOrientation::Vertical => "vertical split",
            crate::state::SplitOrientation::Horizontal => "horizontal split",
        }
        .to_owned();
        self.sync_watch_paths();
        self.refresh_wrap_width();
        self.file.dirty = true;
    }

    pub(crate) fn load_view(&mut self, view: &View) {
        let Some(document) = self.buffers.document(view.document).cloned() else {
            return;
        };
        self.session.editor = document.session;
        self.session.path = document.path;
        self.view.scroll = view.scroll;
        self.view.hscroll = view.hscroll;
        self.file.modified = document.modified;
        self.file.missing = document.missing;
        self.edit.visual_line = view.visual_line;
        self.restore_view_selections(view);
        self.search.query = view.search_query.clone();
        self.search.state.reset_for_buffer();
        self.buffers.active_id = view.document;
        self.ui.input_mode = InputMode::Normal;
        self.input.pending = None;
        self.sync_lsp_active();
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
            if right {
                self.focus_editor();
            } else if self.workspace.tree.visible {
                self.ui.focus.tree = true;
                self.ui.focus.split = false;
                self.file.dirty = true;
            }
            return;
        }
        if self.ui.focus.split == right {
            return;
        }
        if !self.auto_save_before_focus_change() {
            return;
        }
        self.sync_active();
        if right {
            self.buffers.split_left.get_or_insert(self.buffers.active);
            let view = self
                .buffers
                .split
                .as_ref()
                .map(|pane| pane.buffer.clone())
                .expect("split view");
            if let Some(index) = self
                .buffers
                .entries
                .iter()
                .position(|active| active.document == view.document)
            {
                self.buffers.active = index;
            }
            self.load_view(&view);
        } else {
            let index = self
                .buffers
                .split_left
                .take()
                .unwrap_or(self.buffers.active);
            let Some(view) = self.buffers.entries.get(index).cloned() else {
                return;
            };
            self.buffers.active = index;
            self.load_view(&view);
        }
        self.ui.focus.split = right;
        self.sync_watch_paths();
        self.refresh_wrap_width();
        self.status.text = if right { "right pane" } else { "left pane" }.to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn scroll_split_rows(&mut self, delta: isize) {
        let Some(view) = self.buffers.split.as_ref() else {
            return;
        };
        let Some(document) = self.buffers.document(view.buffer.document) else {
            return;
        };
        let total = document.session.index().len_lines();
        if total == 0 {
            return;
        }
        let Some(pane) = self.buffers.split.as_mut() else {
            return;
        };
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
        let was_focused = self.ui.focus.split;
        self.sync_active();
        self.buffers.split = None;
        self.ui.focus.split = false;
        if was_focused {
            let index = self
                .buffers
                .split_left
                .take()
                .unwrap_or(self.buffers.active);
            self.load_buffer(index);
        } else {
            self.buffers.split_left = None;
        }
        self.status.text = "split closed".to_owned();
        self.sync_watch_paths();
        self.refresh_wrap_width();
        self.file.dirty = true;
    }
}

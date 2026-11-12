//! Operation-scoped modal layers.

use super::*;

impl Model {
    pub(crate) fn begin_surround(&mut self) {
        if self.edit.visual_anchor.is_none() {
            self.status.text = "E: surround needs a visual selection".to_owned();
            self.dirty = true;
            return;
        }
        self.edit.modal_layer = Some(ModalLayer::Surround);
        self.status.text = "surround: ( [ { < \" ' ` ; Esc cancels".to_owned();
        self.dirty = true;
    }

    pub(crate) fn cancel_modal_layer(&mut self) {
        if self.edit.modal_layer.take().is_some() {
            self.status.text = "surround cancelled".to_owned();
            self.dirty = true;
        }
    }

    pub(crate) fn surround_selection(&mut self, open: char, close: char) {
        if self.reject_read_only() {
            self.edit.modal_layer = None;
            self.exit_visual();
            return;
        }
        let selections = self.editor.selection_bytes();
        let mut edits = Vec::new();
        for (anchor, head) in selections {
            let start = anchor.min(head);
            let end = anchor.max(head);
            if start == end {
                continue;
            }
            edits.push((end..end, close.to_string()));
            edits.push((start..start, open.to_string()));
        }
        if edits.is_empty() || !self.editor.apply_transaction(edits) {
            self.cancel_modal_layer();
            return;
        }
        self.edit.modal_layer = None;
        self.sync_cursor_from_selection();
        self.modified = true;
        self.exit_visual();
        self.status.text = format!("surrounded with {open}{close}");
        self.dirty = true;
    }
}

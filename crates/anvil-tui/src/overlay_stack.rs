use super::*;

impl Model {
    pub(crate) fn top_overlay(&self) -> Option<OverlayKind> {
        self.overlays.stack.last().map(|entry| entry.kind)
    }

    pub(crate) fn push_overlay(&mut self, kind: OverlayKind) {
        if self.top_overlay() == Some(kind) {
            return;
        }
        self.overlays.stack.push(OverlayEntry {
            kind,
            focus_before: self.ui.focus,
        });
        self.file.dirty = true;
    }

    pub(crate) fn close_overlay(&mut self, kind: OverlayKind) -> bool {
        let Some(index) = self
            .overlays
            .stack
            .iter()
            .rposition(|entry| entry.kind == kind)
        else {
            return false;
        };
        let was_top = index + 1 == self.overlays.stack.len();
        let entry = self.overlays.stack.remove(index);
        if was_top {
            self.ui.focus = entry.focus_before;
            self.file.dirty = true;
        }
        true
    }
}

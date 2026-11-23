use crate::surface::SurfaceId;

use super::*;

impl Model {
    pub(crate) fn top_overlay(&self) -> Option<OverlayKind> {
        self.overlays.stack.last().map(|entry| entry.kind)
    }

    pub(crate) fn push_overlay(&mut self, kind: OverlayKind) {
        if self.top_overlay() == Some(kind) {
            return;
        }
        if self.overlays.stack.is_empty() {
            self.surfaces.set_base(self.base_surface());
        }
        self.surfaces.push_overlay(SurfaceId::from(kind));
        self.overlays.stack.push(OverlayEntry { kind });
        self.file.dirty = true;
        if kind == OverlayKind::CommandPalette {
            self.motion.borrow_mut().start(
                kind,
                Instant::now(),
                self.config.settings.ui.reduced_motion,
            );
        }
    }

    pub(crate) fn push_overlay_if_idle(&mut self, kind: OverlayKind) {
        if self.top_overlay().is_none() {
            self.push_overlay(kind);
        }
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
        self.overlays.stack.remove(index);
        self.surfaces.remove_overlay(index);
        if was_top {
            if let Some(surface) = self.surfaces.top() {
                self.ui.focus = surface.focus_state();
            } else {
                self.restore_base_focus();
            }
            self.file.dirty = true;
        }
        true
    }
}

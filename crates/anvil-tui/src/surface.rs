//! Surface identity, classification, and focus history.

use super::*;
use anvil_session::ui::FocusState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorPane {
    Primary,
    Split,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockId {
    Tree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PageId {
    Start,
    Settings,
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SurfaceClass {
    Page,
    Dock,
    Editor,
    Overlay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SurfaceId {
    Page(PageId),
    Dock(DockId),
    Editor(EditorPane),
    Overlay(OverlayKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SurfaceSpec {
    pub(crate) class: SurfaceClass,
    pub(crate) keymap_context: KeymapContext,
}

impl SurfaceId {
    pub(crate) fn spec(self) -> SurfaceSpec {
        let (class, keymap_context) = match self {
            Self::Page(_) => (SurfaceClass::Page, KeymapContext::Popup),
            Self::Dock(DockId::Tree) => (SurfaceClass::Dock, KeymapContext::FileTree),
            Self::Editor(_) => (SurfaceClass::Editor, KeymapContext::Editor),
            Self::Overlay(_) => (SurfaceClass::Overlay, KeymapContext::Popup),
        };
        SurfaceSpec {
            class,
            keymap_context,
        }
    }

    pub(crate) fn focus_state(self) -> FocusState {
        match self {
            Self::Dock(DockId::Tree) => FocusState {
                tree: true,
                split: false,
            },
            Self::Editor(EditorPane::Split) => FocusState {
                tree: false,
                split: true,
            },
            Self::Page(_) | Self::Editor(EditorPane::Primary) | Self::Overlay(_) => {
                FocusState::default()
            }
        }
    }
}

impl From<OverlayKind> for SurfaceId {
    fn from(kind: OverlayKind) -> Self {
        match kind {
            OverlayKind::Settings => Self::Page(PageId::Settings),
            kind => Self::Overlay(kind),
        }
    }
}

/// Focusable surface history above the current base surface.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FocusStack {
    base: Option<SurfaceId>,
    overlays: Vec<SurfaceId>,
}

impl FocusStack {
    pub(crate) fn base(&self) -> Option<SurfaceId> {
        self.base
    }

    pub(crate) fn set_base(&mut self, base: SurfaceId) {
        self.base = Some(base);
    }

    pub(crate) fn top(&self) -> Option<SurfaceId> {
        self.overlays.last().copied()
    }

    pub(crate) fn push_overlay(&mut self, surface: SurfaceId) {
        self.overlays.push(surface);
    }

    pub(crate) fn remove_overlay(&mut self, index: usize) -> Option<SurfaceId> {
        (index < self.overlays.len()).then(|| self.overlays.remove(index))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.overlays.is_empty()
    }
}

impl Model {
    pub(crate) fn base_surface(&self) -> SurfaceId {
        if self.start_page.welcome {
            SurfaceId::Page(PageId::Start)
        } else if self.pager.back.is_some() {
            SurfaceId::Page(PageId::Help)
        } else if self.ui.focus.tree {
            SurfaceId::Dock(DockId::Tree)
        } else if self.ui.focus.split {
            SurfaceId::Editor(EditorPane::Split)
        } else {
            SurfaceId::Editor(EditorPane::Primary)
        }
    }

    pub(crate) fn active_surface(&self) -> SurfaceId {
        if self.surfaces.is_empty() {
            self.base_surface()
        } else {
            self.surfaces.top().expect("non-empty focus stack")
        }
    }

    pub(crate) fn restore_base_focus(&mut self) {
        self.ui.focus = self
            .surfaces
            .base()
            .map_or_else(FocusState::default, SurfaceId::focus_state);
    }

    pub(crate) fn cycle_surface(&mut self, forward: bool) -> bool {
        if self.ui.input_mode != InputMode::Normal
            || self.edit.visual_anchor.is_some()
            || self.start_page.welcome
            || self.pager.back.is_some()
        {
            return false;
        }
        let mut order = Vec::new();
        if self.workspace.tree.visible {
            order.push(SurfaceId::Dock(DockId::Tree));
        }
        order.push(SurfaceId::Editor(EditorPane::Primary));
        if self.buffers.split.is_some() {
            order.push(SurfaceId::Editor(EditorPane::Split));
        }
        let current = self.base_surface();
        let Some(index) = order.iter().position(|surface| *surface == current) else {
            return false;
        };
        let next = if forward {
            order[(index + 1) % order.len()]
        } else {
            order[(index + order.len() - 1) % order.len()]
        };
        match next {
            SurfaceId::Dock(DockId::Tree) => {
                self.ui.focus.tree = true;
                self.ui.focus.split = false;
                self.refresh_wrap_width();
                self.file.dirty = true;
                true
            }
            SurfaceId::Editor(EditorPane::Primary) => {
                self.focus_editor();
                if self.ui.focus.split {
                    return false;
                }
                self.file.dirty = true;
                true
            }
            SurfaceId::Editor(EditorPane::Split) => {
                self.focus_split(true);
                self.ui.focus.split
            }
            SurfaceId::Page(_) | SurfaceId::Overlay(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_and_keymap_contexts_are_explicit() {
        assert_eq!(
            SurfaceId::Dock(DockId::Tree).spec().class,
            SurfaceClass::Dock
        );
        assert_eq!(
            SurfaceId::Dock(DockId::Tree).spec().keymap_context,
            KeymapContext::FileTree
        );
        assert_eq!(
            SurfaceId::Editor(EditorPane::Split).spec().class,
            SurfaceClass::Editor
        );
        assert_eq!(
            SurfaceId::Page(PageId::Settings).spec().class,
            SurfaceClass::Page
        );
        assert_eq!(
            SurfaceId::from(OverlayKind::Settings),
            SurfaceId::Page(PageId::Settings)
        );
        assert_eq!(
            SurfaceId::from(OverlayKind::Fuzzy).spec().class,
            SurfaceClass::Overlay
        );
    }

    #[test]
    fn focus_stack_restores_each_level() {
        let mut stack = FocusStack::default();
        stack.set_base(SurfaceId::Dock(DockId::Tree));
        stack.push_overlay(SurfaceId::Page(PageId::Settings));
        stack.push_overlay(SurfaceId::Overlay(OverlayKind::Fuzzy));
        assert_eq!(stack.top(), Some(SurfaceId::Overlay(OverlayKind::Fuzzy)));
        assert_eq!(
            stack.remove_overlay(1),
            Some(SurfaceId::Overlay(OverlayKind::Fuzzy))
        );
        assert_eq!(stack.top(), Some(SurfaceId::Page(PageId::Settings)));
        assert_eq!(
            stack.remove_overlay(0),
            Some(SurfaceId::Page(PageId::Settings))
        );
        assert!(stack.is_empty());
        assert_eq!(
            stack.base().map(SurfaceId::focus_state),
            Some(FocusState {
                tree: true,
                split: false,
            })
        );
    }

    #[test]
    fn surface_cycle_moves_between_tree_and_editor() {
        let mut model = Model::new(EditorState::from_text("alpha"), None);
        model.start_page.welcome = false;
        model.workspace.tree.visible = true;
        model.ui.focus.tree = true;
        assert!(model.cycle_surface(true));
        assert!(!model.ui.focus.tree);
        assert_eq!(model.base_surface(), SurfaceId::Editor(EditorPane::Primary));
        assert!(model.cycle_surface(true));
        assert!(model.ui.focus.tree);
        assert_eq!(model.base_surface(), SurfaceId::Dock(DockId::Tree));
    }

    #[test]
    fn surface_cycle_includes_the_split_pane() {
        let mut model = Model::new(EditorState::from_text("alpha"), None);
        model.start_page.welcome = false;
        model.workspace.tree.visible = false;
        model.split_vertical();
        assert!(!model.ui.focus.split);
        assert!(model.cycle_surface(true));
        assert!(model.ui.focus.split);
        assert!(model.cycle_surface(false));
        assert!(!model.ui.focus.split);
    }
}

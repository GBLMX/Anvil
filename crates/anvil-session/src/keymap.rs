//! Frontend-neutral keymap routing contexts.
//!
//! The terminal frontend owns crossterm events and the actual router; these
//! identifiers define the stable priority vocabulary shared with future
//! frontends and the command registry.

/// The UI surface that owns the next key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeymapContext {
    /// Fallback bindings that apply everywhere.
    Global,
    /// The text editor and its normal/insert input modes.
    Editor,
    /// The workspace file tree.
    FileTree,
    /// Incremental search input.
    Search,
    /// Symbol or file rename input.
    Rename,
    /// The command-line prompt.
    CommandLine,
    /// Modal overlays such as completion, symbols, code actions, or fuzzy
    /// pickers.
    Popup,
    /// Operation-scoped transient bindings, such as surround or resize.
    Modal,
}

impl KeymapContext {
    /// Higher values consume key events before lower values.
    pub const fn priority(self) -> u8 {
        match self {
            Self::Global => 0,
            Self::Editor => 1,
            Self::FileTree => 2,
            Self::Search => 3,
            Self::Rename => 4,
            Self::CommandLine => 5,
            Self::Popup => 6,
            Self::Modal => 7,
        }
    }

    /// Contexts from highest to lowest routing priority.
    pub const fn route_order() -> [Self; 8] {
        [
            Self::Modal,
            Self::Popup,
            Self::CommandLine,
            Self::Rename,
            Self::Search,
            Self::FileTree,
            Self::Editor,
            Self::Global,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_order_is_strictly_priority_ordered() {
        let order = KeymapContext::route_order();
        assert_eq!(order.len(), 8);
        for pair in order.windows(2) {
            assert!(
                pair[0].priority() > pair[1].priority(),
                "{:?} must route before {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn popup_and_modal_outrank_editor_and_global() {
        assert!(KeymapContext::Popup.priority() > KeymapContext::Editor.priority());
        assert!(KeymapContext::Popup.priority() > KeymapContext::FileTree.priority());
        assert!(KeymapContext::Modal.priority() > KeymapContext::Popup.priority());
        assert!(KeymapContext::Global.priority() < KeymapContext::Editor.priority());
    }
}

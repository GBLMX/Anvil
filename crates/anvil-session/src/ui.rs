//! Frontend-neutral focus ownership.

/// Which surface currently owns keyboard focus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FocusState {
    /// The workspace tree owns focus.
    pub tree: bool,
    /// The right-hand split pane owns focus.
    pub split: bool,
}

//! Frontend-neutral focus and overlay state.

use std::path::PathBuf;

/// Focus and overlay state owned by a frontend session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiState {
    pub focus: FocusState,
    pub input_mode: InputMode,
}

/// Which surface currently owns keyboard focus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FocusState {
    /// The workspace tree owns focus.
    pub tree: bool,
    /// The right-hand split pane owns focus.
    pub split: bool,
}

/// Kind of entry created from the workspace tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateKind {
    File,
    Directory,
}

/// The small frontend-neutral input and overlay state machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum InputMode {
    /// Navigation, insert entry, command entry.
    #[default]
    Normal,
    /// Text insertion at the cursor.
    Insert,
    /// Rename symbol input.
    Rename { name: String },
    /// Ex-style command line.
    CommandLine { buffer: String },
    /// Incremental substring search.
    Search { query: String },
    /// Create a file or directory in the tree.
    Create {
        directory: PathBuf,
        name: String,
        kind: CreateKind,
    },
    /// Rename a tree entry.
    RenameEntry { path: PathBuf, name: String },
    /// Confirm deletion of a tree entry.
    ConfirmDelete { path: PathBuf },
    /// Confirm closing a modified buffer.
    ConfirmClose { index: usize },
    /// Confirm reloading a dirty buffer changed on disk.
    ConfirmReload { path: PathBuf },
    /// Move or copy a tree entry into a destination directory.
    TransferEntry {
        path: PathBuf,
        copy: bool,
        directory: String,
    },
}

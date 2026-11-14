//! Frontend-neutral document snapshot and transaction boundary.
//!
//! The terminal frontend and a future headless frontend use the same session
//! contract; this crate does not depend on the TUI.

pub mod command;
pub mod editor_service;
pub mod effect;
pub mod format;
mod host_session;
pub mod keymap;
pub mod plugin_events;
pub mod plugin_provider;
pub mod plugin_session;
pub mod plugin_supervisor;
pub mod provider;
pub mod ui;
use anvil_api::{ApplyEditRequest, DocumentSnapshot};
use anvil_core::TextBuffer;
use anvil_editor::{ApplyEditError, EditorState};
use std::fmt;

/// Why a document session operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    /// The request names a different buffer than the session.
    BufferMismatch { expected: u64, actual: u64 },
    /// The transaction is based on an old revision.
    StaleRevision { expected: u64, actual: u64 },
    /// The transaction contains invalid ranges.
    InvalidEdit,
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferMismatch { expected, actual } => {
                write!(
                    formatter,
                    "buffer mismatch: expected {expected}, actual {actual}"
                )
            }
            Self::StaleRevision { expected, actual } => {
                write!(
                    formatter,
                    "stale revision: expected {expected}, actual {actual}"
                )
            }
            Self::InvalidEdit => formatter.write_str("invalid edit transaction"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<ApplyEditError> for SessionError {
    fn from(error: ApplyEditError) -> Self {
        match error {
            ApplyEditError::StaleRevision { expected, actual } => {
                Self::StaleRevision { expected, actual }
            }
            ApplyEditError::InvalidEdit => Self::InvalidEdit,
        }
    }
}

/// Owned active-document state used by a frontend session.
#[derive(Debug, Clone)]
pub struct DocumentSession {
    editor: EditorState,
}

impl DocumentSession {
    pub fn new(editor: EditorState) -> Self {
        Self { editor }
    }

    pub fn into_inner(self) -> EditorState {
        self.editor
    }
}

impl From<EditorState> for DocumentSession {
    fn from(editor: EditorState) -> Self {
        Self::new(editor)
    }
}

impl std::ops::Deref for DocumentSession {
    type Target = EditorState;

    fn deref(&self) -> &Self::Target {
        &self.editor
    }
}

impl std::ops::DerefMut for DocumentSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.editor
    }
}
/// One active document in a frontend-neutral session.
pub struct Session<'a> {
    editor: &'a mut EditorState,
    buffer_id: u64,
}

impl<'a> Session<'a> {
    /// Creates a session view over one active editor buffer.
    pub fn new(editor: &'a mut EditorState, buffer_id: u64) -> Self {
        Self { editor, buffer_id }
    }

    /// Reads a snapshot without transferring editor ownership.
    pub fn snapshot(editor: &EditorState, buffer_id: u64) -> DocumentSnapshot {
        DocumentSnapshot {
            buffer_id,
            revision: editor.revision(),
            text: editor.text().slice(0..editor.text().len_bytes()),
        }
    }

    /// Applies one revision-checked transaction.
    pub fn apply(&mut self, request: ApplyEditRequest) -> Result<u64, SessionError> {
        if request.buffer_id != self.buffer_id {
            return Err(SessionError::BufferMismatch {
                expected: self.buffer_id,
                actual: request.buffer_id,
            });
        }
        let edits = request
            .edits
            .into_iter()
            .map(|edit| {
                let start =
                    usize::try_from(edit.range.start).map_err(|_| SessionError::InvalidEdit)?;
                let end = usize::try_from(edit.range.end).map_err(|_| SessionError::InvalidEdit)?;
                Ok((start..end, edit.text))
            })
            .collect::<Result<Vec<_>, SessionError>>()?;
        Ok(self
            .editor
            .apply_transaction_at(request.base_revision, edits)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_api::{DocumentEdit, TextRange};

    #[test]
    fn snapshot_and_transaction_are_frontend_neutral() {
        let mut editor = EditorState::from_text("abcd");
        let snapshot = Session::snapshot(&editor, 1);
        assert_eq!(snapshot.text, "abcd");
        let revision = Session::new(&mut editor, 1)
            .apply(ApplyEditRequest {
                buffer_id: 1,
                base_revision: snapshot.revision,
                edits: vec![DocumentEdit {
                    range: TextRange { start: 1, end: 3 },
                    text: "X".to_owned(),
                }],
            })
            .expect("apply");
        assert_eq!(revision, snapshot.revision + 1);
        assert_eq!(editor.text().slice(0..3), "aXd");
    }
}

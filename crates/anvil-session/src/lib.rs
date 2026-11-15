//! Frontend-neutral document snapshot and transaction boundary.
//!
//! The terminal frontend and a future headless frontend use the same session
//! contract; this crate does not depend on the TUI.

pub mod command;
pub mod editor_service;
pub mod effect;
pub mod format;
mod host_session;
pub mod jump;
pub mod keymap;
pub mod plugin_events;
pub mod plugin_provider;
pub mod plugin_runtime;
pub mod plugin_session;
pub mod plugin_supervisor;
pub mod provider;
pub mod recents;
pub mod ui;
use anvil_api::{ApplyEditRequest, DocumentSnapshot, InteractionMode, Viewport};
use anvil_core::TextBuffer;
use anvil_editor::{ApplyEditError, EditError, EditorState, MacroStep, PositionError};
use std::{fmt, ops::Range};

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

    pub fn set_selection_bytes(&mut self, anchor: usize, head: usize) -> bool {
        self.editor.set_selection_bytes(anchor, head)
    }

    pub fn try_set_selection_bytes(
        &mut self,
        anchor: usize,
        head: usize,
    ) -> Result<(), PositionError> {
        self.editor.try_set_selection_bytes(anchor, head)
    }

    pub fn set_selections_bytes(&mut self, selections: Vec<(usize, usize)>) -> bool {
        self.editor.set_selections_bytes(selections)
    }

    pub fn try_set_selections_bytes(
        &mut self,
        selections: Vec<(usize, usize)>,
    ) -> Result<(), PositionError> {
        self.editor.try_set_selections_bytes(selections)
    }

    pub fn insert(&mut self, at: usize, text: &str) -> bool {
        self.editor.insert(at, text)
    }

    pub fn try_insert(&mut self, at: usize, text: &str) -> Result<(), EditError> {
        self.editor.try_insert(at, text)
    }

    pub fn delete(&mut self, range: Range<usize>) -> bool {
        self.editor.delete(range)
    }

    pub fn try_delete(&mut self, range: Range<usize>) -> Result<(), EditError> {
        self.editor.try_delete(range)
    }

    pub fn apply_transaction(&mut self, edits: Vec<(Range<usize>, String)>) -> bool {
        self.editor.apply_transaction(edits)
    }

    pub fn try_apply_transaction(
        &mut self,
        edits: Vec<(Range<usize>, String)>,
    ) -> Result<(), EditError> {
        self.editor.try_apply_transaction(edits)
    }

    pub fn apply_transaction_at(
        &mut self,
        base_revision: u64,
        edits: Vec<(Range<usize>, String)>,
    ) -> Result<u64, ApplyEditError> {
        self.editor.apply_transaction_at(base_revision, edits)
    }

    pub fn insert_at_selections(&mut self, text: &str) -> bool {
        self.editor.insert_at_selections(text)
    }

    pub fn yank(&mut self, range: Range<usize>) -> bool {
        self.editor.yank(range)
    }

    pub fn put(&mut self, at: usize) -> Option<Range<usize>> {
        self.editor.put(at)
    }

    pub fn set_register(&mut self, register: char, text: &str) -> bool {
        self.editor.set_register(register, text)
    }

    pub fn set_macro(&mut self, register: char, steps: Vec<MacroStep>) {
        self.editor.set_macro(register, steps);
    }

    pub fn set_interaction_mode(&mut self, mode: InteractionMode) {
        self.editor.set_interaction_mode(mode);
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.editor.set_viewport(viewport);
    }

    pub fn undo(&mut self) -> bool {
        self.editor.undo()
    }

    pub fn redo(&mut self) -> bool {
        self.editor.redo()
    }

    pub fn apply_edits(
        &mut self,
        buffer_id: u64,
        request: ApplyEditRequest,
    ) -> Result<u64, SessionError> {
        Session::new(&mut self.editor, buffer_id).apply(request)
    }

    pub fn apply_format(
        &mut self,
        base_revision: u64,
        formatted: &crate::provider::FormattedText,
    ) -> Result<bool, SessionError> {
        crate::format::FormatService::apply(&mut self.editor, base_revision, formatted)
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

    #[test]
    fn session_rejects_stale_or_foreign_request_without_mutation() {
        let mut editor = EditorState::from_text("abcd");
        let snapshot = Session::snapshot(&editor, 1);
        let edit = DocumentEdit {
            range: TextRange { start: 1, end: 3 },
            text: "X".to_owned(),
        };
        let foreign = Session::new(&mut editor, 2).apply(ApplyEditRequest {
            buffer_id: 1,
            base_revision: snapshot.revision,
            edits: vec![edit.clone()],
        });
        assert_eq!(
            foreign,
            Err(SessionError::BufferMismatch {
                expected: 2,
                actual: 1
            })
        );
        let stale = Session::new(&mut editor, 1).apply(ApplyEditRequest {
            buffer_id: 1,
            base_revision: snapshot.revision + 1,
            edits: vec![edit],
        });
        assert_eq!(
            stale,
            Err(SessionError::StaleRevision {
                expected: snapshot.revision + 1,
                actual: snapshot.revision
            })
        );
        assert_eq!(editor.text().slice(0..4), "abcd");
    }
}

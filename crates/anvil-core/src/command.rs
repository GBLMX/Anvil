//! Edit commands, undo/redo, and revision tracking.
//!
//! The history is independent of the concrete [`TextBuffer`] backend. An
//! application owns one buffer and one history per document.

use std::{error::Error, fmt, ops::Range};

use crate::TextBuffer;

/// Monotonic document revision.
pub type Revision = u64;

/// One reversible text edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditCommand {
    /// Insert `text` at a valid UTF-8 byte boundary.
    Insert { at: usize, text: String },
    /// Delete a non-empty half-open byte range.
    Delete { range: Range<usize> },
}

/// Why an edit could not be applied or replayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    StaleRevision {
        expected: Revision,
        actual: Revision,
    },
    OutOfBounds,
    NotBoundary,
    EmptyEdit,
    NothingToUndo,
    NothingToRedo,
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleRevision { expected, actual } => {
                write!(
                    formatter,
                    "stale revision: expected {expected}, actual {actual}"
                )
            }
            Self::OutOfBounds => formatter.write_str("edit range is out of bounds"),
            Self::NotBoundary => formatter.write_str("edit offset is not a UTF-8 boundary"),
            Self::EmptyEdit => formatter.write_str("edit is empty"),
            Self::NothingToUndo => formatter.write_str("nothing to undo"),
            Self::NothingToRedo => formatter.write_str("nothing to redo"),
        }
    }
}

impl Error for CommandError {}

#[derive(Debug)]
struct HistoryEntry {
    forward: EditCommand,
    inverse: EditCommand,
    typing: bool,
}

/// Undo/redo stack with a monotonically increasing revision.
#[derive(Debug, Default)]
pub struct History {
    revision: Revision,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

impl History {
    /// Current document revision.
    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Applies an edit at the expected revision.
    pub fn apply<B: TextBuffer>(
        &mut self,
        buffer: &mut B,
        expected: Revision,
        command: EditCommand,
    ) -> Result<Revision, CommandError> {
        self.check_revision(expected)?;
        let inverse = command.inverse(buffer)?;
        command.apply(buffer);
        self.undo.push(HistoryEntry {
            forward: command,
            inverse,
            typing: false,
        });
        self.redo.clear();
        self.revision += 1;
        Ok(self.revision)
    }

    /// Applies a typing insert and merges contiguous typing into one undo step.
    pub fn apply_typing<B: TextBuffer>(
        &mut self,
        buffer: &mut B,
        expected: Revision,
        at: usize,
        text: &str,
    ) -> Result<Revision, CommandError> {
        self.check_revision(expected)?;
        let command = EditCommand::Insert {
            at,
            text: text.to_owned(),
        };
        let inverse = command.inverse(buffer)?;
        command.apply(buffer);

        if let Some(entry) = self.undo.last_mut()
            && entry.typing
            && let EditCommand::Insert {
                at: previous_at,
                text: previous_text,
            } = &mut entry.forward
            && *previous_at + previous_text.len() == at
        {
            previous_text.push_str(text);
            entry.inverse = EditCommand::Delete {
                range: *previous_at..*previous_at + previous_text.len(),
            };
        } else {
            self.undo.push(HistoryEntry {
                forward: command,
                inverse,
                typing: true,
            });
        }
        self.redo.clear();
        self.revision += 1;
        Ok(self.revision)
    }

    /// Reverts the most recent edit.
    pub fn undo<B: TextBuffer>(
        &mut self,
        buffer: &mut B,
        expected: Revision,
    ) -> Result<Revision, CommandError> {
        self.check_revision(expected)?;
        let entry = self.undo.pop().ok_or(CommandError::NothingToUndo)?;
        entry.inverse.apply(buffer);
        self.redo.push(entry);
        self.revision += 1;
        Ok(self.revision)
    }

    /// Reapplies the most recently undone edit.
    pub fn redo<B: TextBuffer>(
        &mut self,
        buffer: &mut B,
        expected: Revision,
    ) -> Result<Revision, CommandError> {
        self.check_revision(expected)?;
        let entry = self.redo.pop().ok_or(CommandError::NothingToRedo)?;
        entry.forward.apply(buffer);
        self.undo.push(entry);
        self.revision += 1;
        Ok(self.revision)
    }

    fn check_revision(&self, expected: Revision) -> Result<(), CommandError> {
        if expected != self.revision {
            return Err(CommandError::StaleRevision {
                expected,
                actual: self.revision,
            });
        }
        Ok(())
    }
}

impl EditCommand {
    fn validate<B: TextBuffer>(&self, buffer: &B) -> Result<(), CommandError> {
        match self {
            Self::Insert { at, text } => {
                if text.is_empty() {
                    return Err(CommandError::EmptyEdit);
                }
                if *at > buffer.len_bytes() {
                    return Err(CommandError::OutOfBounds);
                }
                if !buffer.is_char_boundary(*at) {
                    return Err(CommandError::NotBoundary);
                }
            }
            Self::Delete { range } => {
                if range.start >= range.end {
                    return Err(CommandError::EmptyEdit);
                }
                if range.end > buffer.len_bytes() {
                    return Err(CommandError::OutOfBounds);
                }
                if !buffer.is_char_boundary(range.start) || !buffer.is_char_boundary(range.end) {
                    return Err(CommandError::NotBoundary);
                }
            }
        }
        Ok(())
    }

    fn inverse<B: TextBuffer>(&self, buffer: &B) -> Result<Self, CommandError> {
        self.validate(buffer)?;
        Ok(match self {
            Self::Insert { at, text } => Self::Delete {
                range: *at..*at + text.len(),
            },
            Self::Delete { range } => Self::Insert {
                at: range.start,
                text: buffer.slice(range.clone()),
            },
        })
    }

    fn apply<B: TextBuffer>(&self, buffer: &mut B) {
        match self {
            Self::Insert { at, text } => buffer.insert(*at, text),
            Self::Delete { range } => buffer.delete(range.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GapBuffer, PieceTable, RopeBuffer};

    fn exercise<B: TextBuffer>() {
        let mut buffer = B::from_text("ab");
        let mut history = History::default();

        let revision = history
            .apply(
                &mut buffer,
                0,
                EditCommand::Insert {
                    at: 1,
                    text: "X".to_owned(),
                },
            )
            .unwrap();
        assert_eq!(revision, 1);
        assert_eq!(buffer.slice(0..buffer.len_bytes()), "aXb");

        let revision = history.undo(&mut buffer, 1).unwrap();
        assert_eq!(revision, 2);
        assert_eq!(buffer.slice(0..buffer.len_bytes()), "ab");

        let revision = history.redo(&mut buffer, 2).unwrap();
        assert_eq!(revision, 3);
        assert_eq!(buffer.slice(0..buffer.len_bytes()), "aXb");

        let revision = history
            .apply(&mut buffer, 3, EditCommand::Delete { range: 1..2 })
            .unwrap();
        assert_eq!(revision, 4);
        assert_eq!(buffer.slice(0..buffer.len_bytes()), "ab");

        let revision = history.undo(&mut buffer, 4).unwrap();
        assert_eq!(revision, 5);
        assert_eq!(buffer.slice(0..buffer.len_bytes()), "aXb");
    }

    #[test]
    fn history_works_for_all_backends() {
        exercise::<RopeBuffer>();
        exercise::<GapBuffer>();
        exercise::<PieceTable>();
    }

    #[test]
    fn stale_revision_is_rejected() {
        let mut buffer = PieceTable::from_text("a");
        let mut history = History::default();
        assert_eq!(
            history.apply(&mut buffer, 1, EditCommand::Delete { range: 0..1 }),
            Err(CommandError::StaleRevision {
                expected: 1,
                actual: 0
            })
        );
    }

    #[test]
    fn invalid_boundary_is_rejected() {
        let mut buffer = PieceTable::from_text("é");
        let mut history = History::default();
        assert_eq!(
            history.apply(
                &mut buffer,
                0,
                EditCommand::Insert {
                    at: 1,
                    text: "x".to_owned()
                }
            ),
            Err(CommandError::NotBoundary)
        );
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut buffer = PieceTable::from_text("a");
        let mut history = History::default();
        history
            .apply(
                &mut buffer,
                0,
                EditCommand::Insert {
                    at: 1,
                    text: "b".to_owned(),
                },
            )
            .unwrap();
        history.undo(&mut buffer, 1).unwrap();
        history
            .apply(
                &mut buffer,
                2,
                EditCommand::Insert {
                    at: 1,
                    text: "c".to_owned(),
                },
            )
            .unwrap();
        assert_eq!(
            history.redo(&mut buffer, 3),
            Err(CommandError::NothingToRedo)
        );
        assert_eq!(buffer.slice(0..buffer.len_bytes()), "ac");
    }

    fn typing_group<B: TextBuffer>() {
        let mut buffer = B::from_text("");
        let mut history = History::default();
        history.apply_typing(&mut buffer, 0, 0, "a").unwrap();
        history.apply_typing(&mut buffer, 1, 1, "b").unwrap();
        history.apply_typing(&mut buffer, 2, 2, "c").unwrap();
        assert_eq!(buffer.slice(0..buffer.len_bytes()), "abc");

        let revision = history.undo(&mut buffer, 3).unwrap();
        assert_eq!(revision, 4);
        assert_eq!(buffer.slice(0..buffer.len_bytes()), "");

        let revision = history.redo(&mut buffer, 4).unwrap();
        assert_eq!(revision, 5);
        assert_eq!(buffer.slice(0..buffer.len_bytes()), "abc");
    }

    #[test]
    fn contiguous_typing_is_one_undo_group() {
        typing_group::<RopeBuffer>();
        typing_group::<GapBuffer>();
        typing_group::<PieceTable>();
    }

    #[test]
    fn non_contiguous_typing_starts_a_new_group() {
        let mut buffer = PieceTable::from_text("");
        let mut history = History::default();
        history.apply_typing(&mut buffer, 0, 0, "a").unwrap();
        history.apply_typing(&mut buffer, 1, 0, "b").unwrap();
        assert_eq!(buffer.slice(0..buffer.len_bytes()), "ba");
        history.undo(&mut buffer, 2).unwrap();
        assert_eq!(buffer.slice(0..buffer.len_bytes()), "a");
    }
}

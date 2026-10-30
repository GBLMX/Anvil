//! Editor state, immutable snapshots, and `editor.get_view` projection.

use std::{ops::Range, sync::Arc};

use anvil_api::{EditorView, InteractionMode, Selection, TextPosition, Viewport};
use anvil_core::{LineIndex, TextBuffer, TextDocument};

/// Default viewport until the TUI supplies the terminal size.
pub const DEFAULT_VIEWPORT: Viewport = Viewport {
    first_line: 0,
    first_column: 0,
    line_count: 24,
    width: 80,
    height: 24,
};

/// Mutable editor state.
#[derive(Debug)]
pub struct EditorState {
    text: Arc<TextDocument>,
    index: Arc<LineIndex>,
    revision: u64,
    viewport: Viewport,
    selections: Vec<(usize, usize)>,
    interaction_mode: InteractionMode,
}

/// Immutable view of the editor at one revision.
#[derive(Debug, Clone)]
pub struct EditorSnapshot {
    text: Arc<TextDocument>,
    index: Arc<LineIndex>,
    revision: u64,
    viewport: Viewport,
    selections: Vec<Selection>,
    interaction_mode: InteractionMode,
}

impl EditorState {
    /// Creates state with one caret at byte zero.
    pub fn new(text: TextDocument) -> Self {
        let index = LineIndex::from_buffer(&text);
        Self {
            text: Arc::new(text),
            index: Arc::new(index),
            revision: 0,
            viewport: DEFAULT_VIEWPORT,
            selections: vec![(0, 0)],
            interaction_mode: InteractionMode::Select,
        }
    }

    /// Creates state from initial text.
    pub fn from_text(text: &str) -> Self {
        Self::new(TextDocument::from_text(text))
    }

    /// Current text buffer.
    pub fn text(&self) -> &TextDocument {
        &self.text
    }

    /// Current line index.
    pub fn index(&self) -> &LineIndex {
        &self.index
    }

    /// Current revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Current viewport.
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Replaces the viewport.
    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.viewport = viewport;
    }

    /// Current interaction mode.
    pub fn interaction_mode(&self) -> InteractionMode {
        self.interaction_mode
    }

    /// Replaces the interaction mode.
    pub fn set_interaction_mode(&mut self, mode: InteractionMode) {
        self.interaction_mode = mode;
    }

    /// Sets one selection using byte offsets.
    pub fn set_selection_bytes(&mut self, anchor: usize, head: usize) -> bool {
        if anchor > self.text.len_bytes()
            || head > self.text.len_bytes()
            || !self.text.is_char_boundary(anchor)
            || !self.text.is_char_boundary(head)
        {
            return false;
        }
        self.selections = vec![(anchor, head)];
        true
    }

    /// Current selection byte offsets.
    pub fn selection_bytes(&self) -> Vec<(usize, usize)> {
        self.selections.clone()
    }

    /// Inserts text and updates the line index and selections.
    pub fn insert(&mut self, at: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let range = at..at;
        let buffer = Arc::make_mut(&mut self.text);
        buffer.insert(at, text);
        Arc::make_mut(&mut self.index).apply_edit(buffer, range);
        self.map_selections(at, at, text.len());
        self.revision += 1;
    }

    /// Deletes a byte range and updates the line index and selections.
    pub fn delete(&mut self, range: Range<usize>) {
        if range.start == range.end {
            return;
        }
        let buffer = Arc::make_mut(&mut self.text);
        buffer.delete(range.clone());
        Arc::make_mut(&mut self.index).apply_edit(buffer, range.clone());
        self.map_selections(range.start, range.end, 0);
        self.revision += 1;
    }

    /// Builds an immutable snapshot. Text and line-index Arcs are shared.
    pub fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            text: Arc::clone(&self.text),
            index: Arc::clone(&self.index),
            revision: self.revision,
            viewport: self.viewport,
            selections: self
                .selections
                .iter()
                .map(|&(anchor, head)| Selection {
                    anchor: self.position(anchor),
                    head: self.position(head),
                })
                .collect(),
            interaction_mode: self.interaction_mode,
        }
    }

    fn position(&self, offset: usize) -> TextPosition {
        let Some(position) = self.index.position(&*self.text, offset) else {
            return TextPosition {
                byte_offset: 0,
                line: 0,
                grapheme_column: 0,
                display_column: 0,
            };
        };
        TextPosition {
            byte_offset: position.byte_offset as u64,
            line: position.line as u32,
            grapheme_column: position.grapheme_column as u32,
            display_column: position.display_column as u32,
        }
    }

    fn map_selections(&mut self, start: usize, end: usize, inserted_len: usize) {
        for (anchor, head) in &mut self.selections {
            *anchor = map_offset(*anchor, start, end, inserted_len);
            *head = map_offset(*head, start, end, inserted_len);
        }
    }
}

impl EditorSnapshot {
    /// Text at snapshot time.
    pub fn text(&self) -> &TextDocument {
        &self.text
    }

    /// Line index at snapshot time.
    pub fn index(&self) -> &LineIndex {
        &self.index
    }

    /// Revision at snapshot time.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Selects at snapshot time.
    pub fn selections(&self) -> &[Selection] {
        &self.selections
    }

    /// Builds the stable plugin view without recomputing coordinates.
    pub fn view(&self) -> EditorView {
        EditorView {
            revision: self.revision,
            interaction_mode: self.interaction_mode,
            viewport: self.viewport,
            selections: self.selections.clone(),
        }
    }
}

fn map_offset(offset: usize, start: usize, end: usize, inserted_len: usize) -> usize {
    if start == end {
        if offset >= start {
            offset + inserted_len
        } else {
            offset
        }
    } else if offset < start {
        offset
    } else if offset <= end {
        start
    } else {
        offset - (end - start) + inserted_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_projects_view() {
        let state = EditorState::from_text("a\nb");
        let snapshot = state.snapshot();
        let view = snapshot.view();
        assert_eq!(view.revision, 0);
        assert_eq!(view.interaction_mode, InteractionMode::Select);
        assert_eq!(view.viewport, DEFAULT_VIEWPORT);
        assert_eq!(view.selections.len(), 1);
        assert_eq!(view.selections[0].head.byte_offset, 0);
        assert_eq!(view.selections[0].head.line, 0);
    }

    #[test]
    fn insert_shifts_selection_and_updates_index() {
        let mut state = EditorState::from_text("abc");
        assert!(state.set_selection_bytes(1, 1));
        state.insert(0, "x\n");
        assert_eq!(state.revision(), 1);
        let (anchor, head) = state.selection_bytes()[0];
        assert_eq!((anchor, head), (3, 3));
        let view = state.snapshot().view();
        assert_eq!(view.selections[0].head.line, 1);
        assert_eq!(view.selections[0].head.grapheme_column, 1);
    }

    #[test]
    fn delete_clamps_selection_into_range() {
        let mut state = EditorState::from_text("abcdef");
        assert!(state.set_selection_bytes(1, 5));
        state.delete(0..3);
        assert_eq!(state.selection_bytes(), vec![(0, 2)]);
        assert_eq!(state.snapshot().view().selections[0].head.byte_offset, 2);
    }

    #[test]
    fn empty_snapshot_has_safe_position() {
        let state = EditorState::from_text("");
        let view = state.snapshot().view();
        assert_eq!(view.selections.len(), 1);
        assert_eq!(view.selections[0].head.byte_offset, 0);
        assert_eq!(view.selections[0].head.line, 0);
    }

    #[test]
    fn snapshot_shares_cow_storage() {
        let state = EditorState::from_text("a\nb");
        let snapshot = state.snapshot();
        assert!(Arc::ptr_eq(&state.text, &snapshot.text));
        assert!(Arc::ptr_eq(&state.index, &snapshot.index));
    }
}

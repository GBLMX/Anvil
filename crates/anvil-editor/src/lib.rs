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

/// One reversible text edit.
#[derive(Debug, Clone)]
enum EditAction {
    Insert { at: usize, text: String },
    Delete { start: usize, text: String },
    Transaction(Vec<EditAction>),
}

#[derive(Debug, Clone)]
struct EditRecord {
    before: Vec<(usize, usize)>,
    after: Vec<(usize, usize)>,
    action: EditAction,
}

/// Mutable editor state.
#[derive(Debug, Clone)]
pub struct EditorState {
    text: Arc<TextDocument>,
    index: Arc<LineIndex>,
    revision: u64,
    viewport: Viewport,
    selections: Vec<(usize, usize)>,
    interaction_mode: InteractionMode,
    undo: Vec<EditRecord>,
    redo: Vec<EditRecord>,
    yank: String,
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
        Self::with_index(text, index)
    }

    /// Creates state from initial text.
    pub fn from_text(text: &str) -> Self {
        let index = LineIndex::from_text(text);
        Self::with_index(TextDocument::from_text(text), index)
    }

    fn with_index(text: TextDocument, index: LineIndex) -> Self {
        Self {
            text: Arc::new(text),
            index: Arc::new(index),
            revision: 0,
            viewport: DEFAULT_VIEWPORT,
            selections: vec![(0, 0)],
            interaction_mode: InteractionMode::Select,
            undo: Vec::new(),
            redo: Vec::new(),
            yank: String::new(),
        }
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

    /// Replaces all selections using byte offsets.
    pub fn set_selections_bytes(&mut self, selections: Vec<(usize, usize)>) -> bool {
        let valid = selections.iter().all(|(anchor, head)| {
            *anchor <= self.text.len_bytes()
                && *head <= self.text.len_bytes()
                && self.text.is_char_boundary(*anchor)
                && self.text.is_char_boundary(*head)
        });
        if !valid {
            return false;
        }
        self.selections = selections;
        true
    }

    /// Current selection byte offsets.
    pub fn selection_bytes(&self) -> Vec<(usize, usize)> {
        self.selections.clone()
    }

    /// Byte range of the line containing `offset`, including its trailing
    /// `\n` when the document has one.
    pub fn line_range_at(&self, offset: usize) -> Option<Range<usize>> {
        let line = self.index.offset_to_line(offset)?;
        let end = if line.end < self.text.len_bytes() {
            line.end + 1
        } else {
            line.end
        };
        Some(line.start..end)
    }

    /// Next grapheme-cluster boundary after `offset`.
    pub fn next_grapheme_boundary(&self, offset: usize) -> Option<usize> {
        self.index.next_grapheme_boundary(&*self.text, offset)
    }

    /// Previous grapheme-cluster boundary before `offset`.
    pub fn previous_grapheme_boundary(&self, offset: usize) -> Option<usize> {
        self.index.previous_grapheme_boundary(&*self.text, offset)
    }

    /// Copies a byte range into the unnamed register.
    pub fn yank(&mut self, range: Range<usize>) -> bool {
        if range.start > range.end
            || range.end > self.text.len_bytes()
            || !self.text.is_char_boundary(range.start)
            || !self.text.is_char_boundary(range.end)
        {
            return false;
        }
        self.yank = self.text.slice(range);
        true
    }

    /// Current unnamed register contents.
    pub fn yank_text(&self) -> &str {
        &self.yank
    }

    /// Inserts the unnamed register at `at`, returning the inserted range.
    pub fn put(&mut self, at: usize) -> Option<Range<usize>> {
        if self.yank.is_empty() || at > self.text.len_bytes() || !self.text.is_char_boundary(at) {
            return None;
        }
        let text = std::mem::take(&mut self.yank);
        let end = at + text.len();
        self.set_selection_bytes(at, at);
        self.insert(at, &text);
        self.yank = text;
        Some(at..end)
    }

    /// Number of undo records retained.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Approximate heap bytes retained by undo records.
    pub fn undo_bytes(&self) -> usize {
        self.undo
            .iter()
            .map(|record| {
                record.before.len() * std::mem::size_of::<(usize, usize)>()
                    + record.after.len() * std::mem::size_of::<(usize, usize)>()
                    + match &record.action {
                        EditAction::Insert { text, .. } | EditAction::Delete { text, .. } => {
                            text.len()
                        }
                        EditAction::Transaction(actions) => actions
                            .iter()
                            .map(|action| match action {
                                EditAction::Insert { text, .. }
                                | EditAction::Delete { text, .. } => text.len(),
                                EditAction::Transaction(_) => 0,
                            })
                            .sum(),
                    }
            })
            .sum()
    }

    /// Inserts text and updates the line index and selections.
    pub fn insert(&mut self, at: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let before = self.selections.clone();
        self.apply_insert(at, text);
        self.map_selections(at, at, text.len());
        self.push_record(
            EditAction::Insert {
                at,
                text: text.to_owned(),
            },
            before,
        );
    }

    /// Deletes a byte range and updates the line index and selections.
    pub fn delete(&mut self, range: Range<usize>) {
        if range.start == range.end {
            return;
        }
        let text = self.text.slice(range.clone());
        let before = self.selections.clone();
        self.apply_delete(range.clone());
        self.map_selections(range.start, range.end, 0);
        self.push_record(
            EditAction::Delete {
                start: range.start,
                text,
            },
            before,
        );
    }

    /// Applies non-overlapping edits as one undoable transaction.
    ///
    /// Ranges are applied from the end of the document toward the start so
    /// later edits never shift earlier byte offsets.
    pub fn apply_transaction(&mut self, mut edits: Vec<(Range<usize>, String)>) -> bool {
        edits.retain(|(range, text)| range.start != range.end || !text.is_empty());
        if edits.is_empty() {
            return false;
        }
        for (range, _) in &edits {
            if range.start > range.end
                || range.end > self.text.len_bytes()
                || !self.text.is_char_boundary(range.start)
                || !self.text.is_char_boundary(range.end)
            {
                return false;
            }
        }
        edits.sort_by(|left, right| {
            right
                .0
                .start
                .cmp(&left.0.start)
                .then_with(|| right.0.end.cmp(&left.0.end))
        });
        for pair in edits.windows(2) {
            if pair[1].0.end > pair[0].0.start {
                return false;
            }
        }
        let before = self.selections.clone();
        let mut actions = Vec::new();
        for (range, text) in edits {
            if range.start != range.end {
                let deleted = self.text.slice(range.clone());
                self.apply_delete(range.clone());
                self.map_selections(range.start, range.end, 0);
                actions.push(EditAction::Delete {
                    start: range.start,
                    text: deleted,
                });
            }
            if !text.is_empty() {
                self.apply_insert(range.start, &text);
                self.map_selections(range.start, range.start, text.len());
                actions.push(EditAction::Insert {
                    at: range.start,
                    text,
                });
            }
        }
        self.push_record(EditAction::Transaction(actions), before);
        true
    }

    /// Replaces or inserts at every current selection as one transaction.
    ///
    /// Caret-only selections use a batch path: all inserts are applied without
    /// per-edit selection/index work, then the line index and selection offsets
    /// are rebuilt once. This avoids O(k²) selection mapping for multi-cursor
    /// typing.
    pub fn insert_at_selections(&mut self, text: &str) -> bool {
        if text.is_empty() || self.selections.is_empty() {
            return false;
        }
        if self.selections.iter().all(|(anchor, head)| anchor == head) {
            return self.insert_carets_batch(text);
        }
        let edits = self
            .selections
            .iter()
            .map(|(anchor, head)| {
                let start = (*anchor).min(*head);
                let end = (*anchor).max(*head);
                (start..end, text.to_owned())
            })
            .collect();
        self.apply_transaction(edits)
    }

    fn insert_carets_batch(&mut self, text: &str) -> bool {
        let before = self.selections.clone();
        if before.len() <= 16 {
            let edits = before
                .iter()
                .map(|(caret, _)| (*caret..*caret, text.to_owned()))
                .collect();
            return self.apply_transaction(edits);
        }
        let mut edits: Vec<(usize, String)> = before
            .iter()
            .map(|(caret, _)| (*caret, text.to_owned()))
            .collect();
        edits.sort_by(|left, right| right.0.cmp(&left.0));

        for (at, text) in &edits {
            self.apply_insert_text(*at, text);
        }
        self.index = Arc::new(LineIndex::from_buffer(&*self.text));

        let mut positions: Vec<(usize, usize)> =
            edits.iter().map(|(at, text)| (*at, text.len())).collect();
        positions.sort_by_key(|(at, _)| *at);
        let mut prefix = Vec::with_capacity(positions.len() + 1);
        prefix.push(0);
        for (_, len) in &positions {
            prefix.push(prefix.last().copied().unwrap_or(0) + len);
        }
        let map_offset = |offset: usize| {
            let index = positions.partition_point(|(at, _)| *at <= offset);
            offset + prefix[index]
        };
        self.selections = before
            .iter()
            .map(|(anchor, head)| (map_offset(*anchor), map_offset(*head)))
            .collect();

        let actions = edits
            .into_iter()
            .map(|(at, text)| EditAction::Insert { at, text })
            .collect();
        self.push_record(EditAction::Transaction(actions), before);
        true
    }

    /// Reverts the most recent text edit and restores its selection.
    pub fn undo(&mut self) -> bool {
        let Some(record) = self.undo.pop() else {
            return false;
        };
        match &record.action {
            EditAction::Insert { at, text } => self.apply_delete(*at..*at + text.len()),
            EditAction::Delete { start, text } => self.apply_insert(*start, text),
            EditAction::Transaction(actions) => {
                for action in actions.iter().rev() {
                    match action {
                        EditAction::Insert { at, text } => {
                            self.apply_delete(*at..*at + text.len());
                        }
                        EditAction::Delete { start, text } => self.apply_insert(*start, text),
                        EditAction::Transaction(_) => {}
                    }
                }
            }
        }
        self.selections = record.before.clone();
        self.redo.push(record);
        self.revision += 1;
        true
    }

    /// Reapplies the most recently undone edit and restores its selection.
    pub fn redo(&mut self) -> bool {
        let Some(record) = self.redo.pop() else {
            return false;
        };
        match &record.action {
            EditAction::Insert { at, text } => self.apply_insert(*at, text),
            EditAction::Delete { start, text } => self.apply_delete(*start..*start + text.len()),
            EditAction::Transaction(actions) => {
                for action in actions {
                    match action {
                        EditAction::Insert { at, text } => self.apply_insert(*at, text),
                        EditAction::Delete { start, text } => {
                            self.apply_delete(*start..*start + text.len());
                        }
                        EditAction::Transaction(_) => {}
                    }
                }
            }
        }
        self.selections = record.after.clone();
        self.undo.push(record);
        self.revision += 1;
        true
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

    fn apply_insert(&mut self, at: usize, text: &str) {
        self.apply_insert_text(at, text);
        Arc::make_mut(&mut self.index).apply_edit(&*self.text, at..at);
    }

    fn apply_insert_text(&mut self, at: usize, text: &str) {
        let buffer = Arc::make_mut(&mut self.text);
        buffer.insert(at, text);
    }

    fn apply_delete(&mut self, range: Range<usize>) {
        let buffer = Arc::make_mut(&mut self.text);
        buffer.delete(range.clone());
        Arc::make_mut(&mut self.index).apply_edit(buffer, range);
    }

    fn push_record(&mut self, action: EditAction, before: Vec<(usize, usize)>) {
        if let Some(last) = self.undo.last_mut()
            && let (
                EditAction::Insert {
                    at: previous_at,
                    text: previous_text,
                },
                EditAction::Insert { at, text },
            ) = (&mut last.action, &action)
            && *previous_at + previous_text.len() == *at
        {
            previous_text.push_str(text);
            last.after = self.selections.clone();
            self.redo.clear();
            self.revision += 1;
            return;
        }
        self.undo.push(EditRecord {
            before,
            after: self.selections.clone(),
            action,
        });
        self.redo.clear();
        self.revision += 1;
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
    fn transaction_replaces_all_selections_atomically() {
        let mut state = EditorState::from_text("foo foo foo");
        assert!(state.set_selections_bytes(vec![(0, 3), (4, 7), (8, 11)]));
        assert!(state.insert_at_selections("bar"));
        assert_eq!(state.text().slice(0..11), "bar bar bar");
        assert_eq!(state.undo_depth(), 1);
        assert_eq!(state.selection_bytes(), vec![(3, 3), (7, 7), (11, 11)]);
        assert!(state.undo());
        assert_eq!(state.text().slice(0..11), "foo foo foo");
        assert_eq!(state.selection_bytes(), vec![(0, 3), (4, 7), (8, 11)]);
        assert!(state.redo());
        assert_eq!(state.text().slice(0..11), "bar bar bar");
    }

    #[test]
    fn caret_batch_insert_matches_text_selection_and_undo() {
        let mut state = EditorState::from_text("abc");
        assert!(state.set_selections_bytes(vec![(1, 1), (3, 3)]));
        assert!(state.insert_at_selections("X"));
        assert_eq!(state.text().slice(0..5), "aXbcX");
        assert_eq!(state.selection_bytes(), vec![(2, 2), (5, 5)]);
        assert!(state.undo());
        assert_eq!(state.text().slice(0..3), "abc");
        assert_eq!(state.selection_bytes(), vec![(1, 1), (3, 3)]);
    }

    #[test]
    fn caret_batch_insert_matches_transaction_with_duplicates_and_multibyte() {
        const TEXT: &str = "αβγδ\nεζηθ\nικλμ\nνξοπ";
        const INSERT: &str = "Ω";
        let mut boundaries: Vec<usize> = TEXT.char_indices().map(|(at, _)| at).collect();
        boundaries.push(TEXT.len());
        let selections: Vec<(usize, usize)> = [
            18, 0, 9, 5, 12, 1, 0, 3, 10, 5, 6, 16, 2, 8, 9, 14, 11, 7, 19, 15,
        ]
        .into_iter()
        .map(|index| {
            let at = boundaries[index];
            (at, at)
        })
        .collect();
        let edits: Vec<_> = selections
            .iter()
            .map(|(at, _)| (*at..*at, INSERT.to_owned()))
            .collect();

        let mut legacy = EditorState::from_text(TEXT);
        assert!(legacy.set_selections_bytes(selections.clone()));
        assert!(legacy.apply_transaction(edits));

        let mut batched = EditorState::from_text(TEXT);
        assert!(batched.set_selections_bytes(selections.clone()));
        assert!(batched.insert_at_selections(INSERT));

        let expected_len = TEXT.len() + selections.len() * INSERT.len();
        assert_eq!(
            batched.text().slice(0..expected_len),
            legacy.text().slice(0..expected_len)
        );
        assert_eq!(batched.selection_bytes(), legacy.selection_bytes());
        assert_eq!(batched.revision(), legacy.revision());
        let expected_text = batched.text().slice(0..expected_len);
        let expected_selections = batched.selection_bytes();

        assert!(batched.undo());
        assert!(legacy.undo());
        assert_eq!(
            batched.text().slice(0..TEXT.len()),
            legacy.text().slice(0..TEXT.len())
        );
        assert_eq!(batched.selection_bytes(), selections);
        assert_eq!(legacy.selection_bytes(), selections);

        assert!(batched.redo());
        assert!(legacy.redo());
        assert_eq!(batched.text().slice(0..expected_len), expected_text);
        assert_eq!(batched.selection_bytes(), expected_selections);
        assert_eq!(legacy.text().slice(0..expected_len), expected_text);
        assert_eq!(legacy.selection_bytes(), expected_selections);
    }

    #[test]
    fn transaction_rejects_overlapping_ranges() {
        let mut state = EditorState::from_text("abcdef");
        assert!(!state.apply_transaction(vec![(0..4, "x".to_owned()), (3..6, "y".to_owned())]));
        assert_eq!(state.text().slice(0..6), "abcdef");
        assert_eq!(state.undo_depth(), 0);
    }

    #[test]
    fn undo_redo_restore_selection() {
        let mut state = EditorState::from_text("abc");
        assert!(state.set_selection_bytes(1, 1));
        state.insert(0, "x\n");
        assert_eq!(state.revision(), 1);
        assert!(state.undo());
        assert_eq!(state.text().slice(0..3), "abc");
        assert_eq!(state.selection_bytes(), vec![(1, 1)]);
        assert_eq!(state.revision(), 2);
        assert!(state.redo());
        assert_eq!(state.text().slice(0..5), "x\nabc");
        assert_eq!(state.selection_bytes(), vec![(3, 3)]);
        assert_eq!(state.revision(), 3);
    }

    #[test]
    fn delete_undo_restores_removed_text() {
        let mut state = EditorState::from_text("abcdef");
        assert!(state.set_selection_bytes(1, 4));
        state.delete(2..5);
        assert!(state.undo());
        assert_eq!(state.text().slice(0..6), "abcdef");
        assert_eq!(state.selection_bytes(), vec![(1, 4)]);
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

    #[test]
    fn line_range_includes_trailing_newline() {
        let state = EditorState::from_text("a\nb\nc");
        assert_eq!(state.line_range_at(0), Some(0..2));
        assert_eq!(state.line_range_at(2), Some(2..4));
        assert_eq!(state.line_range_at(4), Some(4..5));
    }

    #[test]
    fn unnamed_register_round_trips_and_is_undoable() {
        let mut state = EditorState::from_text("a\nb");
        assert!(state.yank(0..2));
        assert_eq!(state.yank_text(), "a\n");
        assert_eq!(state.put(3), Some(3..5));
        assert_eq!(state.text().slice(0..5), "a\nba\n");
        assert_eq!(state.selection_bytes(), vec![(5, 5)]);
        assert!(state.undo());
        assert_eq!(state.text().slice(0..3), "a\nb");
        assert_eq!(state.yank_text(), "a\n");
    }

    #[test]
    fn grapheme_boundaries_keep_zwj_clusters_whole() {
        let state = EditorState::from_text("a👨‍👩‍👧‍👦b");
        assert_eq!(state.next_grapheme_boundary(0), Some(1));
        let after_emoji = state.next_grapheme_boundary(1).expect("emoji boundary");
        assert_eq!(state.text().slice(1..after_emoji), "👨‍👩‍👧‍👦");
        assert_eq!(state.previous_grapheme_boundary(after_emoji), Some(1));
        assert_eq!(
            state.next_grapheme_boundary(after_emoji),
            Some(after_emoji + 1)
        );
    }

    #[test]
    fn grapheme_boundaries_expand_beyond_the_window() {
        let text = format!("e{}b", "\u{301}".repeat(80));
        let state = EditorState::from_text(&text);
        let after_cluster = state.next_grapheme_boundary(0).expect("cluster boundary");
        assert_eq!(after_cluster, text.len() - 1);
        assert_eq!(state.previous_grapheme_boundary(after_cluster), Some(0));
    }
}

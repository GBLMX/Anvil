//! Text buffer primitives for Anvil.
//!
//! [`PieceTable`] is the production backend selected by ADR-0009. Rope and Gap
//! Buffer remain benchmark comparisons behind [`TextBuffer`].

use std::{ops::Range, sync::Arc};

mod command;
mod line_index;
pub use command::{CommandError, EditCommand, History, Revision};
pub use line_index::{LineIndex, LinePosition, Position};
mod word;
pub use word::{end_word, next_word_start, previous_word_start};

/// A text buffer with byte-addressed edits and slicing.
///
/// Callers must pass UTF-8 code-point boundaries. Invalid offsets panic at the
/// operation that receives them; the buffer never repairs a broken boundary.
pub trait TextBuffer: Sized {
    /// Builds a buffer from initial text.
    fn from_text(text: &str) -> Self;

    /// Current length in bytes.
    fn len_bytes(&self) -> usize;

    /// Number of logical lines. Empty text has zero lines.
    fn len_lines(&self) -> usize;

    /// Whether `at` is a valid UTF-8 boundary within the buffer.
    fn is_char_boundary(&self, at: usize) -> bool;

    /// Inserts `text` at byte offset `at`.
    ///
    /// Panics if `at` is out of bounds or not a UTF-8 boundary.
    fn insert(&mut self, at: usize, text: &str);

    /// Deletes the half-open byte range.
    ///
    /// Panics if either endpoint is out of bounds or not a UTF-8 boundary.
    fn delete(&mut self, range: Range<usize>);

    /// Returns the text in the half-open byte range.
    ///
    /// Panics if either endpoint is out of bounds or not a UTF-8 boundary.
    fn slice(&self, range: Range<usize>) -> String;
}

fn line_count(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        0
    } else {
        bytes.iter().filter(|byte| **byte == b'\n').count() + 1
    }
}

/// Rope-backed candidate from the Phase 0 text-model benchmark.
#[derive(Debug, Clone)]
pub struct RopeBuffer {
    rope: ropey::Rope,
    line_count: usize,
}

impl RopeBuffer {
    fn assert_boundary(&self, at: usize) {
        assert!(at <= self.len_bytes(), "offset {at} is out of bounds");
        if at < self.len_bytes() {
            let byte = self.rope.get_byte(at).expect("offset checked");
            assert!((byte & 0xC0) != 0x80, "offset {at} is not a UTF-8 boundary");
        }
    }
}

impl TextBuffer for RopeBuffer {
    fn from_text(text: &str) -> Self {
        Self {
            line_count: line_count(text.as_bytes()),
            rope: ropey::Rope::from_str(text),
        }
    }

    fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    fn len_lines(&self) -> usize {
        self.line_count
    }

    fn is_char_boundary(&self, at: usize) -> bool {
        at <= self.len_bytes()
            && (at == self.len_bytes()
                || self
                    .rope
                    .get_byte(at)
                    .is_some_and(|byte| (byte & 0xC0) != 0x80))
    }

    fn insert(&mut self, at: usize, text: &str) {
        self.assert_boundary(at);
        if text.is_empty() {
            return;
        }
        let inserted_lines = text.bytes().filter(|byte| *byte == b'\n').count();
        if self.len_bytes() == 0 {
            self.line_count = inserted_lines + 1;
        } else {
            self.line_count += inserted_lines;
        }
        let index = self.rope.byte_to_char(at);
        self.rope.insert(index, text);
    }

    fn delete(&mut self, range: Range<usize>) {
        assert!(range.start <= range.end && range.end <= self.len_bytes());
        self.assert_boundary(range.start);
        self.assert_boundary(range.end);
        if range.start == range.end {
            return;
        }
        let start = self.rope.byte_to_char(range.start);
        let end = self.rope.byte_to_char(range.end);
        let removed_lines = self
            .rope
            .slice(start..end)
            .chars()
            .filter(|c| *c == '\n')
            .count();
        self.rope.remove(start..end);
        if self.len_bytes() == 0 {
            self.line_count = 0;
        } else {
            self.line_count -= removed_lines;
        }
    }

    fn slice(&self, range: Range<usize>) -> String {
        assert!(range.start <= range.end && range.end <= self.len_bytes());
        self.assert_boundary(range.start);
        self.assert_boundary(range.end);
        let start = self.rope.byte_to_char(range.start);
        let end = self.rope.byte_to_char(range.end);
        self.rope.slice(start..end).to_string()
    }
}

/// Gap-buffer candidate from the Phase 0 text-model benchmark.
#[derive(Debug, Clone)]
pub struct GapBuffer {
    storage: Vec<u8>,
    gap_start: usize,
    gap_end: usize,
    line_count: usize,
}

impl GapBuffer {
    fn len(&self) -> usize {
        self.gap_start + self.storage.len() - self.gap_end
    }

    fn move_gap(&mut self, at: usize) {
        assert!(at <= self.len());
        if at < self.gap_start {
            let len = self.gap_start - at;
            self.storage
                .copy_within(at..self.gap_start, self.gap_end - len);
            self.gap_start = at;
            self.gap_end -= len;
        } else if at > self.gap_start {
            let len = at - self.gap_start;
            self.storage
                .copy_within(self.gap_end..self.gap_end + len, self.gap_start);
            self.gap_start += len;
            self.gap_end += len;
        }
    }

    fn ensure_gap(&mut self, needed: usize) {
        if self.gap_end - self.gap_start >= needed {
            return;
        }

        let suffix_len = self.storage.len() - self.gap_end;
        let capacity = (self.storage.len() + needed).next_power_of_two().max(16);
        let mut storage = vec![0; capacity];
        storage[..self.gap_start].copy_from_slice(&self.storage[..self.gap_start]);
        let new_gap_end = capacity - suffix_len;
        storage[new_gap_end..].copy_from_slice(&self.storage[self.gap_end..]);
        self.storage = storage;
        self.gap_end = new_gap_end;
    }

    fn byte_at(&self, at: usize) -> Option<u8> {
        if at < self.gap_start {
            Some(self.storage[at])
        } else if at < self.len() {
            Some(self.storage[self.gap_end + at - self.gap_start])
        } else {
            None
        }
    }

    fn assert_boundary(&self, at: usize) {
        assert!(at <= self.len(), "offset {at} is out of bounds");
        if let Some(byte) = self.byte_at(at) {
            assert!((byte & 0xC0) != 0x80, "offset {at} is not a UTF-8 boundary");
        }
    }
}

impl TextBuffer for GapBuffer {
    fn from_text(text: &str) -> Self {
        let capacity = (text.len().saturating_mul(2)).max(16);
        let mut storage = vec![0; capacity];
        storage[..text.len()].copy_from_slice(text.as_bytes());
        Self {
            storage,
            gap_start: text.len(),
            gap_end: capacity,
            line_count: line_count(text.as_bytes()),
        }
    }

    fn len_bytes(&self) -> usize {
        self.len()
    }

    fn len_lines(&self) -> usize {
        self.line_count
    }

    fn is_char_boundary(&self, at: usize) -> bool {
        at <= self.len()
            && (at == self.len() || self.byte_at(at).is_some_and(|byte| (byte & 0xC0) != 0x80))
    }

    fn insert(&mut self, at: usize, text: &str) {
        self.assert_boundary(at);
        if text.is_empty() {
            return;
        }
        let inserted_lines = text.bytes().filter(|byte| *byte == b'\n').count();
        if self.len() == 0 {
            self.line_count = inserted_lines + 1;
        } else {
            self.line_count += inserted_lines;
        }
        self.move_gap(at);
        self.ensure_gap(text.len());
        self.storage[self.gap_start..self.gap_start + text.len()].copy_from_slice(text.as_bytes());
        self.gap_start += text.len();
    }

    fn delete(&mut self, range: Range<usize>) {
        assert!(range.start <= range.end && range.end <= self.len());
        self.assert_boundary(range.start);
        self.assert_boundary(range.end);
        if range.start == range.end {
            return;
        }
        self.move_gap(range.start);
        let removed = range.end - range.start;
        let removed_lines = self.storage[self.gap_end..self.gap_end + removed]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count();
        self.gap_end += removed;
        if self.len() == 0 {
            self.line_count = 0;
        } else {
            self.line_count -= removed_lines;
        }
    }

    fn slice(&self, range: Range<usize>) -> String {
        assert!(range.start <= range.end && range.end <= self.len());
        self.assert_boundary(range.start);
        self.assert_boundary(range.end);
        let mut bytes = Vec::with_capacity(range.end - range.start);
        if range.start < self.gap_start {
            let end = range.end.min(self.gap_start);
            bytes.extend_from_slice(&self.storage[range.start..end]);
        }
        if range.end > self.gap_start {
            let logical_start = range.start.max(self.gap_start);
            let storage_start = self.gap_end + (logical_start - self.gap_start);
            let storage_end = self.gap_end + (range.end - self.gap_start);
            bytes.extend_from_slice(&self.storage[storage_start..storage_end]);
        }
        String::from_utf8(bytes).expect("buffer contains valid UTF-8")
    }
}

/// Piece-table candidate from the Phase 0 text-model benchmark.
///
/// Text lives in shared `Arc<str>` chunks. Cloning the table, including the
/// `Arc::make_mut` clone after a retained snapshot, copies only the piece and
/// chunk indexes, never the document text.
#[derive(Debug, Clone)]
pub struct PieceTable {
    original: Arc<str>,
    added: Vec<Arc<str>>,
    pieces: Vec<Piece>,
    line_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Original,
    Added(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Piece {
    source: Source,
    start: usize,
    len: usize,
}

impl PieceTable {
    fn text(&self, source: Source) -> &str {
        match source {
            Source::Original => &self.original,
            Source::Added(index) => &self.added[index],
        }
    }

    fn split_at(&mut self, at: usize) -> usize {
        let mut position = 0;
        for (index, piece) in self.pieces.iter().enumerate() {
            if at == position {
                return index;
            }
            if at < position + piece.len {
                let offset = at - position;
                let left = Piece {
                    len: offset,
                    ..*piece
                };
                let right = Piece {
                    source: piece.source,
                    start: piece.start + offset,
                    len: piece.len - offset,
                };
                self.pieces[index] = left;
                self.pieces.insert(index + 1, right);
                return index + 1;
            }
            position += piece.len;
        }
        self.pieces.len()
    }

    fn byte_at(&self, at: usize) -> Option<u8> {
        let mut position = 0;
        for piece in &self.pieces {
            if at < position + piece.len {
                let offset = at - position;
                return Some(self.text(piece.source).as_bytes()[piece.start + offset]);
            }
            position += piece.len;
        }
        None
    }

    fn assert_boundary(&self, at: usize) {
        assert!(at <= self.len_bytes(), "offset {at} is out of bounds");
        if let Some(byte) = self.byte_at(at) {
            assert!((byte & 0xC0) != 0x80, "offset {at} is not a UTF-8 boundary");
        }
    }

    fn count_newlines_in(&self, range: Range<usize>) -> usize {
        let mut position = 0;
        let mut count = 0;
        for piece in &self.pieces {
            let piece_end = position + piece.len;
            if piece_end <= range.start {
                position = piece_end;
                continue;
            }
            if position >= range.end {
                break;
            }
            let local_start = range.start.max(position) - position;
            let local_end = range.end.min(piece_end) - position;
            count += self.text(piece.source)[piece.start + local_start..piece.start + local_end]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count();
            position = piece_end;
        }
        count
    }
    /// Returns borrowed text chunks intersecting `range` without copying the
    /// document text. This is the renderer-facing fast path.
    pub fn chunks(&self, range: Range<usize>) -> impl Iterator<Item = &str> + '_ {
        assert!(range.start <= range.end && range.end <= self.len_bytes());
        self.assert_boundary(range.start);
        self.assert_boundary(range.end);
        let start = range.start;
        let end = range.end;
        let mut position = 0;
        self.pieces.iter().filter_map(move |piece| {
            let piece_start = position;
            let piece_end = position + piece.len;
            position = piece_end;
            if piece_end <= start || piece_start >= end {
                return None;
            }
            let local_start = start.max(piece_start) - piece_start;
            let local_end = end.min(piece_end) - piece_start;
            Some(&self.text(piece.source)[piece.start + local_start..piece.start + local_end])
        })
    }
}

impl TextBuffer for PieceTable {
    fn from_text(text: &str) -> Self {
        let pieces = if text.is_empty() {
            Vec::new()
        } else {
            vec![Piece {
                source: Source::Original,
                start: 0,
                len: text.len(),
            }]
        };
        Self {
            original: Arc::from(text),
            added: Vec::new(),
            pieces,
            line_count: line_count(text.as_bytes()),
        }
    }

    fn len_bytes(&self) -> usize {
        self.pieces.iter().map(|piece| piece.len).sum()
    }

    fn len_lines(&self) -> usize {
        self.line_count
    }

    fn is_char_boundary(&self, at: usize) -> bool {
        at <= self.len_bytes()
            && (at == self.len_bytes()
                || self.byte_at(at).is_some_and(|byte| (byte & 0xC0) != 0x80))
    }

    fn insert(&mut self, at: usize, text: &str) {
        self.assert_boundary(at);
        if text.is_empty() {
            return;
        }
        let inserted_lines = text.bytes().filter(|byte| *byte == b'\n').count();
        if self.len_bytes() == 0 {
            self.line_count = inserted_lines + 1;
        } else {
            self.line_count += inserted_lines;
        }
        let index = self.split_at(at);
        let chunk = Arc::<str>::from(text);
        let source = Source::Added(self.added.len());
        self.added.push(chunk);
        self.pieces.insert(
            index,
            Piece {
                source,
                start: 0,
                len: text.len(),
            },
        );
    }

    fn delete(&mut self, range: Range<usize>) {
        assert!(range.start <= range.end && range.end <= self.len_bytes());
        self.assert_boundary(range.start);
        self.assert_boundary(range.end);
        if range.start == range.end {
            return;
        }
        let removed_lines = self.count_newlines_in(range.clone());
        let start = self.split_at(range.start);
        let end = self.split_at(range.end);
        self.pieces.drain(start..end);
        if self.len_bytes() == 0 {
            self.line_count = 0;
        } else {
            self.line_count -= removed_lines;
        }
    }

    fn slice(&self, range: Range<usize>) -> String {
        assert!(range.start <= range.end && range.end <= self.len_bytes());
        self.assert_boundary(range.start);
        self.assert_boundary(range.end);
        let mut result = String::with_capacity(range.end - range.start);
        let mut position = 0;
        for piece in &self.pieces {
            let piece_end = position + piece.len;
            if piece_end <= range.start {
                position = piece_end;
                continue;
            }
            if position >= range.end {
                break;
            }
            let local_start = range.start.max(position) - position;
            let local_end = range.end.min(piece_end) - position;
            let text = self.text(piece.source);
            result.push_str(&text[piece.start + local_start..piece.start + local_end]);
            position = piece_end;
        }
        result
    }
}

/// Production text buffer selected by ADR-0009.
pub type TextDocument = PieceTable;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn exercise<B: TextBuffer>() {
        let mut buffer = B::from_text("alpha\nβeta");
        assert_eq!(buffer.len_bytes(), 11);
        assert_eq!(buffer.len_lines(), 2);
        assert_eq!(buffer.slice(0..5), "alpha");

        buffer.insert(6, "X");
        assert_eq!(buffer.slice(0..7), "alpha\nX");
        assert_eq!(buffer.len_lines(), 2);

        buffer.delete(0..6);
        assert_eq!(buffer.slice(0..6), "Xβeta");
        assert_eq!(buffer.len_lines(), 1);
    }

    #[test]
    fn rope_candidate_works() {
        exercise::<RopeBuffer>();
    }

    #[test]
    fn gap_candidate_works() {
        exercise::<GapBuffer>();
    }

    #[test]
    fn piece_candidate_works() {
        exercise::<PieceTable>();
    }

    #[test]
    fn empty_buffers_have_no_lines() {
        assert_eq!(RopeBuffer::from_text("").len_lines(), 0);
        assert_eq!(GapBuffer::from_text("").len_lines(), 0);
        assert_eq!(PieceTable::from_text("").len_lines(), 0);
    }

    #[derive(Debug, Clone)]
    enum Op {
        Insert { at: usize, text: String },
        Delete { start: usize, end: usize },
    }

    fn text_strategy() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop::sample::select(vec!['a', 'b', '\n', 'é', 'β', '🦀']),
            0..8,
        )
        .prop_map(|chars| chars.into_iter().collect())
    }

    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0usize..64, text_strategy()).prop_map(|(at, text)| Op::Insert { at, text }),
            (0usize..64, 0usize..64).prop_map(|(a, b)| Op::Delete {
                start: a.min(b),
                end: a.max(b),
            }),
        ]
    }

    fn normalize_boundary(text: &str, at: usize) -> usize {
        let mut at = at.min(text.len());
        while at > 0 && !text.is_char_boundary(at) {
            at -= 1;
        }
        at
    }

    fn model_lines(text: &str) -> usize {
        if text.is_empty() {
            0
        } else {
            text.bytes().filter(|byte| *byte == b'\n').count() + 1
        }
    }

    fn check_ops<B: TextBuffer>(ops: &[Op]) {
        let mut buffer = B::from_text("seed\n");
        let mut model = String::from("seed\n");
        for op in ops {
            match op {
                Op::Insert { at, text } => {
                    let at = normalize_boundary(&model, *at);
                    model.insert_str(at, text);
                    buffer.insert(at, text);
                }
                Op::Delete { start, end } => {
                    let start = normalize_boundary(&model, *start);
                    let end = normalize_boundary(&model, *end).max(start);
                    model.replace_range(start..end, "");
                    buffer.delete(start..end);
                }
            }
            assert_eq!(buffer.len_bytes(), model.len());
            assert_eq!(buffer.len_lines(), model_lines(&model));
            assert_eq!(buffer.slice(0..buffer.len_bytes()), model);
        }
    }

    proptest! {
        #[test]
        fn rope_matches_string_model(ops in prop::collection::vec(op_strategy(), 0..40)) {
            check_ops::<RopeBuffer>(&ops);
        }

        #[test]
        fn gap_matches_string_model(ops in prop::collection::vec(op_strategy(), 0..40)) {
            check_ops::<GapBuffer>(&ops);
        }

        #[test]
        fn piece_matches_string_model(ops in prop::collection::vec(op_strategy(), 0..40)) {
            check_ops::<PieceTable>(&ops);
        }
    }

    #[test]
    fn piece_chunks_borrow_across_piece_boundaries() {
        let mut buffer = PieceTable::from_text("abcd");
        buffer.insert(2, "XY");
        let chunks: Vec<&str> = buffer.chunks(0..buffer.len_bytes()).collect();
        assert_eq!(chunks, vec!["ab", "XY", "cd"]);
    }

    #[test]
    fn text_document_uses_piece_table() {
        let mut text = TextDocument::from_text("a");
        text.insert(1, "b");
        assert_eq!(text.slice(0..2), "ab");
    }

    #[test]
    fn line_counts_follow_edits() {
        fn check<B: TextBuffer>() {
            let mut buffer = B::from_text("a");
            assert_eq!(buffer.len_lines(), 1);
            buffer.insert(1, "\n");
            assert_eq!(buffer.len_lines(), 2);
            buffer.insert(2, "b");
            assert_eq!(buffer.len_lines(), 2);
            buffer.delete(1..2);
            assert_eq!(buffer.len_lines(), 1);
            buffer.delete(0..buffer.len_bytes());
            assert_eq!(buffer.len_lines(), 0);
        }
        check::<RopeBuffer>();
        check::<GapBuffer>();
        check::<PieceTable>();
    }

    #[test]
    fn piece_snapshot_shares_text_chunks() {
        let mut buffer = PieceTable::from_text("alpha");
        let original = buffer.original.clone();
        buffer.insert(5, " beta");
        let added = buffer.added[0].clone();
        let snapshot = buffer.clone();
        assert!(Arc::ptr_eq(&original, &snapshot.original));
        assert!(Arc::ptr_eq(&added, &snapshot.added[0]));
        buffer.insert(0, "x");
        assert!(Arc::ptr_eq(&original, &buffer.original));
        assert!(Arc::ptr_eq(&added, &buffer.added[0]));
    }

    #[test]
    #[should_panic]
    fn rope_rejects_mid_codepoint_insert() {
        RopeBuffer::from_text("é").insert(1, "x");
    }

    #[test]
    #[should_panic(expected = "not a UTF-8 boundary")]
    fn gap_rejects_mid_codepoint_insert() {
        GapBuffer::from_text("é").insert(1, "x");
    }

    #[test]
    #[should_panic(expected = "not a UTF-8 boundary")]
    fn piece_rejects_mid_codepoint_insert() {
        PieceTable::from_text("é").insert(1, "x");
    }

    #[test]
    #[should_panic]
    fn rope_rejects_mid_codepoint_delete() {
        RopeBuffer::from_text("é").delete(0..1);
    }

    #[test]
    #[should_panic(expected = "not a UTF-8 boundary")]
    fn gap_rejects_mid_codepoint_delete() {
        GapBuffer::from_text("é").delete(0..1);
    }

    #[test]
    #[should_panic(expected = "not a UTF-8 boundary")]
    fn piece_rejects_mid_codepoint_delete() {
        PieceTable::from_text("é").delete(0..1);
    }
}

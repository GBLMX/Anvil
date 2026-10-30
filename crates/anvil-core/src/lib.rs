//! Text buffer primitives for Anvil.
//!
//! The Phase 0 benchmark compares three candidates behind [`TextBuffer`]. Only
//! the winner becomes the production backend; the others are benchmark-only.

use std::{ops::Range, string::String};

/// A text buffer with byte-addressed edits and slicing.
///
/// Callers must pass UTF-8 code-point boundaries. The buffer validates slices
/// but does not repair invalid boundaries.
pub trait TextBuffer: Sized {
    /// Builds a buffer from initial text.
    fn from_text(text: &str) -> Self;

    /// Current length in bytes.
    fn len_bytes(&self) -> usize;

    /// Number of logical lines. Empty text has zero lines.
    fn len_lines(&self) -> usize;

    /// Inserts `text` at byte offset `at`.
    fn insert(&mut self, at: usize, text: &str);

    /// Deletes the half-open byte range.
    fn delete(&mut self, range: Range<usize>);

    /// Returns the text in the half-open byte range.
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
}

impl TextBuffer for RopeBuffer {
    fn from_text(text: &str) -> Self {
        Self {
            rope: ropey::Rope::from_str(text),
        }
    }

    fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    fn len_lines(&self) -> usize {
        if self.rope.len_bytes() == 0 {
            0
        } else {
            self.rope.len_lines()
        }
    }

    fn insert(&mut self, at: usize, text: &str) {
        let index = self.rope.byte_to_char(at);
        self.rope.insert(index, text);
    }

    fn delete(&mut self, range: Range<usize>) {
        let start = self.rope.byte_to_char(range.start);
        let end = self.rope.byte_to_char(range.end);
        self.rope.remove(start..end);
    }

    fn slice(&self, range: Range<usize>) -> String {
        assert!(range.start <= range.end && range.end <= self.len_bytes());
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
        }
    }

    fn len_bytes(&self) -> usize {
        self.len()
    }

    fn len_lines(&self) -> usize {
        let mut bytes = Vec::with_capacity(self.len());
        bytes.extend_from_slice(&self.storage[..self.gap_start]);
        bytes.extend_from_slice(&self.storage[self.gap_end..]);
        line_count(&bytes)
    }

    fn insert(&mut self, at: usize, text: &str) {
        self.move_gap(at);
        self.ensure_gap(text.len());
        self.storage[self.gap_start..self.gap_start + text.len()].copy_from_slice(text.as_bytes());
        self.gap_start += text.len();
    }

    fn delete(&mut self, range: Range<usize>) {
        assert!(range.start <= range.end && range.end <= self.len());
        if range.start == range.end {
            return;
        }
        self.move_gap(range.start);
        let removed = range.end - range.start;
        let tail_start = self.gap_end + removed;
        self.storage.copy_within(tail_start.., self.gap_end);
        self.gap_end += removed;
    }

    fn slice(&self, range: Range<usize>) -> String {
        assert!(range.start <= range.end && range.end <= self.len());
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
#[derive(Debug, Clone)]
pub struct PieceTable {
    original: String,
    added: String,
    pieces: Vec<Piece>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Original,
    Added,
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
            Source::Added => &self.added,
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

    fn count_newlines(&self) -> usize {
        self.pieces
            .iter()
            .map(|piece| {
                self.text(piece.source)[piece.start..piece.start + piece.len]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
            })
            .sum()
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
            original: text.to_owned(),
            added: String::new(),
            pieces,
        }
    }

    fn len_bytes(&self) -> usize {
        self.pieces.iter().map(|piece| piece.len).sum()
    }

    fn len_lines(&self) -> usize {
        if self.len_bytes() == 0 {
            0
        } else {
            self.count_newlines() + 1
        }
    }

    fn insert(&mut self, at: usize, text: &str) {
        let index = self.split_at(at);
        let start = self.added.len();
        self.added.push_str(text);
        self.pieces.insert(
            index,
            Piece {
                source: Source::Added,
                start,
                len: text.len(),
            },
        );
    }

    fn delete(&mut self, range: Range<usize>) {
        assert!(range.start <= range.end && range.end <= self.len_bytes());
        if range.start == range.end {
            return;
        }
        let start = self.split_at(range.start);
        let end = self.split_at(range.end);
        self.pieces.drain(start..end);
    }

    fn slice(&self, range: Range<usize>) -> String {
        assert!(range.start <= range.end && range.end <= self.len_bytes());
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

#[cfg(test)]
mod tests {
    use super::*;

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
}

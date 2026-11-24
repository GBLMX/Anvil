//! Benchmark-only text backends for the ADR-0009 comparison.
//!
//! `PieceTable` remains the production backend in `anvil-core`. The Rope and
//! Gap Buffer implementations live here so they stay out of the production
//! kernel while the comparison harness remains reproducible.

use std::ops::Range;

use anvil_core::TextBuffer;

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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn empty_buffers_have_no_lines() {
        assert_eq!(RopeBuffer::from_text("").len_lines(), 0);
        assert_eq!(GapBuffer::from_text("").len_lines(), 0);
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
    }

    #[test]
    #[should_panic(expected = "not a UTF-8 boundary")]
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
    fn rope_rejects_mid_codepoint_delete() {
        RopeBuffer::from_text("é").delete(0..1);
    }

    #[test]
    #[should_panic(expected = "not a UTF-8 boundary")]
    fn gap_rejects_mid_codepoint_delete() {
        GapBuffer::from_text("é").delete(0..1);
    }
}

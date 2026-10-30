//! Chunked line index and cursor coordinate conversion.
//!
//! The index stores line starts in persistent chunks. A snapshot can share the
//! chunk vector and every line-start slice; an edit copies only the chunk
//! metadata and the chunks it actually rebuilds.

use std::{ops::Range, sync::Arc};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

use crate::TextBuffer;

const TARGET_LINES: usize = 1024;
const MAX_LINES: usize = 2048;
const MIN_LINES: usize = 512;
const TAB_WIDTH: usize = 4;

#[derive(Debug, Clone)]
struct LineChunk {
    base: usize,
    first_line: usize,
    end: usize,
    starts: Arc<[usize]>,
}

/// A byte range belonging to one logical line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinePosition {
    /// Zero-based line number.
    pub line: usize,
    /// Byte offset where the line starts.
    pub start: usize,
    /// Byte offset where the line content ends, excluding the final `\n`.
    pub end: usize,
}

/// A cursor position with all coordinate representations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// UTF-8 byte offset.
    pub byte_offset: usize,
    /// Zero-based line number.
    pub line: usize,
    /// Zero-based grapheme-cluster column.
    pub grapheme_column: usize,
    /// Zero-based terminal display-cell column.
    pub display_column: usize,
}

/// Chunked line index. Empty text has no chunks and zero lines.
#[derive(Debug, Clone)]
pub struct LineIndex {
    len: usize,
    chunks: Vec<LineChunk>,
}

impl LineIndex {
    /// Builds an index from initial text.
    pub fn from_text(text: &str) -> Self {
        let starts = line_starts(text);
        Self {
            len: text.len(),
            chunks: build_chunks(&starts, text.len(), 0),
        }
    }

    /// Builds an index from the current buffer.
    pub fn from_buffer<B: TextBuffer>(buffer: &B) -> Self {
        Self::from_text(&buffer.slice(0..buffer.len_bytes()))
    }

    /// Current byte length.
    pub fn len_bytes(&self) -> usize {
        self.len
    }

    /// Number of persistent chunks currently held.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Number of line starts in each chunk.
    pub fn chunk_sizes(&self) -> Vec<usize> {
        self.chunks.iter().map(|chunk| chunk.starts.len()).collect()
    }

    /// Asserts the chunk occupancy invariant used by the design.
    pub fn assert_chunk_bounds(&self) {
        for (index, chunk) in self.chunks.iter().enumerate() {
            let size = chunk.starts.len();
            assert!(size <= MAX_LINES, "chunk {index} has {size} lines");
            if index + 1 < self.chunks.len() {
                assert!(size >= MIN_LINES, "chunk {index} has only {size} lines");
            }
        }
    }

    /// Number of logical lines. Empty text has zero lines.
    pub fn len_lines(&self) -> usize {
        self.chunks
            .last()
            .map_or(0, |chunk| chunk.first_line + chunk.starts.len())
    }

    /// Maps a byte offset to a line and line bounds.
    ///
    /// Returns `None` for an out-of-range offset or any offset in empty text.
    pub fn offset_to_line(&self, offset: usize) -> Option<LinePosition> {
        if offset > self.len || self.chunks.is_empty() {
            return None;
        }
        let chunk_index = self
            .chunks
            .partition_point(|chunk| chunk.base <= offset)
            .saturating_sub(1);
        let chunk = &self.chunks[chunk_index];
        let local = chunk
            .starts
            .partition_point(|start| chunk.base + start <= offset)
            .saturating_sub(1);
        let start = chunk.base + chunk.starts[local];
        let next_start = chunk
            .starts
            .get(local + 1)
            .map(|start| chunk.base + start)
            .unwrap_or(chunk.end);
        let has_next_line = local + 1 < chunk.starts.len() || chunk_index + 1 < self.chunks.len();
        let end = if has_next_line && next_start <= self.len {
            next_start - 1
        } else {
            self.len
        };
        Some(LinePosition {
            line: chunk.first_line + local,
            start,
            end,
        })
    }

    /// Maps a zero-based line to its byte offset.
    pub fn line_to_offset(&self, line: usize) -> Option<usize> {
        if self.chunks.is_empty() || line >= self.len_lines() {
            return None;
        }
        let chunk_index = self
            .chunks
            .partition_point(|chunk| chunk.first_line <= line)
            .saturating_sub(1);
        let chunk = &self.chunks[chunk_index];
        let local = line - chunk.first_line;
        chunk.starts.get(local).map(|start| chunk.base + start)
    }

    /// Returns the cursor coordinates for a byte offset.
    pub fn position<B: TextBuffer>(&self, buffer: &B, offset: usize) -> Option<Position> {
        if offset > buffer.len_bytes() || !buffer.is_char_boundary(offset) || self.chunks.is_empty()
        {
            return None;
        }
        let line = self.offset_to_line(offset)?;
        let prefix = buffer.slice(line.start..offset);
        let grapheme_column = prefix.graphemes(true).count();
        let display_column = display_width(&prefix);
        Some(Position {
            byte_offset: offset,
            line: line.line,
            grapheme_column,
            display_column,
        })
    }

    /// Returns the byte offset at a grapheme column.
    ///
    /// Columns beyond the line content clamp to the line content end.
    pub fn offset_at_grapheme<B: TextBuffer>(
        &self,
        buffer: &B,
        line: usize,
        column: usize,
    ) -> Option<usize> {
        let start = self.line_to_offset(line)?;
        let line = self.offset_to_line(start)?;
        let content_end = line.end;
        let text = buffer.slice(start..content_end);
        for (index, (byte, _)) in text.grapheme_indices(true).enumerate() {
            if index == column {
                return Some(start + byte);
            }
        }
        Some(content_end)
    }

    /// Returns the byte offset at a terminal display column.
    ///
    /// A column inside a wide character clamps to that character's start.
    pub fn offset_at_display_column<B: TextBuffer>(
        &self,
        buffer: &B,
        line: usize,
        column: usize,
    ) -> Option<usize> {
        let start = self.line_to_offset(line)?;
        let line = self.offset_to_line(start)?;
        let content_end = line.end;
        let text = buffer.slice(start..content_end);
        let mut width = 0;
        for (byte, character) in text.char_indices() {
            if width >= column {
                return Some(start + byte);
            }
            width += display_char_width(character, width);
        }
        Some(content_end)
    }

    /// Applies an edit after `buffer` has already been mutated.
    ///
    /// `range` is the old byte range replaced by the edit.
    pub fn apply_edit<B: TextBuffer>(&mut self, buffer: &B, range: Range<usize>) {
        let new_len = buffer.len_bytes();
        assert!(range.start <= range.end && range.end <= self.len);
        if new_len == 0 || self.chunks.is_empty() {
            *self = Self::from_buffer(buffer);
            return;
        }
        if range.start == range.end && new_len == self.len {
            return;
        }

        let old_len = self.len;
        let delta = new_len as isize - old_len as isize;
        let start_line = self
            .offset_to_line(range.start)
            .map_or(0, |line| line.line.saturating_sub(1));
        let end_line = self
            .offset_to_line(range.end)
            .map_or(self.len_lines().saturating_sub(1), |line| {
                (line.line + 1).min(self.len_lines().saturating_sub(1))
            });
        let first_chunk = self.chunk_for_line(start_line);
        let last_chunk = self.chunk_for_line(end_line);
        let region_start = self.chunks[first_chunk].base;
        let old_region_end = self.chunks[last_chunk].end;
        let new_region_end = add_delta(old_region_end, delta).min(new_len);
        let old_region_lines = self.chunks[last_chunk].first_line
            + self.chunks[last_chunk].starts.len()
            - self.chunks[first_chunk].first_line;
        let slice = buffer.slice(region_start..new_region_end);
        let mut local_starts = line_starts(&slice);
        if last_chunk + 1 < self.chunks.len() && local_starts.last() == Some(&slice.len()) {
            local_starts.pop();
        }
        let new_region_lines = local_starts.len();
        let line_delta = new_region_lines as isize - old_region_lines as isize;

        for chunk in &mut self.chunks[last_chunk + 1..] {
            chunk.base = add_delta(chunk.base, delta);
            chunk.end = add_delta(chunk.end, delta);
            chunk.first_line = add_delta(chunk.first_line, line_delta);
        }

        let absolute_starts: Vec<usize> = local_starts
            .into_iter()
            .map(|start| region_start + start)
            .collect();
        let middle = build_chunks(
            &absolute_starts,
            new_region_end,
            self.chunks[first_chunk].first_line,
        );
        let middle_needs_normalize = middle.iter().any(|chunk| chunk.starts.len() < MIN_LINES);
        self.chunks.splice(first_chunk..=last_chunk, middle);
        self.len = new_len;
        if line_delta != 0 || middle_needs_normalize {
            let needs_normalize = self.chunks.iter().enumerate().any(|(index, chunk)| {
                chunk.starts.len() > MAX_LINES
                    || (index + 1 < self.chunks.len() && chunk.starts.len() < MIN_LINES)
            });
            if needs_normalize {
                self.normalize();
            }
        }
    }

    fn chunk_for_line(&self, line: usize) -> usize {
        self.chunks
            .partition_point(|chunk| chunk.first_line <= line)
            .saturating_sub(1)
    }

    fn normalize(&mut self) {
        let mut split = Vec::new();
        for chunk in std::mem::take(&mut self.chunks) {
            if chunk.starts.len() <= MAX_LINES {
                split.push(chunk);
            } else {
                for (part_index, part) in chunk.starts.chunks(TARGET_LINES).enumerate() {
                    let base = chunk.base + part[0];
                    let end = chunk
                        .starts
                        .get((part_index + 1) * TARGET_LINES)
                        .map(|start| chunk.base + start)
                        .unwrap_or(chunk.end);
                    let starts: Vec<usize> = part.iter().map(|start| start - part[0]).collect();
                    split.push(LineChunk {
                        base,
                        first_line: chunk.first_line,
                        end,
                        starts: Arc::from(starts),
                    });
                }
                if let Some(last) = split.last_mut() {
                    last.end = chunk.end;
                }
            }
        }
        self.chunks = split;
        self.recompute_first_lines();

        let mut index = 0;
        while index < self.chunks.len() {
            if self.chunks[index].starts.len() >= MIN_LINES {
                index += 1;
                continue;
            }
            if self.chunks.len() == 1 {
                break;
            }
            let (left, right) = if index == 0 {
                (0, 1)
            } else if index + 1 == self.chunks.len() {
                (index - 1, index)
            } else {
                let left_len = self.chunks[index - 1].starts.len();
                let right_len = self.chunks[index + 1].starts.len();
                if left_len <= right_len {
                    (index - 1, index)
                } else {
                    (index, index + 1)
                }
            };
            let total = self.chunks[left].starts.len() + self.chunks[right].starts.len();
            let mut starts = absolute_starts(&self.chunks[left]);
            starts.extend(absolute_starts(&self.chunks[right]));
            let split_at = if total <= MAX_LINES { total } else { total / 2 };
            let left_base = self.chunks[left].base;
            let right_end = self.chunks[right].end;
            let left_first_line = self.chunks[left].first_line;
            self.chunks[left].starts = Arc::from(
                starts[..split_at]
                    .iter()
                    .map(|start| start - left_base)
                    .collect::<Vec<_>>(),
            );
            self.chunks[left].end = if split_at == total {
                right_end
            } else {
                starts[split_at]
            };
            if split_at == total {
                self.chunks.remove(right);
            } else {
                self.chunks[right].base = starts[split_at];
                self.chunks[right].first_line = left_first_line + split_at;
                self.chunks[right].starts = Arc::from(
                    starts[split_at..]
                        .iter()
                        .map(|start| start - starts[split_at])
                        .collect::<Vec<_>>(),
                );
                self.chunks[right].end = right_end;
            }
            index = left;
        }
        self.recompute_first_lines();
    }

    fn recompute_first_lines(&mut self) {
        let mut first_line = 0;
        for chunk in &mut self.chunks {
            chunk.first_line = first_line;
            first_line += chunk.starts.len();
        }
    }
}

fn line_starts(text: &str) -> Vec<usize> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(index, _)| index + 1));
    starts
}

fn build_chunks(starts: &[usize], len: usize, first_line: usize) -> Vec<LineChunk> {
    if starts.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    for (index, part) in starts.chunks(TARGET_LINES).enumerate() {
        let base = part[0];
        let end = starts
            .get((index + 1) * TARGET_LINES)
            .copied()
            .unwrap_or(len);
        let relative: Vec<usize> = part.iter().map(|start| start - base).collect();
        chunks.push(LineChunk {
            base,
            first_line: first_line + index * TARGET_LINES,
            end,
            starts: Arc::from(relative),
        });
    }
    chunks
}

fn absolute_starts(chunk: &LineChunk) -> Vec<usize> {
    chunk
        .starts
        .iter()
        .map(|start| chunk.base + start)
        .collect()
}

fn add_delta(value: usize, delta: isize) -> usize {
    (value as isize + delta).max(0) as usize
}

fn display_char_width(character: char, column: usize) -> usize {
    if character == '\t' {
        TAB_WIDTH - column % TAB_WIDTH
    } else if character == '\r' || character.is_control() {
        0
    } else {
        character.width().unwrap_or(0)
    }
}

fn display_width(text: &str) -> usize {
    let mut column = 0;
    for character in text.chars() {
        column += display_char_width(character, column);
    }
    column
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PieceTable;

    fn assert_index_matches<B: TextBuffer>(buffer: &B, index: &LineIndex) {
        assert_eq!(index.len_bytes(), buffer.len_bytes());
        assert_eq!(index.len_lines(), buffer.len_lines());
        if buffer.len_bytes() == 0 {
            assert!(index.offset_to_line(0).is_none());
            return;
        }
        for line in 0..index.len_lines() {
            let offset = index.line_to_offset(line).unwrap();
            let mapped = index.offset_to_line(offset).unwrap();
            assert_eq!(mapped.line, line);
        }
        for offset in 0..=index.len_bytes() {
            if buffer.is_char_boundary(offset) {
                assert!(index.offset_to_line(offset).is_some());
            }
        }
    }

    #[test]
    fn line_semantics_match_text_buffer() {
        for text in ["", "a", "a\n", "a\r\n", "a\r\nb", "\n", "a\n\nb"] {
            let buffer = PieceTable::from_text(text);
            let index = LineIndex::from_buffer(&buffer);
            assert_index_matches(&buffer, &index);
        }
        let index = LineIndex::from_text("a\n");
        assert_eq!(index.len_lines(), 2);
        assert_eq!(index.line_to_offset(0), Some(0));
        assert_eq!(index.line_to_offset(1), Some(2));
        assert_eq!(index.offset_to_line(1).unwrap().line, 0);
        assert_eq!(index.offset_to_line(2).unwrap().line, 1);
        assert_eq!(index.offset_to_line(0).unwrap().end, 1);
        assert_eq!(index.offset_to_line(2).unwrap().end, 2);
        let crlf = LineIndex::from_text("a\r\n");
        assert_eq!(crlf.offset_to_line(0).unwrap().end, 2);
        let no_newline = LineIndex::from_text("a");
        assert_eq!(no_newline.offset_to_line(0).unwrap().end, 1);
        let blank = LineIndex::from_text("\n");
        assert_eq!(blank.offset_to_line(0).unwrap().end, 0);
        assert_eq!(blank.offset_to_line(1).unwrap().end, 1);
    }

    #[test]
    fn chunk_bounds_hold_after_edits() {
        let mut text = String::new();
        for _ in 0..4000 {
            text.push_str("line\n");
        }
        let mut buffer = PieceTable::from_text(&text);
        let mut index = LineIndex::from_buffer(&buffer);
        buffer.insert(0, &"inserted\n".repeat(3000));
        index.apply_edit(&buffer, 0..0);
        index.assert_chunk_bounds();
        assert!(index.chunk_count() > 1);
        assert!(index.chunk_sizes().iter().all(|size| *size <= MAX_LINES));
    }

    #[test]
    fn empty_text_has_no_line_position() {
        let index = LineIndex::from_text("");
        assert_eq!(index.len_lines(), 0);
        assert_eq!(index.offset_to_line(0), None);
        assert_eq!(index.line_to_offset(0), None);
    }

    #[test]
    fn insert_and_delete_update_index() {
        let mut buffer = PieceTable::from_text("a\nb\nc");
        let mut index = LineIndex::from_buffer(&buffer);
        buffer.insert(1, "x\n");
        index.apply_edit(&buffer, 1..1);
        assert_index_matches(&buffer, &index);

        buffer.delete(0..2);
        index.apply_edit(&buffer, 0..2);
        assert_index_matches(&buffer, &index);
    }

    #[test]
    fn cross_chunk_edits_match_rebuilt_index() {
        let mut text = String::new();
        for _ in 0..3000 {
            text.push_str("line\n");
        }
        let mut buffer = PieceTable::from_text(&text);
        let mut index = LineIndex::from_buffer(&buffer);
        buffer.insert(15_000, "new\nlines\n");
        index.apply_edit(&buffer, 15_000..15_000);
        assert_index_matches(&buffer, &index);
        let end = buffer.len_bytes().min(12_000);
        buffer.delete(2_000..end);
        index.apply_edit(&buffer, 2_000..end);
        assert_index_matches(&buffer, &index);
        assert!(index.chunks.len() <= index.len_lines() / MIN_LINES + 1);
    }

    #[test]
    fn cursor_conversion_handles_unicode() {
        let buffer = PieceTable::from_text("a\tb\néx");
        let index = LineIndex::from_buffer(&buffer);
        let position = index.position(&buffer, 2).unwrap();
        assert_eq!(position.line, 0);
        assert_eq!(position.grapheme_column, 2);
        assert_eq!(position.display_column, 4);
        assert_eq!(index.offset_at_grapheme(&buffer, 0, 2), Some(2));
        assert_eq!(index.offset_at_display_column(&buffer, 0, 4), Some(2));
        assert_eq!(index.position(&buffer, 1).unwrap().display_column, 1);
    }

    #[test]
    fn clone_shares_unaffected_chunks() {
        let text = "a\n".repeat(3000);
        let mut buffer = PieceTable::from_text(&text);
        let index = LineIndex::from_buffer(&buffer);
        let clone = index.clone();
        assert!(Arc::ptr_eq(
            &index.chunks[0].starts,
            &clone.chunks[0].starts
        ));
        buffer.insert(0, "x\n");
        let mut edited = index;
        edited.apply_edit(&buffer, 0..0);
        assert!(Arc::ptr_eq(
            &edited.chunks.last().unwrap().starts,
            &clone.chunks.last().unwrap().starts
        ));
    }
}

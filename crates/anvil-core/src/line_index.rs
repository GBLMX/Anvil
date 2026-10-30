//! Chunked line index and cursor coordinate conversion.
//!
//! Line metadata is stored in parallel arrays so an edit shifts contiguous
//! cache lines instead of walking a large array of chunk structs.

use std::{ops::Range, sync::Arc, thread};

use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete, UnicodeSegmentation};
use unicode_width::UnicodeWidthChar;

use crate::TextBuffer;

const TARGET_LINES: usize = 1024;
const MAX_LINES: usize = 2048;
const MIN_LINES: usize = 512;
const TAB_WIDTH: usize = 4;
const GRAPHEME_WINDOW: usize = 64;
const PARALLEL_LINE_SCAN: usize = 4 << 20;

fn floor_char_boundary<B: TextBuffer>(buffer: &B, mut offset: usize) -> usize {
    while offset > 0 && !buffer.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn ceil_char_boundary<B: TextBuffer>(buffer: &B, mut offset: usize) -> usize {
    while offset < buffer.len_bytes() && !buffer.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

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
    bases: Vec<usize>,
    first_lines: Vec<usize>,
    ends: Vec<usize>,
    starts: Vec<Arc<[usize]>>,
}

impl LineIndex {
    /// Builds an index from initial text.
    pub fn from_text(text: &str) -> Self {
        let starts = line_starts(text);
        Self::from_chunks(text.len(), build_chunks(&starts, text.len(), 0))
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
        self.starts.len()
    }

    /// Number of line starts in each chunk.
    pub fn chunk_sizes(&self) -> Vec<usize> {
        self.starts.iter().map(|starts| starts.len()).collect()
    }

    /// Asserts the chunk occupancy invariant used by the design.
    pub fn assert_chunk_bounds(&self) {
        for (index, starts) in self.starts.iter().enumerate() {
            let size = starts.len();
            assert!(size <= MAX_LINES, "chunk {index} has {size} lines");
            if index + 1 < self.starts.len() {
                assert!(size >= MIN_LINES, "chunk {index} has only {size} lines");
            }
        }
    }

    /// Number of logical lines. Empty text has zero lines.
    pub fn len_lines(&self) -> usize {
        self.first_lines
            .last()
            .zip(self.starts.last())
            .map_or(0, |(first_line, starts)| first_line + starts.len())
    }

    /// Maps a byte offset to a line and line bounds.
    ///
    /// Returns `None` for an out-of-range offset or any offset in empty text.
    pub fn offset_to_line(&self, offset: usize) -> Option<LinePosition> {
        if offset > self.len || self.starts.is_empty() {
            return None;
        }
        let chunk_index = self
            .bases
            .partition_point(|base| *base <= offset)
            .saturating_sub(1);
        let base = self.bases[chunk_index];
        let starts = &self.starts[chunk_index];
        let local = starts
            .partition_point(|start| base + start <= offset)
            .saturating_sub(1);
        let start = base + starts[local];
        let next_start = starts
            .get(local + 1)
            .map(|start| base + start)
            .unwrap_or(self.ends[chunk_index]);
        let has_next_line = local + 1 < starts.len() || chunk_index + 1 < self.starts.len();
        let end = if has_next_line && next_start <= self.len {
            next_start - 1
        } else {
            self.len
        };
        Some(LinePosition {
            line: self.first_lines[chunk_index] + local,
            start,
            end,
        })
    }

    /// Maps a zero-based line to its byte offset.
    pub fn line_to_offset(&self, line: usize) -> Option<usize> {
        if self.starts.is_empty() || line >= self.len_lines() {
            return None;
        }
        let chunk_index = self
            .first_lines
            .partition_point(|first_line| *first_line <= line)
            .saturating_sub(1);
        let local = line - self.first_lines[chunk_index];
        self.starts[chunk_index]
            .get(local)
            .map(|start| self.bases[chunk_index] + start)
    }

    /// Returns the next grapheme-cluster boundary after `offset`.
    pub fn next_grapheme_boundary<B: TextBuffer>(
        &self,
        buffer: &B,
        offset: usize,
    ) -> Option<usize> {
        if offset > buffer.len_bytes() || !buffer.is_char_boundary(offset) || self.starts.is_empty()
        {
            return None;
        }
        let len = buffer.len_bytes();
        if offset == len {
            return Some(len);
        }
        let mut end = floor_char_boundary(buffer, (offset + GRAPHEME_WINDOW).min(len));
        loop {
            let chunk = buffer.slice(offset..end);
            let mut cursor = GraphemeCursor::new(offset, len, true);
            match cursor.next_boundary(&chunk, offset) {
                Ok(Some(boundary)) => return Some(boundary),
                Ok(None) => return Some(len),
                Err(GraphemeIncomplete::NextChunk) if end < len => {
                    end = floor_char_boundary(buffer, (end + GRAPHEME_WINDOW).min(len));
                }
                Err(GraphemeIncomplete::NextChunk) => return Some(len),
                Err(_) => return None,
            }
        }
    }

    /// Returns the previous grapheme-cluster boundary before `offset`.
    pub fn previous_grapheme_boundary<B: TextBuffer>(
        &self,
        buffer: &B,
        offset: usize,
    ) -> Option<usize> {
        if offset > buffer.len_bytes() || !buffer.is_char_boundary(offset) || self.starts.is_empty()
        {
            return None;
        }
        if offset == 0 {
            return Some(0);
        }
        let len = buffer.len_bytes();
        let mut start = ceil_char_boundary(buffer, offset.saturating_sub(GRAPHEME_WINDOW));
        loop {
            let chunk = buffer.slice(start..offset);
            let mut cursor = GraphemeCursor::new(offset, len, true);
            match cursor.prev_boundary(&chunk, start) {
                Ok(Some(boundary)) => return Some(boundary),
                Ok(None) => return Some(0),
                Err(GraphemeIncomplete::PrevChunk | GraphemeIncomplete::PreContext(_))
                    if start > 0 =>
                {
                    start = ceil_char_boundary(buffer, start.saturating_sub(GRAPHEME_WINDOW));
                }
                Err(GraphemeIncomplete::PrevChunk | GraphemeIncomplete::PreContext(_)) => {
                    return Some(0);
                }
                Err(_) => return None,
            }
        }
    }

    /// Returns the cursor coordinates for a byte offset.
    pub fn position<B: TextBuffer>(&self, buffer: &B, offset: usize) -> Option<Position> {
        if offset > buffer.len_bytes() || !buffer.is_char_boundary(offset) || self.starts.is_empty()
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
    pub fn offset_at_grapheme<B: TextBuffer>(
        &self,
        buffer: &B,
        line: usize,
        column: usize,
    ) -> Option<usize> {
        let start = self.line_to_offset(line)?;
        let line = self.offset_to_line(start)?;
        let text = buffer.slice(start..line.end);
        for (index, (byte, _)) in text.grapheme_indices(true).enumerate() {
            if index == column {
                return Some(start + byte);
            }
        }
        Some(line.end)
    }

    /// Returns the byte offset at a terminal display column.
    pub fn offset_at_display_column<B: TextBuffer>(
        &self,
        buffer: &B,
        line: usize,
        column: usize,
    ) -> Option<usize> {
        let start = self.line_to_offset(line)?;
        let line = self.offset_to_line(start)?;
        let text = buffer.slice(start..line.end);
        let mut width = 0;
        for (byte, character) in text.char_indices() {
            if width >= column {
                return Some(start + byte);
            }
            width += display_char_width(character, width);
        }
        Some(line.end)
    }

    /// Applies an edit after `buffer` has already been mutated.
    ///
    /// `range` is the old byte range replaced by the edit.
    pub fn apply_edit<B: TextBuffer>(&mut self, buffer: &B, range: Range<usize>) {
        let new_len = buffer.len_bytes();
        assert!(range.start <= range.end && range.end <= self.len);
        if new_len == 0 || self.starts.is_empty() {
            *self = Self::from_buffer(buffer);
            return;
        }
        if range.start == range.end && new_len == self.len {
            return;
        }
        if range.start == range.end
            && range.start == self.len
            && new_len > self.len
            && !buffer.slice(self.len..new_len).contains('\n')
        {
            if let Some(end) = self.ends.last_mut() {
                *end = new_len;
                self.len = new_len;
                return;
            }
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
        let region_start = self.bases[first_chunk];
        let old_region_end = self.ends[last_chunk];
        let new_region_end = add_delta(old_region_end, delta).min(new_len);
        let old_region_lines = self.first_lines[last_chunk] + self.starts[last_chunk].len()
            - self.first_lines[first_chunk];
        let slice = buffer.slice(region_start..new_region_end);
        let mut local_starts = line_starts(&slice);
        if last_chunk + 1 < self.starts.len() && local_starts.last() == Some(&slice.len()) {
            local_starts.pop();
        }
        let new_region_lines = local_starts.len();
        let line_delta = new_region_lines as isize - old_region_lines as isize;

        for index in last_chunk + 1..self.starts.len() {
            self.bases[index] = add_delta(self.bases[index], delta);
            self.ends[index] = add_delta(self.ends[index], delta);
            self.first_lines[index] = add_delta(self.first_lines[index], line_delta);
        }

        let absolute_starts: Vec<usize> = local_starts
            .into_iter()
            .map(|start| region_start + start)
            .collect();
        let middle = build_chunks(
            &absolute_starts,
            new_region_end,
            self.first_lines[first_chunk],
        );
        let middle_needs_normalize = middle.iter().any(|chunk| chunk.starts.len() < MIN_LINES);
        let range_end = last_chunk + 1;
        self.bases.splice(
            first_chunk..range_end,
            middle.iter().map(|chunk| chunk.base),
        );
        self.first_lines.splice(
            first_chunk..range_end,
            middle.iter().map(|chunk| chunk.first_line),
        );
        self.ends
            .splice(first_chunk..range_end, middle.iter().map(|chunk| chunk.end));
        self.starts.splice(
            first_chunk..range_end,
            middle.iter().map(|chunk| Arc::clone(&chunk.starts)),
        );
        self.len = new_len;
        if middle_needs_normalize {
            self.normalize();
        }
    }

    fn chunk_for_line(&self, line: usize) -> usize {
        self.first_lines
            .partition_point(|first_line| *first_line <= line)
            .saturating_sub(1)
    }

    fn normalize(&mut self) {
        let mut chunks = self.to_chunks();
        let mut split = Vec::new();
        for chunk in chunks.drain(..) {
            if chunk.starts.len() <= MAX_LINES {
                split.push(chunk);
            } else {
                let chunk_count = chunk.starts.len().div_ceil(TARGET_LINES);
                let chunk_size = chunk.starts.len().div_ceil(chunk_count);
                for (part_index, part) in chunk.starts.chunks(chunk_size).enumerate() {
                    let base = chunk.base + part[0];
                    let end = chunk
                        .starts
                        .get((part_index + 1) * chunk_size)
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
        recompute_first_lines(&mut split);
        let mut index = 0;
        while index < split.len() {
            if split[index].starts.len() >= MIN_LINES {
                index += 1;
                continue;
            }
            if split.len() == 1 {
                break;
            }
            let (left, right) = if index == 0 {
                (0, 1)
            } else if index + 1 == split.len()
                || split[index - 1].starts.len() <= split[index + 1].starts.len()
            {
                (index - 1, index)
            } else {
                (index, index + 1)
            };
            let total = split[left].starts.len() + split[right].starts.len();
            let mut starts = absolute_starts(&split[left]);
            starts.extend(absolute_starts(&split[right]));
            let split_at = if total <= MAX_LINES { total } else { total / 2 };
            let left_base = split[left].base;
            let right_end = split[right].end;
            let left_first_line = split[left].first_line;
            split[left].starts = Arc::from(
                starts[..split_at]
                    .iter()
                    .map(|start| start - left_base)
                    .collect::<Vec<_>>(),
            );
            split[left].end = if split_at == total {
                right_end
            } else {
                starts[split_at]
            };
            if split_at == total {
                split.remove(right);
            } else {
                split[right].base = starts[split_at];
                split[right].first_line = left_first_line + split_at;
                split[right].starts = Arc::from(
                    starts[split_at..]
                        .iter()
                        .map(|start| start - starts[split_at])
                        .collect::<Vec<_>>(),
                );
                split[right].end = right_end;
            }
            recompute_first_lines(&mut split);
            index = left;
        }
        self.adopt_chunks(split);
    }

    fn from_chunks(len: usize, chunks: Vec<LineChunk>) -> Self {
        let mut index = Self {
            len,
            bases: Vec::with_capacity(chunks.len()),
            first_lines: Vec::with_capacity(chunks.len()),
            ends: Vec::with_capacity(chunks.len()),
            starts: Vec::with_capacity(chunks.len()),
        };
        index.adopt_chunks(chunks);
        index
    }

    fn adopt_chunks(&mut self, chunks: Vec<LineChunk>) {
        self.bases = chunks.iter().map(|chunk| chunk.base).collect();
        self.first_lines = chunks.iter().map(|chunk| chunk.first_line).collect();
        self.ends = chunks.iter().map(|chunk| chunk.end).collect();
        self.starts = chunks.into_iter().map(|chunk| chunk.starts).collect();
    }

    fn to_chunks(&self) -> Vec<LineChunk> {
        (0..self.starts.len())
            .map(|index| LineChunk {
                base: self.bases[index],
                first_line: self.first_lines[index],
                end: self.ends[index],
                starts: Arc::clone(&self.starts[index]),
            })
            .collect()
    }
}

fn line_starts(text: &str) -> Vec<usize> {
    if text.is_empty() {
        return Vec::new();
    }
    let bytes = text.as_bytes();
    let configured = std::env::var("ANVIL_LINE_SCAN_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|workers| *workers > 0)
        .unwrap_or(16);
    let workers = thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(configured)
        .min(bytes.len() / (1 << 20))
        .max(1);
    if workers == 1 || bytes.len() < PARALLEL_LINE_SCAN {
        return serial_line_starts(bytes);
    }
    // Ranges are half-open. A newline is ASCII and cannot be a UTF-8
    // continuation byte, so a newline at a boundary belongs to exactly one range.
    let chunk = bytes.len().div_ceil(workers);
    let mut ranges = Vec::with_capacity(workers);
    for index in 0..workers {
        let start = index * chunk;
        let end = (start + chunk).min(bytes.len());
        if start < end {
            ranges.push((start, end));
        }
    }
    let counts = thread::scope(|scope| {
        let mut handles = Vec::with_capacity(ranges.len());
        for (start, end) in &ranges {
            let start = *start;
            let end = *end;
            handles.push(scope.spawn(move || {
                bytes[start..end]
                    .iter()
                    .filter(|byte| **byte == b'\n')
                    .count()
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("line scan worker"))
            .collect::<Vec<_>>()
    });
    let total = 1 + counts.iter().sum::<usize>();
    let mut starts = vec![0; total];
    let mut rest = &mut starts[1..];
    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(ranges.len());
        for ((start, end), count) in ranges.into_iter().zip(counts) {
            let (slot, next) = rest.split_at_mut(count);
            rest = next;
            handles.push(scope.spawn(move || {
                let mut index = 0;
                for (offset, byte) in bytes[start..end].iter().enumerate() {
                    if *byte == b'\n' {
                        slot[index] = start + offset + 1;
                        index += 1;
                    }
                }
            }));
        }
        for handle in handles {
            handle.join().expect("line scan worker");
        }
    });
    starts
}

fn serial_line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(
        bytes
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == b'\n')
            .map(|(index, _)| index + 1),
    );
    starts
}

fn build_chunks(starts: &[usize], len: usize, first_line: usize) -> Vec<LineChunk> {
    if starts.is_empty() {
        return Vec::new();
    }
    let chunk_count = starts.len().div_ceil(TARGET_LINES);
    let chunk_size = starts.len().div_ceil(chunk_count);
    let mut chunks = Vec::new();
    for (index, part) in starts.chunks(chunk_size).enumerate() {
        let base = part[0];
        let end = starts.get((index + 1) * chunk_size).copied().unwrap_or(len);
        let relative: Vec<usize> = part.iter().map(|start| start - base).collect();
        chunks.push(LineChunk {
            base,
            first_line: first_line + index * chunk_size,
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

fn recompute_first_lines(chunks: &mut [LineChunk]) {
    let mut first_line = 0;
    for chunk in chunks {
        chunk.first_line = first_line;
        first_line += chunk.starts.len();
    }
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
    fn parallel_line_scan_matches_serial() {
        let text = "a\n".repeat(3_000_000);
        assert_eq!(line_starts(&text), serial_line_starts(text.as_bytes()));
        let index = LineIndex::from_text(&text);
        assert_eq!(index.len_lines(), 3_000_001);
        assert_eq!(index.line_to_offset(3_000_000), Some(6_000_000));
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
        assert!(index.chunk_count() <= index.len_lines() / MIN_LINES + 1);
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
        assert!(Arc::ptr_eq(&index.starts[0], &clone.starts[0]));
        buffer.insert(0, "x\n");
        let mut edited = index;
        edited.apply_edit(&buffer, 0..0);
        assert!(Arc::ptr_eq(
            edited.starts.last().unwrap(),
            clone.starts.last().unwrap()
        ));
    }
}

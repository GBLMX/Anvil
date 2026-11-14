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
const COLUMN_CACHE_MIN_BYTES: usize = 64 * 1024;
const COLUMN_CHECKPOINT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy)]
struct ColumnCheckpoint {
    byte: usize,
    grapheme: usize,
    scalar: usize,
    display: usize,
}

#[derive(Debug, Clone, Copy)]
enum ColumnBias {
    Right,
    Nearest,
}

#[derive(Debug, Clone)]
struct LineColumns {
    line_start: usize,
    checkpoints: Vec<ColumnCheckpoint>,
}

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
    /// Zero-based Unicode scalar column.
    pub char_column: usize,
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
    columns: Vec<LineColumns>,
}

impl LineIndex {
    /// Builds an index from initial text.
    pub fn from_text(text: &str) -> Self {
        let starts = line_starts(text);
        let mut index = Self::from_chunks(text.len(), build_chunks(&starts, text.len(), 0));
        index.build_columns(text);
        index
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

    fn build_columns(&mut self, text: &str) {
        self.columns.clear();
        for chunk in 0..self.starts.len() {
            let base = self.bases[chunk];
            for (line, relative_start) in self.starts[chunk].iter().enumerate() {
                let start = base + *relative_start;
                let next = if line + 1 < self.starts[chunk].len() {
                    base + self.starts[chunk][line + 1]
                } else if chunk + 1 < self.starts.len() {
                    self.bases[chunk + 1]
                } else {
                    self.len
                };
                let end = if next > start && text.as_bytes().get(next - 1) == Some(&b'\n') {
                    next - 1
                } else {
                    next
                };
                if end.saturating_sub(start) > COLUMN_CACHE_MIN_BYTES {
                    self.columns.push(build_line_columns(text, start, end));
                }
            }
        }
    }

    fn columns_for_line(&self, line_start: usize) -> Option<&LineColumns> {
        self.columns
            .binary_search_by_key(&line_start, |columns| columns.line_start)
            .ok()
            .map(|index| &self.columns[index])
    }

    fn column_checkpoint(&self, line_start: usize, byte: usize) -> Option<ColumnCheckpoint> {
        let columns = self.columns_for_line(line_start)?;
        let relative = byte.saturating_sub(line_start);
        let index = columns
            .checkpoints
            .partition_point(|checkpoint| checkpoint.byte <= relative)
            .saturating_sub(1);
        columns.checkpoints.get(index).copied()
    }

    /// Returns the cursor coordinates for a byte offset.
    pub fn position<B: TextBuffer>(&self, buffer: &B, offset: usize) -> Option<Position> {
        if offset > buffer.len_bytes() || !buffer.is_char_boundary(offset) || self.starts.is_empty()
        {
            return None;
        }
        let line = self.offset_to_line(offset)?;
        let (grapheme_column, char_column, display_column) =
            if let Some(checkpoint) = self.column_checkpoint(line.start, offset) {
                let prefix = buffer.slice(line.start + checkpoint.byte..offset);
                (
                    checkpoint.grapheme + prefix.graphemes(true).count(),
                    checkpoint.scalar + prefix.chars().count(),
                    checkpoint.display + display_width_from(&prefix, checkpoint.display),
                )
            } else {
                let prefix = buffer.slice(line.start..offset);
                (
                    prefix.graphemes(true).count(),
                    prefix.chars().count(),
                    display_width_from(&prefix, 0),
                )
            };
        Some(Position {
            byte_offset: offset,
            line: line.line,
            grapheme_column,
            char_column,
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

    /// Returns the byte offset at a Unicode scalar column.
    pub fn offset_at_char<B: TextBuffer>(
        &self,
        buffer: &B,
        line: usize,
        column: usize,
    ) -> Option<usize> {
        let start = self.line_to_offset(line)?;
        let line = self.offset_to_line(start)?;
        let text = buffer.slice(start..line.end);
        for (index, (byte, _)) in text.char_indices().enumerate() {
            if index == column {
                return Some(start + byte);
            }
        }
        (column >= text.chars().count()).then_some(line.end)
    }

    /// Returns the byte offset at a UTF-16 column.
    ///
    /// Returns `None` for an invalid line, a column beyond the line, or a
    /// column inside a surrogate pair.
    pub fn offset_at_utf16<B: TextBuffer>(
        &self,
        buffer: &B,
        line: usize,
        column: usize,
    ) -> Option<usize> {
        let start = self.line_to_offset(line)?;
        let line = self.offset_to_line(start)?;
        let text = buffer.slice(start..line.end);
        let mut units = 0;
        for (byte, character) in text.char_indices() {
            if units == column {
                return Some(start + byte);
            }
            let next = units + character.len_utf16();
            if column < next {
                return None;
            }
            units = next;
        }
        (units == column).then_some(line.end)
    }

    /// UTF-16 column for a byte offset.
    pub fn utf16_column<B: TextBuffer>(&self, buffer: &B, offset: usize) -> Option<usize> {
        if offset > buffer.len_bytes() || !buffer.is_char_boundary(offset) {
            return None;
        }
        let line = self.offset_to_line(offset)?;
        let prefix = buffer.slice(line.start..offset);
        Some(prefix.chars().map(char::len_utf16).sum())
    }

    /// Returns the byte offset at a terminal display column.
    ///
    /// The result is always a grapheme-cluster boundary. Columns inside a wide
    /// cluster resolve to its trailing boundary; use
    /// [`Self::offset_at_display_column_rounded`] for pointer hit testing.
    pub fn offset_at_display_column<B: TextBuffer>(
        &self,
        buffer: &B,
        line: usize,
        column: usize,
    ) -> Option<usize> {
        self.offset_at_display_column_with_bias(buffer, line, column, ColumnBias::Right)
    }

    /// Returns the nearest grapheme boundary for a pointer display column.
    pub fn offset_at_display_column_rounded<B: TextBuffer>(
        &self,
        buffer: &B,
        line: usize,
        column: usize,
    ) -> Option<usize> {
        self.offset_at_display_column_with_bias(buffer, line, column, ColumnBias::Nearest)
    }

    fn offset_at_display_column_with_bias<B: TextBuffer>(
        &self,
        buffer: &B,
        line: usize,
        column: usize,
        bias: ColumnBias,
    ) -> Option<usize> {
        let start = self.line_to_offset(line)?;
        let line = self.offset_to_line(start)?;
        let (scan_start, mut width, scan_end) =
            if let Some(columns) = self.columns_for_line(line.start) {
                let index = columns
                    .checkpoints
                    .partition_point(|checkpoint| checkpoint.display <= column)
                    .saturating_sub(1);
                let checkpoint = columns.checkpoints[index];
                let end = columns
                    .checkpoints
                    .get(index + 1)
                    .map_or(line.end, |next| line.start + next.byte);
                (line.start + checkpoint.byte, checkpoint.display, end)
            } else {
                (start, 0, line.end)
            };
        let text = buffer.slice(scan_start..scan_end);
        for (byte, cluster) in text.grapheme_indices(true) {
            if width >= column {
                return Some(scan_start + byte);
            }
            let next = width + display_width_from(cluster, width);
            if column < next {
                return Some(match bias {
                    ColumnBias::Right => scan_start + byte + cluster.len(),
                    ColumnBias::Nearest => {
                        if column - width < next - column {
                            scan_start + byte
                        } else {
                            scan_start + byte + cluster.len()
                        }
                    }
                });
            }
            width = next;
        }
        Some(line.end)
    }

    /// Applies an edit after `buffer` has already been mutated.
    ///
    /// `range` is the old byte range replaced by the edit.
    pub fn apply_edit<B: TextBuffer>(&mut self, buffer: &B, range: Range<usize>) {
        let new_len = buffer.len_bytes();
        assert!(range.start <= range.end && range.end <= self.len);
        self.columns.clear();
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
            self.normalize_window(first_chunk, first_chunk + middle.len());
        }
    }

    /// Applies an insertion that cannot change line structure.
    pub fn apply_insert<B: TextBuffer>(&mut self, buffer: &B, at: usize, text: &str) {
        if text.is_empty() || text.contains('\n') || self.starts.is_empty() {
            self.apply_edit(buffer, at..at);
            return;
        }
        let Some(line) = self.offset_to_line(at) else {
            self.apply_edit(buffer, at..at);
            return;
        };
        let chunk = self.chunk_for_line(line.line);
        let base = self.bases[chunk];
        let delta = text.len();
        self.columns.clear();
        self.len = buffer.len_bytes();
        self.ends[chunk] = self.ends[chunk].saturating_add(delta);
        for index in chunk + 1..self.starts.len() {
            self.bases[index] = self.bases[index].saturating_add(delta);
            self.ends[index] = self.ends[index].saturating_add(delta);
        }
        let starts = Arc::make_mut(&mut self.starts[chunk]);
        for start in starts.iter_mut() {
            if base + *start > at {
                *start = start.saturating_add(delta);
            }
        }
    }

    fn chunk_for_line(&self, line: usize) -> usize {
        self.first_lines
            .partition_point(|first_line| *first_line <= line)
            .saturating_sub(1)
    }

    fn normalize_window(&mut self, first: usize, last: usize) {
        let mut end = (last + 1).min(self.starts.len());
        let mut index = first.saturating_sub(1);
        while index < end {
            if self.starts[index].len() >= MIN_LINES {
                index += 1;
                continue;
            }
            if self.starts.len() == 1 {
                break;
            }
            let (left, right) = if index == 0 {
                (0, 1)
            } else if index + 1 == self.starts.len()
                || self.starts[index - 1].len() <= self.starts[index + 1].len()
            {
                (index - 1, index)
            } else {
                (index, index + 1)
            };
            let left_base = self.bases[left];
            let right_end = self.ends[right];
            let left_first_line = self.first_lines[left];
            let mut starts: Vec<usize> = self.starts[left]
                .iter()
                .map(|start| left_base + start)
                .collect();
            starts.extend(
                self.starts[right]
                    .iter()
                    .map(|start| self.bases[right] + start),
            );
            let total = starts.len();
            let split_at = if total <= MAX_LINES { total } else { total / 2 };
            self.starts[left] = Arc::from(
                starts[..split_at]
                    .iter()
                    .map(|start| start - left_base)
                    .collect::<Vec<_>>(),
            );
            self.bases[left] = left_base;
            self.ends[left] = if split_at == total {
                right_end
            } else {
                starts[split_at]
            };
            if split_at == total {
                self.bases.remove(right);
                self.first_lines.remove(right);
                self.ends.remove(right);
                self.starts.remove(right);
                end = end.saturating_sub(1);
            } else {
                let right_base = starts[split_at];
                self.bases[right] = right_base;
                self.first_lines[right] = left_first_line + split_at;
                self.ends[right] = right_end;
                self.starts[right] = Arc::from(
                    starts[split_at..]
                        .iter()
                        .map(|start| start - right_base)
                        .collect::<Vec<_>>(),
                );
            }
            index = left;
        }
    }

    fn from_chunks(len: usize, chunks: Vec<LineChunk>) -> Self {
        let mut index = Self {
            len,
            bases: Vec::with_capacity(chunks.len()),
            first_lines: Vec::with_capacity(chunks.len()),
            ends: Vec::with_capacity(chunks.len()),
            starts: Vec::with_capacity(chunks.len()),
            columns: Vec::new(),
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
}

fn line_starts(text: &str) -> Vec<usize> {
    if text.is_empty() {
        return Vec::new();
    }
    let bytes = text.as_bytes();
    if bytes.len() < PARALLEL_LINE_SCAN {
        return serial_line_starts(bytes);
    }
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

#[cfg(test)]
fn display_width(text: &str) -> usize {
    let mut column = 0;
    for character in text.chars() {
        column += display_char_width(character, column);
    }
    column
}

fn display_width_from(text: &str, start: usize) -> usize {
    let mut column = start;
    for character in text.chars() {
        column += display_char_width(character, column);
    }
    column - start
}

fn build_line_columns(text: &str, start: usize, end: usize) -> LineColumns {
    let slice = &text[start..end];
    let mut checkpoints = Vec::new();
    let mut grapheme = 0;
    let mut scalar = 0;
    let mut display = 0;
    let mut next_checkpoint = 0;
    let ascii = slice.is_ascii();
    if ascii && slice.bytes().all(|byte| (0x20..0x7f).contains(&byte)) {
        for byte in (0..slice.len()).step_by(COLUMN_CHECKPOINT_BYTES) {
            checkpoints.push(ColumnCheckpoint {
                byte,
                grapheme: byte,
                scalar: byte,
                display: byte,
            });
        }
    } else if ascii {
        for (byte, character) in slice.bytes().enumerate() {
            if byte >= next_checkpoint {
                checkpoints.push(ColumnCheckpoint {
                    byte,
                    grapheme,
                    scalar,
                    display,
                });
                next_checkpoint = (byte / COLUMN_CHECKPOINT_BYTES + 1) * COLUMN_CHECKPOINT_BYTES;
            }
            display += if character == b'\t' {
                TAB_WIDTH - display % TAB_WIDTH
            } else if character == b'\r' || character.is_ascii_control() {
                0
            } else {
                1
            };
            scalar += 1;
            grapheme += 1;
        }
    } else {
        for (byte, cluster) in slice.grapheme_indices(true) {
            if byte >= next_checkpoint {
                checkpoints.push(ColumnCheckpoint {
                    byte,
                    grapheme,
                    scalar,
                    display,
                });
                next_checkpoint = (byte / COLUMN_CHECKPOINT_BYTES + 1) * COLUMN_CHECKPOINT_BYTES;
            }
            for character in cluster.chars() {
                display += display_char_width(character, display);
                scalar += 1;
            }
            grapheme += 1;
        }
    }
    LineColumns {
        line_start: start,
        checkpoints,
    }
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
    fn scalar_utf16_and_display_columns_round_trip() {
        let buffer = PieceTable::from_text("a🦀b");
        let index = LineIndex::from_buffer(&buffer);
        let position = index.position(&buffer, 5).expect("after crab");
        assert_eq!(position.char_column, 2);
        assert_eq!(position.grapheme_column, 2);
        assert_eq!(index.offset_at_char(&buffer, 0, 2), Some(5));
        assert_eq!(index.utf16_column(&buffer, 5), Some(3));
        assert_eq!(index.offset_at_utf16(&buffer, 0, 1), Some(1));
        assert_eq!(index.offset_at_utf16(&buffer, 0, 2), None);
        assert_eq!(index.offset_at_utf16(&buffer, 0, 3), Some(5));
        assert_eq!(index.offset_at_utf16(&buffer, 0, 4), Some(6));
        assert_eq!(index.offset_at_utf16(&buffer, 0, 5), None);
    }

    #[test]
    fn display_columns_never_split_grapheme_clusters() {
        let buffer = PieceTable::from_text("a👨‍👩‍👧‍👦b");
        let index = LineIndex::from_buffer(&buffer);
        let emoji_end = index.next_grapheme_boundary(&buffer, 1).expect("emoji end");
        assert_eq!(index.offset_at_display_column(&buffer, 0, 1), Some(1));
        assert_eq!(
            index.offset_at_display_column(&buffer, 0, 2),
            Some(emoji_end)
        );
        assert_eq!(
            index.offset_at_display_column_rounded(&buffer, 0, 1),
            Some(1)
        );
        assert_eq!(
            index.offset_at_display_column_rounded(&buffer, 0, 2),
            Some(1)
        );
        assert_eq!(
            index.offset_at_display_column_rounded(&buffer, 0, 5),
            Some(emoji_end)
        );
        for column in 0..=4 {
            let offset = index
                .offset_at_display_column_rounded(&buffer, 0, column)
                .expect("rounded display column");
            assert!(buffer.is_char_boundary(offset));
        }
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

    #[test]
    fn sparse_columns_match_linear_scan() {
        let text = format!("\t你好\x7f{}", "x".repeat(70_000));
        let buffer = PieceTable::from_text(&text);
        let index = LineIndex::from_buffer(&buffer);
        assert!(!index.columns.is_empty());

        for offset in [0, 1, 4, 16_384, 65_536, text.len()] {
            let position = index.position(&buffer, offset).expect("position");
            assert_eq!(
                position.grapheme_column,
                text[..offset].graphemes(true).count()
            );
            assert_eq!(position.display_column, display_width(&text[..offset]));
        }

        let total = display_width(&text);
        for column in [0, 1, 4, 5, 16_384, 65_536, total] {
            let mut expected = text.len();
            let mut width = 0;
            for (byte, character) in text.char_indices() {
                if width >= column {
                    expected = byte;
                    break;
                }
                width += display_char_width(character, width);
            }
            assert_eq!(
                index.offset_at_display_column(&buffer, 0, column),
                Some(expected)
            );
        }
    }

    #[test]
    #[ignore = "manual 100MB column cache boundary"]
    fn sparse_columns_100mb() {
        let text = "x".repeat(100 * 1024 * 1024);
        let buffer = PieceTable::from_text(&text);
        let started = std::time::Instant::now();
        let index = LineIndex::from_buffer(&buffer);
        let build = started.elapsed();
        let started = std::time::Instant::now();
        let position = index.position(&buffer, text.len()).expect("position");
        let position_time = started.elapsed();
        let started = std::time::Instant::now();
        let offset = index
            .offset_at_display_column(&buffer, 0, text.len() - 1)
            .expect("column");
        let column_time = started.elapsed();
        println!("columns-100MB build={build:?} position={position_time:?} column={column_time:?}");
        assert_eq!(position.display_column, text.len());
        assert_eq!(offset, text.len() - 1);
    }

    #[test]
    fn indexed_insert_matches_rebuilt_index() {
        let mut buffer = PieceTable::from_text("abc\ndef\n");
        let mut index = LineIndex::from_buffer(&buffer);
        buffer.insert(2, "x");
        index.apply_insert(&buffer, 2, "x");
        assert_index_matches(&buffer, &index);
        buffer.insert(0, "start");
        index.apply_insert(&buffer, 0, "start");
        assert_index_matches(&buffer, &index);
        let end = buffer.len_bytes();
        buffer.insert(end, "tail");
        index.apply_insert(&buffer, end, "tail");
        assert_index_matches(&buffer, &index);
    }
}

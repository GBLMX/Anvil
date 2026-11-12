//! Viewport, gutter, and visible-line layout.

use super::*;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DisplayRow {
    Source(usize),
    CodeLens {
        line: usize,
        title: String,
        hidden: usize,
        index: Option<usize>,
    },
    Wrapped {
        line: usize,
        start: usize,
        end: usize,
        first: bool,
        truncated: bool,
    },
}

impl Model {
    pub(crate) fn wrap_segments(&self, line: usize, width: usize) -> Vec<(usize, usize, bool)> {
        let Some(start) = self.editor.index().line_to_offset(line) else {
            return Vec::new();
        };
        let Some(range) = self.editor.index().offset_to_line(start) else {
            return Vec::new();
        };
        let text = self.editor.text().slice(start..range.end);
        let line_start = start;
        let width = width.max(1);
        let mut segments = Vec::new();
        let mut segment_start = 0usize;
        let mut display = 0usize;
        for (index, grapheme) in text.grapheme_indices(true) {
            let grapheme_width = if grapheme == "\t" {
                4
            } else {
                UnicodeWidthStr::width(grapheme)
            };
            if display > 0 && display + grapheme_width > width {
                if segments.len() >= 256 {
                    return segments
                        .into_iter()
                        .enumerate()
                        .map(|(index, (start, end))| {
                            (line_start + start, line_start + end, index + 1 == 256)
                        })
                        .collect();
                }
                segments.push((segment_start, index));
                segment_start = index;
                display = 0;
            }
            display += grapheme_width;
        }
        segments.push((segment_start, text.len()));
        let count = segments.len();
        segments
            .into_iter()
            .enumerate()
            .map(|(index, (start, end))| {
                (
                    line_start + start,
                    line_start + end,
                    index + 1 == count && count >= 256,
                )
            })
            .collect()
    }

    pub(crate) fn wrap_text_width(&self, width: u16) -> usize {
        let mut width = width;
        if self.tree_visible && width > 50 {
            let sidebar = (width / 5).clamp(18, 40).min(width - 20);
            width = width.saturating_sub(sidebar);
        }
        if self.split.is_some() {
            let split = width.saturating_mul(self.split_ratio.min(100)) / 100;
            width = if self.ui.focus.split {
                split
            } else {
                width.saturating_sub(split)
            };
        }
        usize::from(width.saturating_sub(self.gutter_width()).max(1))
    }

    pub(crate) fn scroll_display_rows(&mut self, delta: isize, width: u16) {
        if !self.settings.editor.soft_wrap {
            self.scroll = self
                .scroll
                .saturating_add_signed(delta)
                .min(self.editor.index().len_lines().saturating_sub(1));
            self.scroll_subrow = 0;
            return;
        }
        self.wrap_width = self.wrap_text_width(width);
        let total = self.editor.index().len_lines();
        if total == 0 {
            return;
        }
        self.scroll = self.scroll.min(total.saturating_sub(1));
        let mut remaining = delta.unsigned_abs();
        if delta > 0 {
            while remaining > 0 {
                let segments = self
                    .wrap_segments(self.scroll, self.wrap_width)
                    .len()
                    .max(1);
                let available = segments.saturating_sub(self.scroll_subrow);
                if remaining < available {
                    self.scroll_subrow += remaining;
                    break;
                }
                remaining -= available;
                let next = self.next_visible_line(self.scroll);
                if next >= total {
                    self.scroll = total - 1;
                    self.scroll_subrow = segments.saturating_sub(1);
                    break;
                }
                self.scroll = next;
                self.scroll_subrow = 0;
            }
        } else {
            while remaining > 0 {
                if self.scroll_subrow > 0 {
                    let take = remaining.min(self.scroll_subrow);
                    self.scroll_subrow -= take;
                    remaining -= take;
                    continue;
                }
                if self.scroll == 0 {
                    break;
                }
                let previous = self.first_visible_line(self.scroll.saturating_sub(1));
                if previous == self.scroll {
                    break;
                }
                self.scroll = previous;
                self.scroll_subrow = self
                    .wrap_segments(previous, self.wrap_width)
                    .len()
                    .saturating_sub(1);
                remaining -= 1;
            }
        }
    }

    pub(crate) fn visible_rows(&self, start: usize, height: usize) -> Vec<DisplayRow> {
        if height == 0 {
            return Vec::new();
        }
        let total = self.editor.index().len_lines();
        let max_lens_rows = crate::code_lens::MAX_CODE_LENS_ROWS_PER_VIEWPORT.min(height / 2);
        let visible_lines = self.visible_line_numbers(start, height);
        let total_items = visible_lines
            .iter()
            .map(|line| self.code_lens_items_for_line(*line).len())
            .sum::<usize>();
        let global_hidden = total_items.saturating_sub(max_lens_rows);
        let lens_limit = if global_hidden > 0 {
            max_lens_rows.saturating_sub(1)
        } else {
            max_lens_rows
        };
        let mut shown_items = 0;
        let mut lens_rows = 0;
        let mut rows = Vec::with_capacity(height);
        let mut line = self.first_visible_line(start);
        while rows.len() < height && line < total {
            let lenses = self.code_lens_items_for_line(line);
            if !lenses.is_empty() && lens_rows < lens_limit {
                let visible = lenses.len().min(crate::code_lens::MAX_CODE_LENS_PER_LINE);
                let truncate = lenses.len() > visible;
                let shown = if truncate {
                    visible.saturating_sub(1)
                } else {
                    visible
                };
                for (index, lens) in lenses.iter().take(shown) {
                    if lens_rows >= lens_limit {
                        break;
                    }
                    rows.push(DisplayRow::CodeLens {
                        line,
                        title: lens.title.clone(),
                        hidden: 0,
                        index: Some(*index),
                    });
                    lens_rows += 1;
                    shown_items += 1;
                }
                if truncate && lens_rows < lens_limit && rows.len() < height {
                    let hidden = lenses.len().saturating_sub(shown);
                    rows.push(DisplayRow::CodeLens {
                        line,
                        title: format!("… +{hidden} lenses"),
                        hidden,
                        index: None,
                    });
                    lens_rows += 1;
                }
            }
            if rows.len() >= height {
                break;
            }
            if self.settings.editor.soft_wrap && self.wrap_width > 0 {
                let segments = self.wrap_segments(line, self.wrap_width);
                let first_segment = if line == self.scroll {
                    self.scroll_subrow.min(segments.len().saturating_sub(1))
                } else {
                    0
                };
                for (index, (start, end, truncated)) in
                    segments.into_iter().enumerate().skip(first_segment)
                {
                    rows.push(DisplayRow::Wrapped {
                        line,
                        start,
                        end,
                        first: index == first_segment,
                        truncated,
                    });
                    if rows.len() >= height {
                        break;
                    }
                }
            } else {
                rows.push(DisplayRow::Source(line));
            }
            line = self.next_visible_line(line);
        }
        if global_hidden > 0 {
            let hidden = total_items.saturating_sub(shown_items);
            if rows.len() >= height {
                rows.pop();
            }
            rows.push(DisplayRow::CodeLens {
                line: line.saturating_sub(1),
                title: format!("… +{hidden} lenses"),
                hidden,
                index: None,
            });
        }
        rows
    }

    pub(crate) fn code_lens_at_row(&self, offset: usize, height: usize) -> Option<usize> {
        match self.visible_rows(self.scroll, height).get(offset) {
            Some(DisplayRow::CodeLens {
                index: Some(index), ..
            }) => Some(*index),
            _ => None,
        }
    }

    pub(crate) fn position_at_visible_row(&self, row: usize) -> Option<(usize, usize)> {
        let text = self.editor.text();
        let rows = self.visible_rows(self.scroll, row.saturating_add(1));
        match rows.get(row) {
            Some(DisplayRow::Source(line)) => Some((*line, self.hscroll)),
            Some(DisplayRow::Wrapped { line, start, .. }) => {
                let base = self
                    .editor
                    .index()
                    .position(text, *start)
                    .map_or(0, |position| position.display_column);
                Some((*line, base))
            }
            _ => None,
        }
    }

    pub(crate) fn row_of_position(
        &self,
        line: usize,
        display_column: usize,
        height: usize,
    ) -> Option<usize> {
        let text = self.editor.text();
        let line_end = self
            .editor
            .index()
            .line_to_offset(line)
            .and_then(|offset| self.editor.index().offset_to_line(offset))
            .map_or(0, |range| range.end);
        self.visible_rows(self.scroll, height)
            .iter()
            .enumerate()
            .find_map(|(row, candidate)| match candidate {
                DisplayRow::Source(source) if *source == line => Some(row),
                DisplayRow::Wrapped {
                    line: wrapped,
                    start,
                    end,
                    ..
                } if *wrapped == line => {
                    let start_column = self
                        .editor
                        .index()
                        .position(text, *start)
                        .map_or(0, |position| position.display_column);
                    let end_column = self
                        .editor
                        .index()
                        .position(text, *end)
                        .map_or(start_column, |position| position.display_column);
                    (display_column >= start_column
                        && (display_column < end_column || *end >= line_end))
                        .then_some(row)
                }
                _ => None,
            })
    }

    pub(crate) fn row_of_source_line(&self, line: usize, height: usize) -> Option<usize> {
        self.visible_rows(self.scroll, height)
            .iter()
            .position(|row| {
                matches!(
                    row,
                    DisplayRow::Source(source) if *source == line
                ) || matches!(
                    row,
                    DisplayRow::Wrapped {
                        line: wrapped,
                        first: true,
                        ..
                    } if *wrapped == line
                )
            })
    }
}
impl Model {
    pub(crate) fn gutter_digits(&self) -> usize {
        self.editor
            .index()
            .len_lines()
            .max(1)
            .to_string()
            .len()
            .max(3)
    }

    pub(crate) fn gutter_width(&self) -> u16 {
        (self.gutter_digits() + 2) as u16
    }
    pub(crate) fn current_line(&self) -> Option<usize> {
        self.editor
            .index()
            .position(self.editor.text(), self.cursor)
            .map(|position| position.line)
    }
    pub(crate) fn document_highlight_ranges(
        &self,
        line_start: usize,
        line_end: usize,
    ) -> Vec<(usize, usize, u8)> {
        let Some(highlights) = &self.document_highlights else {
            return Vec::new();
        };
        if highlights.revision != self.editor.revision() {
            return Vec::new();
        }
        highlights
            .items
            .iter()
            .filter_map(|highlight| {
                let start = highlight.start.max(line_start);
                let end = highlight.end.min(line_end);
                (end > start).then_some((start, end, highlight.kind))
            })
            .collect()
    }

    pub(crate) fn line_in_code_block(&self, line_number: usize) -> bool {
        let Some(start) = self.editor.index().line_to_offset(line_number) else {
            return false;
        };
        let Some(line) = self.editor.index().offset_to_line(start) else {
            return false;
        };
        let line_end = line.end.max(start + 1);
        self.syntax.borrow().highlights().iter().any(|highlight| {
            highlight.class == syntax::SyntaxClass::CodeBlock
                && highlight.start < line_end
                && highlight.end > start
        })
    }

    pub(crate) fn visible_gutter(&self, height: u16) -> Vec<Line<'static>> {
        let visible = self.visible_rows(self.scroll, usize::from(height));
        let current = self.current_line();
        let digits = self.gutter_digits();
        let mut lines = Vec::with_capacity(visible.len());
        for row in visible {
            let line_number = match row {
                DisplayRow::Source(line_number) => line_number,
                DisplayRow::Wrapped {
                    line, first: true, ..
                } => line,
                DisplayRow::Wrapped { .. } | DisplayRow::CodeLens { .. } => {
                    lines.push(Line::raw(" ".repeat(digits + 1)));
                    continue;
                }
            };
            let display = match current {
                Some(current) if current != line_number => current.abs_diff(line_number),
                _ => line_number + 1,
            };
            let marker = if self
                .path
                .as_ref()
                .is_some_and(|path| self.breakpoint_marker(path, line_number))
            {
                '●'
            } else {
                self.fold_marker(line_number)
                    .or_else(|| self.git_marker(line_number))
                    .unwrap_or(' ')
            };
            let number = format!("{display:>digits$} ");
            let mut style = Style::default().fg(theme::muted());
            if Some(line_number) == current {
                style = theme::current_line();
            }
            let marker_style = match marker {
                '+' => Style::default().fg(theme::success()),
                '-' => Style::default().fg(theme::error()),
                '~' => Style::default().fg(theme::warning()),
                '▸' | '▾' => Style::default().fg(theme::accent()),
                '●' => Style::default().fg(theme::error()),
                _ => Style::default(),
            };
            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), marker_style),
                Span::styled(number, style),
            ]));
        }
        if lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines
    }

    pub(crate) fn visible_lines(&self, height: u16, width: u16) -> Vec<Line<'_>> {
        let index = self.editor.index();
        let text = self.editor.text();
        let visible = self.visible_rows(self.scroll, usize::from(height));
        let current = self.current_line();

        let diagnostic_lines = self.diagnostic_lines();
        let bracket_pair = self.bracket_pair();
        let hscroll = if self.settings.editor.soft_wrap {
            0
        } else {
            self.hscroll
        };
        let visible_width = usize::from(width);
        let mut lines = Vec::with_capacity(visible.len());
        for row in visible {
            let (
                line_number,
                start,
                line_end,
                render_start,
                render_end,
                pad,
                truncated,
                row_base,
                last_segment,
            ) = match row {
                DisplayRow::Source(line) => {
                    let Some(start) = index.line_to_offset(line) else {
                        break;
                    };
                    let Some(line_range) = index.offset_to_line(start) else {
                        break;
                    };
                    let (render_start, render_end, pad) =
                        self.line_window(line, start, line_range.end, usize::from(width));
                    (
                        line,
                        start,
                        line_range.end,
                        render_start,
                        render_end,
                        pad,
                        false,
                        hscroll,
                        true,
                    )
                }
                DisplayRow::CodeLens { title, hidden, .. } => {
                    let style = if hidden > 0 {
                        Style::default().fg(theme::muted())
                    } else {
                        Style::default().fg(theme::accent())
                    };
                    lines.push(Line::styled(format!(" {title}"), style));
                    continue;
                }
                DisplayRow::Wrapped {
                    line,
                    start,
                    end,
                    truncated,
                    ..
                } => {
                    let Some(line_start) = index.line_to_offset(line) else {
                        break;
                    };
                    let Some(line_range) = index.offset_to_line(line_start) else {
                        break;
                    };
                    let base = index
                        .position(text, start)
                        .map_or(0, |position| position.display_column);
                    (
                        line,
                        line_start,
                        line_range.end,
                        start,
                        end,
                        0,
                        truncated,
                        base,
                        end >= line_range.end,
                    )
                }
            };
            let mut spans = Vec::new();
            let mut base = render_start;
            let search_ranges: Vec<(usize, usize)> = self
                .search
                .matches
                .iter()
                .filter(|range| range.start < render_end && range.end > render_start)
                .map(|range| (range.start.max(render_start), range.end.min(render_end)))
                .collect();
            let document_highlight_ranges = self.document_highlight_ranges(start, line_end);
            let bracket_ranges = bracket_pair
                .iter()
                .flat_map(|&(open, close)| [(open, open + 1), (close, close + 1)])
                .filter(|&(range_start, range_end)| {
                    range_start < render_end && range_end > render_start
                })
                .map(|(range_start, range_end)| {
                    (range_start.max(render_start), range_end.min(render_end))
                })
                .collect::<Vec<_>>();
            let semantic_ranges = self.semantic_ranges(start, line_end);
            let syntax_guard = self.syntax.borrow();
            let highlights = syntax_guard.highlights();
            for chunk in text.chunks(render_start..render_end) {
                push_syntax_spans(&mut spans, chunk, base, highlights);
                base += chunk.len();
            }
            if !semantic_ranges.is_empty() {
                apply_semantic_tokens(&mut spans, render_start, &semantic_ranges);
            }
            if !document_highlight_ranges.is_empty() {
                apply_document_highlights(&mut spans, render_start, &document_highlight_ranges);
            }
            if !bracket_ranges.is_empty() {
                apply_bracket_highlights(&mut spans, render_start, &bracket_ranges);
            }
            if !search_ranges.is_empty() {
                apply_search_highlights(&mut spans, render_start, &search_ranges, self.cursor);
            }
            if spans.is_empty() {
                spans.push(Span::raw(""));
            }
            if last_segment {
                self.insert_inlay_hints(line_number, render_start, render_end, &mut spans);
            }
            if pad > 0 {
                spans.insert(0, Span::raw(" ".repeat(pad)));
            }
            expand_tabs(&mut spans, pad);
            if truncated {
                spans.push(Span::styled("…", Style::default().fg(theme::warning())));
            }
            let diagnostic = diagnostic_lines.contains(&line_number);
            let full_ranges = if diagnostic {
                self.diagnostic_ranges(line_number, start, line_end)
            } else {
                Vec::new()
            };
            let ranges = full_ranges
                .iter()
                .filter_map(|(range_start, range_end)| {
                    let start =
                        (*range_start as isize - row_base as isize + pad as isize).max(0) as usize;
                    let end = (*range_end as isize - row_base as isize + pad as isize)
                        .min(visible_width as isize);
                    (end > start as isize).then_some((start, end as usize))
                })
                .collect::<Vec<_>>();
            apply_diagnostic_underlines(&mut spans, &ranges);
            let mut rendered = Line::from(spans);
            if Some(line_number) == current {
                rendered = rendered.style(theme::current_line());
            }
            if diagnostic && full_ranges.is_empty() && hscroll == 0 && render_start == start {
                rendered = rendered.style(
                    Style::default()
                        .fg(theme::error())
                        .add_modifier(Modifier::UNDERLINED),
                );
            }
            lines.push(rendered);
        }
        if lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines
    }

    pub(crate) fn line_window(
        &self,
        line_number: usize,
        start: usize,
        end: usize,
        width: usize,
    ) -> (usize, usize, usize) {
        let index = self.editor.index();
        let text = self.editor.text();
        let hscroll = self.hscroll;
        let Some(start_offset) = index.offset_at_display_column(text, line_number, hscroll) else {
            return (start, end, 0);
        };
        let start_display = index
            .position(text, start_offset)
            .map_or(0, |position| position.display_column);
        let (render_start, pad) = if start_display < hscroll {
            let next = index
                .next_grapheme_boundary(text, start_offset)
                .unwrap_or(end)
                .min(end);
            let next_display = index
                .position(text, next)
                .map_or(start_display, |position| position.display_column);
            (next, next_display.saturating_sub(hscroll))
        } else {
            (start_offset, 0)
        };
        let target = hscroll.saturating_add(width);
        let mut render_end = index
            .offset_at_display_column(text, line_number, target)
            .unwrap_or(end)
            .min(end);
        if render_end < render_start {
            render_end = render_start;
        }
        (render_start, render_end, pad)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_wrap_projects_sources_into_multiple_rows() {
        let mut model = Model::new(EditorState::from_text("abcdefghij\nnext"), None);
        model.settings.editor.soft_wrap = true;
        model.wrap_width = 4;

        let rows = model.visible_rows(0, 10);
        let wrapped = rows
            .iter()
            .filter(|row| matches!(row, DisplayRow::Wrapped { .. }))
            .count();
        assert!(wrapped >= 3, "{rows:?}");
        assert_eq!(model.row_of_position(0, 5, 10), Some(1));
        assert_eq!(model.position_at_visible_row(1), Some((0, 4)));
        let rendered = model
            .visible_lines(10, 4)
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(rendered.len(), 4);
        assert_eq!(rendered[0], "abcd");
        assert_eq!(rendered[1], "efgh");
        assert_eq!(rendered[2], "ij");
        assert_eq!(rendered[3], "next");
    }

    #[test]
    fn soft_wrap_cursor_scroll_tracks_segment() {
        let mut model = Model::new(EditorState::from_text("abcdefghij"), None);
        model.settings.editor.soft_wrap = true;
        model.wrap_width = 4;
        model.cursor = 9;
        model.editor.set_selection_bytes(9, 9);
        model.ensure_cursor_visible(2, 5);
        assert!(model.scroll_subrow > 0, "subrow={}", model.scroll_subrow);
    }

    #[test]
    fn wrap_width_accounts_for_tree_and_split() {
        let mut model = Model::new(EditorState::from_text("x"), None);
        let gutter = usize::from(model.gutter_width());
        assert_eq!(model.wrap_text_width(100), 100 - gutter);
        model.tree_visible = true;
        let sidebar = (100u16 / 5).clamp(18, 40).min(100 - 20);
        assert_eq!(
            model.wrap_text_width(100),
            usize::from(100 - sidebar) - gutter
        );
        model.split_vertical();
        model.split_ratio = 50;
        let editor_width = 100 - sidebar;
        let half = editor_width / 2;
        model.ui.focus.split = false;
        assert_eq!(
            model.wrap_text_width(100),
            usize::from(editor_width - half) - gutter
        );
        model.ui.focus.split = true;
        assert_eq!(model.wrap_text_width(100), usize::from(half) - gutter);
    }

    #[test]
    fn wrapped_scroll_advances_display_rows() {
        let mut model = Model::new(EditorState::from_text("abcdefghij\nnext"), None);
        model.settings.editor.soft_wrap = true;
        let width = model.gutter_width() + 2;
        model.scroll_display_rows(1, width);
        assert_eq!(model.scroll, 0);
        assert_eq!(model.scroll_subrow, 1);
        model.scroll_display_rows(4, width);
        assert_eq!(model.scroll, 1);
        assert_eq!(model.scroll_subrow, 0);
        model.scroll_display_rows(-1, width);
        assert_eq!(model.scroll, 0);
        assert_eq!(model.scroll_subrow, 4);
    }

    #[test]
    fn wrap_segment_cap_keeps_absolute_offsets() {
        let mut model = Model::new(
            EditorState::from_text(&format!("first\n{}", "a".repeat(300))),
            None,
        );
        model.settings.editor.soft_wrap = true;
        let segments = model.wrap_segments(1, 1);
        assert_eq!(segments.len(), 256);
        assert_eq!(segments[0].0, 6);
        assert_eq!(segments[255].0, 6 + 255);
        assert!(segments[255].2);
    }

    #[test]
    fn disabling_soft_wrap_restores_source_rows() {
        let mut model = Model::new(EditorState::from_text("abcdefghij"), None);
        model.settings.editor.soft_wrap = false;
        let rows = model.visible_rows(0, 5);
        assert!(rows.iter().any(|row| matches!(row, DisplayRow::Source(0))));
        assert!(
            !rows
                .iter()
                .any(|row| matches!(row, DisplayRow::Wrapped { .. }))
        );
    }
}

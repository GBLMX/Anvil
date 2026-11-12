//! Viewport, gutter, and visible-line layout.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DisplayRow {
    Source(usize),
    CodeLens {
        line: usize,
        title: String,
        hidden: usize,
        index: Option<usize>,
    },
}

impl Model {
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
            rows.push(DisplayRow::Source(line));
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

    pub(crate) fn row_of_source_line(&self, line: usize, height: usize) -> Option<usize> {
        self.visible_rows(self.scroll, height)
            .iter()
            .position(|row| matches!(row, DisplayRow::Source(source) if *source == line))
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
            let DisplayRow::Source(line_number) = row else {
                lines.push(Line::raw(" ".repeat(digits + 1)));
                continue;
            };
            let display = match current {
                Some(current) if current != line_number => current.abs_diff(line_number),
                _ => line_number + 1,
            };
            let marker = self
                .fold_marker(line_number)
                .or_else(|| self.git_marker(line_number))
                .unwrap_or(' ');
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
        let hscroll = self.hscroll;
        let visible_width = usize::from(width);
        let mut lines = Vec::with_capacity(visible.len());
        for row in visible {
            let line_number = match row {
                DisplayRow::Source(line) => line,
                DisplayRow::CodeLens { title, hidden, .. } => {
                    let style = if hidden > 0 {
                        Style::default().fg(theme::muted())
                    } else {
                        Style::default().fg(theme::accent())
                    };
                    lines.push(Line::styled(format!(" {title}"), style));
                    continue;
                }
            };
            let Some(start) = index.line_to_offset(line_number) else {
                break;
            };
            let Some(line) = index.offset_to_line(start) else {
                break;
            };
            let (render_start, render_end, pad) =
                self.line_window(line_number, start, line.end, usize::from(width));
            let mut spans = Vec::new();
            let mut base = render_start;
            let search_ranges: Vec<(usize, usize)> = self
                .search
                .matches
                .iter()
                .filter(|range| range.start < render_end && range.end > render_start)
                .map(|range| (range.start.max(render_start), range.end.min(render_end)))
                .collect();
            let document_highlight_ranges = self.document_highlight_ranges(start, line.end);
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
            let semantic_ranges = self.semantic_ranges(start, line.end);
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
            self.insert_inlay_hints(line_number, render_start, render_end, &mut spans);
            if pad > 0 {
                spans.insert(0, Span::raw(" ".repeat(pad)));
            }
            expand_tabs(&mut spans, pad);
            let diagnostic = diagnostic_lines.contains(&line_number);
            let full_ranges = if diagnostic {
                self.diagnostic_ranges(line_number, start, line.end)
            } else {
                Vec::new()
            };
            let ranges = full_ranges
                .iter()
                .filter_map(|(range_start, range_end)| {
                    let start =
                        (*range_start as isize - hscroll as isize + pad as isize).max(0) as usize;
                    let end = (*range_end as isize - hscroll as isize + pad as isize)
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

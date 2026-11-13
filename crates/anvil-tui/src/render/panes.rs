//! Split-pane and pipe-location rendering.

use super::*;
use crate::theme;
use ratatui::widgets::Borders;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) fn render_pipe_locations(model: &Model, frame: &mut Frame, area: Rect) {
    let rows = usize::from(area.height.saturating_sub(2));
    let start = model.overlays.location.stdin_location_scroll.min(
        model
            .overlays
            .location
            .stdin_locations
            .len()
            .saturating_sub(rows.max(1)),
    );
    let lines = model
        .overlays
        .location
        .stdin_locations
        .iter()
        .enumerate()
        .skip(start)
        .take(rows)
        .map(|(index, location)| {
            let text = truncate_width(
                &format!(
                    "{}:{}:{} {}",
                    location.path, location.line, location.column, location.message
                ),
                area.width.saturating_sub(3),
            );
            let style = if model.overlays.location.stdin_location_selected == Some(index) {
                theme::selected()
            } else {
                theme::panel()
            };
            Line::styled(format!(" {text}"), style)
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::panel())
            .block(panel_block(format!(
                "{} · {}",
                model.overlays.location.location_title,
                model.overlays.location.stdin_locations.len()
            ))),
        area,
    );
}

pub(crate) fn render_split_pane(
    frame: &mut Frame,
    area: Rect,
    buffer: &Buffer,
    is_active: bool,
    soft_wrap: bool,
) {
    let block = ratatui::widgets::Block::default()
        .borders(Borders::LEFT)
        .border_style(theme::border(is_active));
    let inner = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(1),
        ..area
    };
    frame.render_widget(block, area);
    let total = buffer.editor.index().len_lines();
    let digits = total.max(1).to_string().len().max(3);
    let text_width = usize::from(inner.width.saturating_sub((digits + 1) as u16));
    let mut lines = Vec::new();
    'lines: for line_number in buffer.scroll..total {
        let Some(start) = buffer.editor.index().line_to_offset(line_number) else {
            break;
        };
        let Some(line) = buffer.editor.index().offset_to_line(start) else {
            break;
        };
        let text = buffer.editor.text().slice(start..line.end);
        if soft_wrap && text_width > 0 {
            for (index, (segment_start, segment_end)) in
                wrap_ranges(&text, text_width).into_iter().enumerate()
            {
                let prefix = if index == 0 {
                    format!("{:>digits$} ", line_number + 1, digits = digits)
                } else {
                    " ".repeat(digits + 1)
                };
                lines.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(theme::muted())),
                    Span::raw(text[segment_start..segment_end].to_owned()),
                ]));
                if lines.len() >= usize::from(inner.height) {
                    break 'lines;
                }
            }
        } else {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:>digits$} ", line_number + 1, digits = digits),
                    Style::default().fg(theme::muted()),
                ),
                Span::raw(text),
            ]));
            if lines.len() >= usize::from(inner.height) {
                break;
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::raw(""));
    }
    let hscroll = if soft_wrap {
        0
    } else {
        u16::try_from(buffer.hscroll).unwrap_or(u16::MAX)
    };
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::text())
            .scroll((0, hscroll)),
        inner,
    );
}

fn wrap_ranges(text: &str, width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut display = 0usize;
    for (index, grapheme) in text.grapheme_indices(true) {
        let grapheme_width = if grapheme == "\t" {
            4
        } else {
            UnicodeWidthStr::width(grapheme)
        };
        if display > 0 && display + grapheme_width > width {
            ranges.push((start, index));
            start = index;
            display = 0;
        }
        display += grapheme_width;
    }
    ranges.push((start, text.len()));
    ranges
}

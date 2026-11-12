//! Split-pane and pipe-location rendering.

use super::*;
use crate::theme;

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

pub(crate) fn render_split_pane(frame: &mut Frame, area: Rect, buffer: &Buffer, is_active: bool) {
    let block = panel_block(
        buffer
            .path
            .as_ref()
            .map_or("[no file]".to_owned(), |path| path.display().to_string()),
    )
    .border_style(theme::border(is_active));
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    frame.render_widget(block, area);
    let total = buffer.editor.index().len_lines();
    let digits = total.max(1).to_string().len().max(3);
    let end = (buffer.scroll + usize::from(inner.height)).min(total);
    let mut lines = Vec::new();
    for line_number in buffer.scroll..end {
        let Some(start) = buffer.editor.index().line_to_offset(line_number) else {
            break;
        };
        let Some(line) = buffer.editor.index().offset_to_line(start) else {
            break;
        };
        let text = buffer.editor.text().slice(start..line.end);
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:>digits$} ", line_number + 1, digits = digits),
                Style::default().fg(theme::muted()),
            ),
            Span::raw(text),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::raw(""));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::text())
            .scroll((0, u16::try_from(buffer.hscroll).unwrap_or(u16::MAX))),
        inner,
    );
}

//! Ratatui rendering for the editor, tree, completion, and hover overlays.

use super::*;
use crate::file_query::FileQuery;
use crate::theme;
use anvil_api::EditorView;
use anvil_session::ui::CreateKind;
use anvil_syntax::{Highlight, SyntaxClass};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod editor;
mod overlays;
mod panes;
mod tree;
mod welcome;
pub use editor::view;
use overlays::render_overlays;
use panes::{render_pipe_locations, render_split_pane};
use tree::render_tree;
use welcome::render_welcome;
pub(crate) use welcome::{WelcomeHit, welcome_hit_at};

pub(crate) const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub(crate) fn panel_block(title: impl Into<Line<'static>>) -> Block<'static> {
    let title = title.into();
    let title = if title.spans.iter().any(|span| !span.content.is_empty()) {
        let mut spans = Vec::with_capacity(title.spans.len() + 2);
        spans.push(Span::raw(" "));
        spans.extend(title.spans);
        spans.push(Span::raw(" "));
        Line::from(spans)
    } else {
        title
    };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border(false))
        .style(theme::panel())
        .title_style(theme::title())
        .title(title)
}

pub(crate) fn focused_panel_block(title: impl Into<Line<'static>>) -> Block<'static> {
    panel_block(title)
        .border_type(BorderType::Thick)
        .border_style(theme::border(true))
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

pub(crate) fn truncate_width(text: &str, max: u16) -> String {
    let max = usize::from(max);
    let mut output = String::new();
    let mut width = 0;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width > max {
            break;
        }
        width += character_width;
        output.push(character);
    }
    output
}

pub(crate) fn command_palette_rect(area: Rect) -> Rect {
    let width = (area.width * 3 / 5)
        .max(40)
        .min(area.width.saturating_sub(2));
    let height = (area.height * 3 / 5)
        .max(10)
        .min(area.height.saturating_sub(2));
    centered_rect(area, width, height)
}

pub(crate) fn command_completion_rect(area: Rect, items: usize) -> Rect {
    let width = area.width.saturating_sub(4).clamp(20, 72);
    let height = (items as u16 + 2)
        .min(8)
        .min(area.height.saturating_sub(2))
        .max(3);
    Rect {
        x: area.x + 1,
        y: area.height.saturating_sub(height + 1),
        width,
        height,
    }
}

pub(crate) fn fuzzy_rect(area: Rect) -> Rect {
    let horizontal = area.width >= 80;
    let stacked = !horizontal && area.height >= 16;
    let width = if horizontal {
        (area.width * 3 / 4).max(60)
    } else {
        (area.width * 3 / 5).max(30)
    }
    .min(area.width.saturating_sub(2));
    let height = if stacked {
        (area.height * 3 / 4).max(10)
    } else {
        (area.height / 2).max(8)
    }
    .min(area.height.saturating_sub(2));
    centered_rect(area, width, height)
}

pub(crate) fn fuzzy_row_at(model: &Model, area: Rect, col: u16, row: u16) -> Option<usize> {
    let fuzzy = model.overlays.fuzzy.as_ref()?;
    let horizontal = area.width >= 80;
    let stacked = !horizontal && area.height >= 16;
    let popup = fuzzy_rect(area);
    let list_width = if horizontal {
        (popup.width * 55 / 100)
            .max(20)
            .min(popup.width.saturating_sub(20))
    } else {
        popup.width
    };
    let list_height = if stacked {
        (popup.height * 3 / 5)
            .max(6)
            .min(popup.height.saturating_sub(4))
    } else {
        popup.height
    };
    let list_area = Rect {
        width: list_width,
        height: list_height,
        ..popup
    };
    let inner = Rect {
        x: list_area.x + 1,
        y: list_area.y + 1,
        width: list_area.width.saturating_sub(2),
        height: list_area.height.saturating_sub(2),
    };
    if !rect_contains(inner, col, row) {
        return None;
    }
    let visible = usize::from(list_area.height.saturating_sub(3));
    let offset = usize::from(row.saturating_sub(inner.y));
    if visible == 0 || offset >= visible {
        return None;
    }
    let start = fuzzy.selected.saturating_sub(visible.saturating_sub(1));
    let selected = start + offset;
    (selected < fuzzy.filtered.len()).then_some(selected)
}

pub(crate) fn code_actions_rect(area: Rect, items: usize) -> Rect {
    let width = (area.width * 3 / 5)
        .max(36)
        .min(area.width.saturating_sub(2));
    let height = (items as u16 + 2)
        .min(14)
        .min(area.height.saturating_sub(2));
    centered_rect(area, width, height)
}

pub(crate) fn document_symbols_rect(area: Rect, items: usize) -> Rect {
    let width = (area.width * 3 / 5)
        .max(36)
        .min(area.width.saturating_sub(2));
    let height = (items as u16 + 2)
        .min(16)
        .min(area.height.saturating_sub(2));
    centered_rect(area, width, height)
}

pub(crate) fn completion_rect(area: Rect, items: usize) -> Rect {
    let width = (area.width * 2 / 5)
        .max(36)
        .min(area.width.saturating_sub(2));
    let height = (items as u16 + 2)
        .min(14)
        .min(area.height.saturating_sub(2));
    centered_rect(area, width, height)
}

pub(crate) fn rect_contains(rect: Rect, col: u16, row: u16) -> bool {
    col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
}

struct BufferTab {
    label: String,
    style: Style,
    icon: Option<(String, Color)>,
    width: usize,
}

pub(crate) struct BufferTabHit {
    pub range: std::ops::Range<u16>,
    pub buffer: Option<usize>,
}

fn buffer_tabs(model: &Model) -> (Vec<BufferTab>, usize) {
    let ascii_icons = model.appearance.ascii_icons;
    let icons = &model.appearance.icons;
    let mut tabs: Vec<BufferTab> = model
        .buffers
        .entries
        .iter()
        .map(|buffer| {
            let name = buffer
                .path
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "[no file]".to_owned());
            let marker = if buffer.missing {
                " !"
            } else if buffer.modified {
                " ●"
            } else {
                ""
            };
            let label = format!("{name}{marker} ");
            let icon = buffer
                .path
                .as_deref()
                .and_then(|path| path.file_name())
                .and_then(|name| {
                    let icon = icon_theme::icon_for(&name.to_string_lossy())
                        .unwrap_or_else(icon_theme::default_file_icon);
                    let text = icons.icon_text(icon, ascii_icons)?.trim_end().to_owned();
                    let color = if buffer.missing {
                        theme::dim()
                    } else {
                        (icon.color)()
                    };
                    Some((text, color))
                });
            let icon_width = icon
                .as_ref()
                .map(|(text, _)| UnicodeWidthStr::width(text.as_str()))
                .unwrap_or(0);
            let width = 1
                + UnicodeWidthStr::width(label.as_str())
                + icon_width
                + usize::from(icon.is_some());
            BufferTab {
                label,
                style: theme::panel(),
                icon,
                width,
            }
        })
        .collect();
    let mut active = model.buffers.active.min(tabs.len().saturating_sub(1));
    if model.pager.back.is_some() {
        let name = model.pager.title.as_deref().unwrap_or("help");
        let label = format!("[{name}] RO ");
        let width = 1 + UnicodeWidthStr::width(label.as_str());
        tabs.push(BufferTab {
            label,
            style: theme::selected(),
            icon: None,
            width,
        });
        active = tabs.len() - 1;
    }
    (tabs, active)
}

fn buffer_tab_window(tabs: &[BufferTab], active: usize, width: u16) -> (usize, usize) {
    let width = usize::from(width);
    let mut start = active;
    let mut end = active + 1;
    let mut used = tabs[active].width;
    while used < width {
        let mut progressed = false;
        if end < tabs.len() {
            let next = tabs[end].width + 1;
            if used + next <= width {
                used += next;
                end += 1;
                progressed = true;
            }
        }
        if start > 0 {
            let next = tabs[start - 1].width + 1;
            if used + next <= width {
                used += next;
                start -= 1;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    (start, end)
}

fn tab_label(tab: &BufferTab, width: u16) -> String {
    let icon_width = tab
        .icon
        .as_ref()
        .map(|(text, _)| UnicodeWidthStr::width(text.as_str()))
        .unwrap_or(0);
    let label_width =
        usize::from(width).saturating_sub(1 + icon_width + usize::from(tab.icon.is_some()));
    if UnicodeWidthStr::width(tab.label.as_str()) > label_width {
        crate::status_line::middle_ellipsis(&tab.label, label_width)
    } else {
        tab.label.clone()
    }
}

pub(crate) fn buffer_tab_hits(model: &Model, width: u16) -> Vec<BufferTabHit> {
    let (tabs, active) = buffer_tabs(model);
    if tabs.is_empty() {
        return Vec::new();
    }
    let (start, end) = buffer_tab_window(&tabs, active, width);
    let mut hits = Vec::with_capacity(end - start);
    let mut offset = u16::from(start > 0);
    for (index, tab) in tabs.iter().enumerate().take(end).skip(start) {
        if index > start {
            offset = offset.saturating_add(1);
        }
        let icon_width = tab
            .icon
            .as_ref()
            .map(|(text, _)| UnicodeWidthStr::width(text.as_str()))
            .unwrap_or(0);
        let label = tab_label(tab, width);
        let drawn = 1
            + icon_width
            + usize::from(tab.icon.is_some())
            + UnicodeWidthStr::width(label.as_str());
        let end = offset.saturating_add(drawn.min(usize::from(u16::MAX)) as u16);
        hits.push(BufferTabHit {
            range: offset..end,
            buffer: (index < model.buffers.entries.len()).then_some(index),
        });
        offset = end;
    }
    hits
}

pub(crate) fn buffer_tab_at(model: &Model, width: u16, offset: u16) -> Option<usize> {
    buffer_tab_hits(model, width)
        .into_iter()
        .find(|hit| hit.range.contains(&offset))
        .and_then(|hit| hit.buffer)
}

fn buffer_bar_line(model: &Model, width: u16) -> Line<'static> {
    let (mut tabs, active) = buffer_tabs(model);
    if tabs.is_empty() {
        return Line::default();
    }
    tabs[active].style = theme::selected();
    let (start, end) = buffer_tab_window(&tabs, active, width);
    let mut spans = Vec::new();
    if start > 0 {
        spans.push(Span::styled("‹", theme::panel().fg(theme::muted())));
    }
    for (index, tab) in tabs.iter().enumerate().take(end).skip(start) {
        if index > start {
            spans.push(Span::styled("│", theme::panel().fg(theme::border_color())));
        }
        spans.push(Span::styled(" ", tab.style));
        if let Some((text, color)) = &tab.icon {
            spans.push(Span::styled(text.clone(), tab.style.fg(*color)));
            spans.push(Span::styled(" ", tab.style));
        }
        spans.push(Span::styled(tab_label(tab, width), tab.style));
    }
    if end < tabs.len() {
        spans.push(Span::styled(
            crate::art::SELECTED,
            theme::panel().fg(theme::muted()),
        ));
    }
    Line::from(spans)
}

fn progress_lines(model: &Model) -> Vec<Line<'static>> {
    model
        .ui
        .progress
        .items()
        .iter()
        .take(5)
        .map(|item| {
            let fraction = item
                .fraction
                .map(|value| format!("{value}% "))
                .unwrap_or_default();
            let cancel = if item.cancellable {
                " · cancellable"
            } else {
                ""
            };
            Line::styled(
                format!("{fraction}{}: {}{cancel}", item.title, item.message),
                theme::panel().fg(theme::accent()),
            )
        })
        .collect()
}

pub(crate) fn problems_popup_rect(model: &Model, area: Rect) -> Rect {
    let width = (area.width * 3 / 4)
        .max(30)
        .min(area.width.saturating_sub(2));
    let mut last_path: Option<&std::path::Path> = None;
    let mut headers = 0;
    for (_, problem) in model.ui.problems.visible() {
        let path = problem.path.as_path();
        if last_path != Some(path) {
            last_path = Some(path);
            headers += 1;
        }
    }
    let rows = model.ui.problems.visible_len() + headers;
    let height = (rows.max(1) as u16 + 2)
        .min(18)
        .min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

fn problem_lines(model: &Model, width: usize) -> (Vec<Line<'static>>, Option<usize>) {
    let mut lines = Vec::new();
    let mut selected_line = None;
    let mut current_path = None;
    for (index, problem) in model.ui.problems.visible() {
        if current_path.as_ref() != Some(&problem.path) {
            current_path = Some(problem.path.clone());
            lines.push(Line::styled(
                problem.path.display().to_string(),
                theme::panel().fg(theme::accent()),
            ));
        }
        let severity = match problem.severity {
            Some(1) => "[E]",
            Some(2) => "[W]",
            Some(3) => "[I]",
            _ => "[D]",
        };
        let source = problem
            .source
            .as_deref()
            .or(problem.code.as_deref())
            .unwrap_or("");
        let message =
            crate::status_line::middle_ellipsis(&problem.message, width.saturating_sub(12));
        let text = format!(
            "{severity} {}:{} {source} {message}",
            problem.line + 1,
            problem.character + 1
        );
        let style = if index == model.ui.problems.selected {
            selected_line = Some(lines.len());
            theme::selected()
        } else {
            theme::panel()
        };
        lines.push(Line::styled(text, style));
    }
    (lines, selected_line)
}

pub(crate) fn diagnostic_peek_rect(
    area: Rect,
    cursor_y: u16,
    width: u16,
    desired_height: u16,
) -> Option<Rect> {
    let bottom = area.y + area.height;
    let below = bottom.saturating_sub(cursor_y.saturating_add(1));
    let above = cursor_y.saturating_sub(area.y);
    let max_height = below.max(above).min(area.height.saturating_sub(2));
    let height = desired_height.min(max_height);
    if height == 0 {
        return None;
    }
    let y = if below >= height {
        cursor_y.saturating_add(1)
    } else {
        cursor_y.saturating_sub(height)
    };
    let width = width.min(area.width);
    Some(Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y,
        width,
        height,
    })
}

fn diagnostic_peek_lines(peek: &anvil_session::ui::DiagnosticPeek) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for diagnostic in &peek.diagnostics {
        let severity = match diagnostic.severity {
            Some(1) => "[E]",
            Some(2) => "[W]",
            Some(3) => "[I]",
            _ => "[D]",
        };
        let source = diagnostic.source.as_deref().unwrap_or("");
        let code = diagnostic.code.as_deref().unwrap_or("");
        lines.push(Line::styled(
            format!("{severity} {source} {code}"),
            theme::panel().fg(match diagnostic.severity {
                Some(1) => theme::error(),
                Some(2) => theme::warning(),
                _ => theme::accent(),
            }),
        ));
        for line in diagnostic.message.lines() {
            lines.push(Line::styled(line.to_owned(), theme::panel()));
        }
        for related in &diagnostic.related {
            lines.push(Line::styled(
                format!("  ↳ {related}"),
                theme::panel().fg(theme::muted()),
            ));
        }
    }
    lines
}

pub(crate) fn apply_diagnostic_underlines(spans: &mut Vec<Span<'_>>, ranges: &[(usize, usize)]) {
    if ranges.is_empty() {
        return;
    }
    let mut result = Vec::new();
    let mut column = 0;
    for span in spans.drain(..) {
        let mut run = String::new();
        let mut run_style = span.style;
        let mut run_underlined = false;
        for character in span.content.chars() {
            let width = if character == '\t' {
                0
            } else {
                character.width().unwrap_or(0)
            };
            let underlined = ranges
                .iter()
                .any(|(start, end)| column < *end && column + width.max(1) > *start);
            let style = if underlined {
                span.style
                    .fg(theme::error())
                    .add_modifier(Modifier::UNDERLINED)
            } else {
                span.style
            };
            if !run.is_empty() && (style != run_style || underlined != run_underlined) {
                result.push(Span::styled(std::mem::take(&mut run), run_style));
            }
            run_style = style;
            run_underlined = underlined;
            run.push(character);
            column += width;
        }
        if !run.is_empty() {
            result.push(Span::styled(run, run_style));
        }
    }
    *spans = result;
}

pub(crate) fn expand_tabs(spans: &mut Vec<Span<'_>>, mut column: usize) {
    for span in spans.iter_mut() {
        if !span.content.contains('\t') {
            column += span.content.width();
            continue;
        }
        let mut expanded = String::with_capacity(span.content.len());
        for character in span.content.chars() {
            if character == '\t' {
                let spaces = anvil_core::tab_width(column);
                expanded.extend(std::iter::repeat_n(' ', spaces));
                column += spaces;
            } else {
                expanded.push(character);
                column += character.width().unwrap_or(0);
            }
        }
        *span = Span::styled(expanded, span.style);
    }
}

pub(crate) fn push_syntax_spans<'a>(
    spans: &mut Vec<Span<'a>>,
    chunk: &'a str,
    base: usize,
    highlights: &[Highlight],
) {
    let chunk_end = base + chunk.len();
    let first = highlights.partition_point(|highlight| highlight.end <= base);
    let mut cursor = base;
    for highlight in &highlights[first..] {
        if highlight.start >= chunk_end {
            break;
        }
        if highlight.class == SyntaxClass::CodeBlock {
            continue;
        }
        let start = base
            + next_char_boundary(
                chunk,
                highlight.start.max(base).max(cursor).saturating_sub(base),
            );
        let end =
            base + previous_char_boundary(chunk, highlight.end.min(chunk_end).saturating_sub(base));
        if end <= start {
            continue;
        }
        if start > cursor {
            spans.push(Span::raw(&chunk[cursor - base..start - base]));
        }
        if end > start {
            spans.push(Span::styled(
                &chunk[start - base..end - base],
                syntax_style(highlight.class),
            ));
        }
        cursor = end.max(cursor);
    }
    if cursor < chunk_end {
        spans.push(Span::raw(&chunk[cursor - base..]));
    }
    if chunk.is_empty() {
        spans.push(Span::raw(""));
    }
}

fn next_char_boundary(text: &str, mut local: usize) -> usize {
    local = local.min(text.len());
    while local < text.len() && !text.is_char_boundary(local) {
        local += 1;
    }
    local
}

fn previous_char_boundary(text: &str, mut local: usize) -> usize {
    local = local.min(text.len());
    while local > 0 && !text.is_char_boundary(local) {
        local -= 1;
    }
    local
}

/// Applies selection backgrounds without replacing syntax foregrounds.
pub(crate) fn apply_selection_highlights<'a>(
    spans: &mut Vec<Span<'a>>,
    line_start: usize,
    ranges: &[(usize, usize)],
) {
    apply_overlay_ranges(
        spans,
        line_start,
        ranges,
        |range| range,
        |_| Style::default().bg(theme::selection()),
    );
}

/// Applies the primary cursor block over one grapheme.
pub(crate) fn apply_cursor_highlight<'a>(
    spans: &mut Vec<Span<'a>>,
    line_start: usize,
    range: (usize, usize),
) {
    apply_overlay_ranges(
        spans,
        line_start,
        &[range],
        |range| range,
        |_| Style::default().fg(theme::surface()).bg(theme::warning()),
    );
}
/// Applies search backgrounds without replacing existing syntax foregrounds.
pub(crate) fn apply_search_highlights<'a>(
    spans: &mut Vec<Span<'a>>,
    line_start: usize,
    ranges: &[(usize, usize)],
    active_start: usize,
) {
    apply_overlay_ranges(
        spans,
        line_start,
        ranges,
        |range| range,
        |range| {
            if range.0 == active_start {
                theme::search_active()
            } else {
                theme::search_match()
            }
        },
    );
}

/// Applies document-highlight backgrounds without replacing syntax colors.
pub(crate) fn apply_document_highlights<'a>(
    spans: &mut Vec<Span<'a>>,
    line_start: usize,
    ranges: &[(usize, usize, u8)],
) {
    apply_overlay_ranges(
        spans,
        line_start,
        ranges,
        |range| (range.0, range.1),
        |range| {
            let overlay = theme::search_match();
            if range.2 == 3 {
                overlay.add_modifier(Modifier::UNDERLINED)
            } else {
                overlay
            }
        },
    );
}

/// Applies matching-bracket emphasis without replacing syntax colors.
pub(crate) fn apply_bracket_highlights(
    spans: &mut Vec<Span<'_>>,
    line_start: usize,
    ranges: &[(usize, usize)],
) {
    apply_overlay_ranges(
        spans,
        line_start,
        ranges,
        |range| range,
        |_| {
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD)
        },
    );
}

/// Applies semantic-token foregrounds over syntax spans.
pub(crate) fn apply_semantic_tokens<'a>(
    spans: &mut Vec<Span<'a>>,
    line_start: usize,
    ranges: &[(usize, usize, Style)],
) {
    apply_overlay_ranges(
        spans,
        line_start,
        ranges,
        |range| (range.0, range.1),
        |range| range.2,
    );
}

fn apply_overlay_ranges<'a, R: Copy>(
    spans: &mut Vec<Span<'a>>,
    line_start: usize,
    ranges: &[R],
    bounds: impl Fn(R) -> (usize, usize),
    style: impl Fn(R) -> Style,
) {
    if ranges.is_empty() {
        return;
    }
    let mut result = Vec::with_capacity(spans.len() + ranges.len());
    let mut offset = line_start;
    for span in spans.drain(..) {
        let content = span.content.as_ref();
        let span_start = offset;
        let span_end = offset + content.len();
        offset = span_end;
        let mut cursor = span_start;
        for &range in ranges {
            let (start, end) = bounds(range);
            if end <= span_start {
                continue;
            }
            if start >= span_end {
                break;
            }
            let overlay_start = start.max(span_start).max(cursor);
            let overlay_end = end.min(span_end);
            if overlay_end <= overlay_start {
                continue;
            }
            if overlay_start > cursor {
                let from = cursor - span_start;
                let to = overlay_start - span_start;
                result.push(Span::styled(content[from..to].to_owned(), span.style));
            }
            let from = overlay_start - span_start;
            let to = overlay_end - span_start;
            result.push(Span::styled(
                content[from..to].to_owned(),
                span.style.patch(style(range)),
            ));
            cursor = overlay_end;
        }
        if cursor < span_end {
            let from = cursor - span_start;
            result.push(Span::styled(content[from..].to_owned(), span.style));
        }
    }
    *spans = result;
}

fn syntax_style(class: SyntaxClass) -> Style {
    if class == SyntaxClass::CodeBlock {
        return theme::code_block();
    }
    let (color, index) = match class {
        SyntaxClass::Keyword => (theme::syntax_keyword(), 0),
        SyntaxClass::String => (theme::syntax_string(), 1),
        SyntaxClass::Comment => (theme::syntax_comment(), 2),
        SyntaxClass::Number => (theme::syntax_number(), 3),
        SyntaxClass::Function => (theme::syntax_function(), 4),
        SyntaxClass::Type => (theme::syntax_type(), 5),
        SyntaxClass::Property => (theme::syntax_property(), 6),
        SyntaxClass::Constant => (theme::syntax_constant(), 7),
        SyntaxClass::Heading => (theme::syntax_heading(), 8),
        SyntaxClass::Link => (theme::syntax_link(), 9),
        SyntaxClass::Code => (theme::syntax_code(), 10),
        SyntaxClass::InlineCode => (theme::syntax_code(), 10),
        SyntaxClass::Emphasis => (theme::syntax_emphasis(), 11),
        SyntaxClass::List => (theme::syntax_list(), 12),
        SyntaxClass::Quote => (theme::syntax_quote(), 13),
        SyntaxClass::Table => (theme::syntax_table(), 14),
        SyntaxClass::CodeBlock => (theme::syntax_code(), 10),
    };
    let style = Style::default()
        .fg(color)
        .add_modifier(theme::syntax_modifier(index));
    if class == SyntaxClass::InlineCode {
        return style.bg(theme::code_block_bg());
    }
    style
}

fn clip_path_with_offset(path: &str, max_width: usize) -> (String, usize) {
    let width = path.width();
    if width <= max_width {
        return (path.to_owned(), 0);
    }
    if max_width == 0 {
        return (String::new(), path.len());
    }
    let mut suffix = String::new();
    let mut used = 0;
    for character in path.chars().rev() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width + 1 > max_width {
            break;
        }
        suffix.insert(0, character);
        used += character_width;
    }
    let offset = path.len() - suffix.len();
    (format!("…{suffix}"), offset)
}

#[cfg(test)]
fn clip_path(path: &str, max_width: usize) -> String {
    clip_path_with_offset(path, max_width).0
}

fn clipped_match_ranges(
    path: &str,
    clipped: &str,
    offset: usize,
    query: &str,
) -> Vec<(usize, usize)> {
    let ellipsis = clipped
        .len()
        .saturating_sub(path.len().saturating_sub(offset));
    fuzzy_match_ranges(path, query)
        .into_iter()
        .filter_map(|(start, end)| {
            let start = start.max(offset);
            let end = end.min(path.len());
            (end > start).then_some((start - offset + ellipsis, end - offset + ellipsis))
        })
        .collect()
}

fn fuzzy_match_ranges(path: &str, query: &str) -> Vec<(usize, usize)> {
    FileQuery::parse(query).highlight_ranges(path)
}

fn fuzzy_path_line(
    icons: &icon_theme::IconTheme,
    ascii: bool,
    path: &str,
    query: &str,
    max_width: usize,
    selected: bool,
) -> Line<'static> {
    let base = if selected {
        theme::selected()
    } else {
        theme::panel()
    };
    let icon = icon_theme::icon_for(path).unwrap_or_else(icon_theme::default_file_icon);
    let icon = icons
        .icon_text(icon, ascii)
        .map(|text| (text, (icon.color)()));
    let icon_width = icon
        .as_ref()
        .map(|(text, _)| UnicodeWidthStr::width(text.as_str()))
        .unwrap_or(0);
    let show_icon = icon_width > 0 && icon_width < max_width;
    let path_width = if show_icon {
        max_width.saturating_sub(icon_width)
    } else {
        max_width
    };
    let (clipped, offset) = clip_path_with_offset(path, path_width);
    let ranges = clipped_match_ranges(path, &clipped, offset, query);
    let mut spans = Vec::new();
    if show_icon {
        let (text, color) = icon.expect("checked icon");
        spans.push(Span::styled(text, base.fg(color)));
    }
    if ranges.is_empty() {
        if spans.is_empty() {
            return Line::styled(clipped, base);
        }
        spans.push(Span::styled(clipped, base));
        return Line::from(spans);
    }
    let match_style = base.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let mut cursor = 0;
    for (start, end) in ranges {
        if start > cursor {
            spans.push(Span::styled(clipped[cursor..start].to_owned(), base));
        }
        spans.push(Span::styled(clipped[start..end].to_owned(), match_style));
        cursor = end;
    }

    if cursor < clipped.len() {
        spans.push(Span::styled(clipped[cursor..].to_owned(), base));
    }
    Line::from(spans)
}

fn fuzzy_preview_lines(model: &Model) -> Vec<Line<'static>> {
    let preview = &model.overlays.preview;
    if preview.loading {
        return vec![Line::styled(
            "loading preview…",
            theme::panel().fg(theme::muted()),
        )];
    }
    if let Some(message) = &preview.message {
        return vec![Line::styled(
            message.clone(),
            theme::panel().fg(theme::error()),
        )];
    }
    let scroll = preview.scroll.min(preview.lines.len().saturating_sub(1));
    let highlights = preview.syntax.highlights();
    let mut offset: usize = preview
        .lines
        .iter()
        .take(scroll)
        .map(|line| line.len() + 1)
        .sum();
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (index, line) in preview.lines.iter().enumerate().skip(scroll) {
        let text = line.clone();
        let mut spans = vec![Span::styled(
            format!("{:>4} ", index + 1),
            theme::panel().fg(theme::muted()),
        )];
        let mut syntax_spans = Vec::new();
        push_syntax_spans(&mut syntax_spans, &text, offset, highlights);
        spans.extend(
            syntax_spans
                .into_iter()
                .map(|span| Span::styled(span.content.into_owned(), span.style)),
        );
        lines.push(Line::from(spans));
        offset += line.len() + 1;
    }
    if preview.truncated {
        lines.push(Line::styled(
            "… preview truncated",
            theme::panel().fg(theme::muted()),
        ));
    }
    lines
}
pub(crate) fn apply_code_block_background(model: &Model, lines: &mut [Line<'_>], width: u16) {
    let current = model.current_line();
    let target = usize::from(width).saturating_add(model.view.hscroll);
    for (offset, line) in lines.iter_mut().enumerate() {
        let line_number = model.view.scroll + offset;
        if !model.line_in_code_block(line_number) {
            continue;
        }
        let style = if current == Some(line_number) {
            theme::current_line()
        } else {
            theme::code_block()
        };
        let used = line.width();
        let mut rendered = std::mem::replace(line, Line::raw("")).style(style);
        if used < target {
            rendered
                .spans
                .push(Span::styled(" ".repeat(target - used), style));
        }
        *line = rendered;
    }
}

#[cfg(test)]
mod tests {
    use super::{clip_path, clip_path_with_offset, clipped_match_ranges, fuzzy_match_ranges};

    #[test]
    fn clips_paths_from_the_left() {
        assert_eq!(clip_path("crates/anvil-tui/src/lib.rs", 12), "…/src/lib.rs");
        assert_eq!(clip_path("short.rs", 20), "short.rs");
    }

    #[test]
    fn finds_case_insensitive_match_ranges() {
        assert_eq!(fuzzy_match_ranges("src/main.rs", "main"), vec![(4, 8)]);
        assert!(fuzzy_match_ranges("src/main.rs", "MAIN").is_empty());
        assert!(fuzzy_match_ranges("src/main.rs", "zzz").is_empty());
    }
    #[test]
    fn long_path_suffix_match_is_highlighted_in_the_clipped_suffix() {
        let path = "a/very/long/workspace/path/that/exceeds/the/picker/width/src/main.rs";
        let (clipped, offset) = clip_path_with_offset(path, 24);
        let ranges = clipped_match_ranges(path, &clipped, offset, "main.rs$");
        assert!(!ranges.is_empty());
        assert!(ranges.iter().all(|(_start, end)| *end <= clipped.len()));
    }
}

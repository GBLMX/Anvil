//! Ratatui rendering for the editor, tree, completion, and hover overlays.

use super::*;
use crate::syntax::{Highlight, SyntaxClass};
use crate::theme;
use anvil_api::EditorView;
use anvil_session::ui::CreateKind;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod editor;
pub use editor::view;
pub(crate) use editor::welcome_recent_at;

pub(crate) const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub(crate) fn panel_block(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border(false))
        .style(theme::panel())
        .title_style(theme::title())
        .title(title)
}

pub(crate) fn focused_panel_block(title: impl Into<Line<'static>>) -> Block<'static> {
    panel_block(title).border_style(theme::border(true))
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
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

fn buffer_bar_line(model: &Model, width: u16) -> Line<'static> {
    let width = usize::from(width);
    let mut tabs: Vec<(String, Style)> = model
        .buffers
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
            (format!(" {name}{marker} "), theme::panel())
        })
        .collect();
    let mut active = model.active.min(tabs.len().saturating_sub(1));
    if model.help_back.is_some() {
        let name = model.read_only_title.as_deref().unwrap_or("help");
        tabs.push((format!(" [{name}] RO "), theme::selected()));
        active = tabs.len() - 1;
    }
    if tabs.is_empty() {
        return Line::default();
    }
    tabs[active].1 = theme::selected();
    let tab_width = |index: usize| UnicodeWidthStr::width(tabs[index].0.as_str());
    let mut start = active;
    let mut end = active + 1;
    let mut used = tab_width(active);
    while used < width {
        let mut progressed = false;
        if end < tabs.len() {
            let next = tab_width(end) + 1;
            if used + next <= width {
                used += next;
                end += 1;
                progressed = true;
            }
        }
        if start > 0 {
            let next = tab_width(start - 1) + 1;
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
    let mut spans = Vec::new();
    if start > 0 {
        spans.push(Span::styled("‹", theme::panel().fg(theme::muted())));
    }
    for (index, (label, style)) in tabs.iter().enumerate().take(end).skip(start) {
        if index > start {
            spans.push(Span::styled("│", theme::panel().fg(theme::border_color())));
        }
        let label = if UnicodeWidthStr::width(label.as_str()) > width {
            crate::status_line::middle_ellipsis(label, width)
        } else {
            label.clone()
        };
        spans.push(Span::styled(label, *style));
    }
    if end < tabs.len() {
        spans.push(Span::styled("›", theme::panel().fg(theme::muted())));
    }
    Line::from(spans)
}

fn notification_lines(model: &Model) -> Vec<Line<'static>> {
    model
        .ui
        .notifications
        .items()
        .iter()
        .rev()
        .take(5)
        .map(|notification| {
            let (marker, color) = match notification.severity {
                anvil_session::ui::NotificationSeverity::Error => ("!", theme::error()),
                anvil_session::ui::NotificationSeverity::Warning => (
                    if icon_theme::ascii_mode() { "!" } else { "▲" },
                    theme::warning(),
                ),
                anvil_session::ui::NotificationSeverity::Info => ("i", theme::accent()),
            };
            let suffix = if notification.count > 1 {
                format!(" ×{}", notification.count)
            } else {
                String::new()
            };
            Line::styled(
                format!(
                    "{marker} {}: {}{suffix}",
                    notification.source, notification.message
                ),
                theme::panel().fg(color),
            )
        })
        .collect()
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

fn problem_lines(model: &Model, width: usize) -> (Vec<Line<'static>>, Option<usize>) {
    let mut lines = Vec::new();
    let mut selected_line = None;
    let mut current_path = None;
    for (index, problem) in model.ui.problems.items.iter().enumerate() {
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

pub(crate) fn utf16_to_byte(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (byte, character) in text.char_indices() {
        if seen >= units {
            return byte;
        }
        seen += character.len_utf16();
    }
    text.len()
}

pub(crate) fn display_column(text: &str) -> usize {
    let mut column = 0;
    for character in text.chars() {
        if character == '\t' {
            column += 4 - column % 4;
        } else if character == '\r' || character.is_control() {
        } else {
            column += character.width().unwrap_or(0);
        }
    }
    column
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
                let spaces = 4 - column % 4;
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

fn clip_path(path: &str, max_width: usize) -> String {
    let width = path.width();
    if width <= max_width {
        return path.to_owned();
    }
    if max_width == 0 {
        return String::new();
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
    format!("…{suffix}")
}

fn fuzzy_match_ranges(path: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() || !path.is_ascii() || !query.is_ascii() {
        return Vec::new();
    }
    let path_lower = path.to_ascii_lowercase();
    let query_lower = query.to_ascii_lowercase();
    let mut ranges = Vec::new();
    let mut offset = 0;
    while let Some(found) = path_lower[offset..].find(&query_lower) {
        let start = offset + found;
        let end = start + query_lower.len();
        ranges.push((start, end));
        offset = end;
    }
    ranges
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
    let clipped = clip_path(path, path_width);
    let ranges = fuzzy_match_ranges(&clipped, query);
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
    let preview = &model.preview;
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
    let mut lines: Vec<Line<'static>> = preview
        .lines
        .iter()
        .map(|line| Line::styled(line.clone(), theme::panel()))
        .collect();
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
    let target = usize::from(width).saturating_add(model.hscroll);
    for (offset, line) in lines.iter_mut().enumerate() {
        let line_number = model.scroll + offset;
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
    use super::{clip_path, fuzzy_match_ranges};

    #[test]
    fn clips_paths_from_the_left() {
        assert_eq!(clip_path("crates/anvil-tui/src/lib.rs", 12), "…/src/lib.rs");
        assert_eq!(clip_path("short.rs", 20), "short.rs");
    }

    #[test]
    fn finds_case_insensitive_match_ranges() {
        assert_eq!(fuzzy_match_ranges("src/main.rs", "MAIN"), vec![(4, 8)]);
        assert!(fuzzy_match_ranges("src/main.rs", "zzz").is_empty());
    }
}

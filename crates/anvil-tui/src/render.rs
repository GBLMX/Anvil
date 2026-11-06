//! Ratatui rendering for the editor, tree, completion, and hover overlays.

use super::*;
use crate::syntax::{Highlight, SyntaxClass};
use crate::theme;
use anvil_api::EditorView;
use anvil_session::ui::CreateKind;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(crate) const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

fn panel_block(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border(false))
        .style(theme::panel())
        .title_style(theme::title())
        .title(title)
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
        tabs.push((" [help] RO ".to_owned(), theme::selected()));
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
        spans.push(Span::styled("‹", theme::panel().fg(theme::MUTED)));
    }
    for (index, (label, style)) in tabs.iter().enumerate().take(end).skip(start) {
        if index > start {
            spans.push(Span::styled("│", theme::panel().fg(theme::BORDER)));
        }
        let label = if UnicodeWidthStr::width(label.as_str()) > width {
            crate::status_line::middle_ellipsis(label, width)
        } else {
            label.clone()
        };
        spans.push(Span::styled(label, *style));
    }
    if end < tabs.len() {
        spans.push(Span::styled("›", theme::panel().fg(theme::MUTED)));
    }
    Line::from(spans)
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
                    .fg(theme::ERROR)
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

pub(crate) fn expand_tabs(spans: &mut Vec<Span<'_>>) {
    let mut column = 0;
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
        let start = highlight.start.max(base).max(cursor);
        let end = highlight.end.min(chunk_end);
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

/// Applies search backgrounds without replacing existing syntax foregrounds.
pub(crate) fn apply_search_highlights<'a>(
    spans: &mut Vec<Span<'a>>,
    line_start: usize,
    ranges: &[(usize, usize)],
    active_start: usize,
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
        for &(start, end) in ranges {
            if end <= span_start {
                continue;
            }
            if start >= span_end {
                break;
            }
            let match_start = start.max(span_start);
            let match_end = end.min(span_end);
            if match_start > cursor {
                let from = cursor - span_start;
                let to = match_start - span_start;
                result.push(Span::styled(content[from..to].to_owned(), span.style));
            }
            let overlay = if start == active_start {
                theme::search_active()
            } else {
                theme::search_match()
            };
            let from = match_start - span_start;
            let to = match_end - span_start;
            result.push(Span::styled(
                content[from..to].to_owned(),
                span.style.patch(overlay),
            ));
            cursor = match_end;
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
    let color = match class {
        SyntaxClass::Keyword => theme::SYNTAX_KEYWORD,
        SyntaxClass::String => theme::SYNTAX_STRING,
        SyntaxClass::Comment => theme::SYNTAX_COMMENT,
        SyntaxClass::Number => theme::SYNTAX_NUMBER,
        SyntaxClass::Function => theme::SYNTAX_FUNCTION,
        SyntaxClass::Type => theme::SYNTAX_TYPE,
        SyntaxClass::Property => theme::SYNTAX_PROPERTY,
        SyntaxClass::Constant => theme::SYNTAX_CONSTANT,
        SyntaxClass::Heading => theme::SYNTAX_HEADING,
        SyntaxClass::Link => theme::SYNTAX_LINK,
        SyntaxClass::Code => theme::SYNTAX_CODE,
        SyntaxClass::InlineCode => theme::SYNTAX_CODE,
        SyntaxClass::Emphasis => theme::SYNTAX_EMPHASIS,
        SyntaxClass::List => theme::SYNTAX_LIST,
        SyntaxClass::Quote => theme::SYNTAX_QUOTE,
        SyntaxClass::Table => theme::SYNTAX_TABLE,
        SyntaxClass::CodeBlock => theme::SYNTAX_CODE,
    };
    if class == SyntaxClass::InlineCode {
        return Style::default().fg(color).bg(theme::CODE_BLOCK_BG);
    }
    Style::default().fg(color)
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

fn fuzzy_path_line(path: &str, query: &str, max_width: usize, selected: bool) -> Line<'static> {
    let base = if selected {
        theme::selected()
    } else {
        theme::panel()
    };
    let icon = icon_theme::icon_for(path)
        .and_then(|icon| icon_theme::icon_text(icon).map(|text| (text, icon.color)));
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
            theme::panel().fg(theme::MUTED),
        )];
    }
    if let Some(message) = &preview.message {
        return vec![Line::styled(
            message.clone(),
            theme::panel().fg(theme::ERROR),
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
            theme::panel().fg(theme::MUTED),
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

pub fn view(model: &Model, frame: &mut Frame) {
    let area = frame.area();
    let editor_view: EditorView = model.editor.snapshot().view();
    let title = if model.help_back.is_some() {
        "[help]".to_owned()
    } else {
        let name = model.breadcrumb_text(usize::from(area.width).saturating_sub(6));
        let name = if model.modified {
            format!("[+] {name}")
        } else {
            name
        };
        if model.buffers.is_empty() {
            name
        } else {
            format!("[{}/{}] {name}", model.active + 1, model.buffers.len())
        }
    };
    let debug_revision = std::env::var_os("ANVIL_DEBUG_REV").is_some();
    let title = if debug_revision {
        format!("{title} · rev {}", editor_view.revision)
    } else {
        title
    };
    let (mode_text, mode_color, status) = match &model.ui.input_mode {
        InputMode::Rename { name } => (" REN ", theme::MAGENTA, format!("rename> {name}")),
        InputMode::CommandLine { buffer } => (" CMD ", theme::WARNING, format!(":{buffer}")),
        InputMode::Search { query } => (" SRC ", theme::WARNING, format!("/{query}")),
        InputMode::Insert => (" INS ", theme::SUCCESS, String::new()),
        InputMode::Create { name, kind, .. } => (
            if matches!(kind, CreateKind::Directory) {
                " DIR "
            } else {
                " NEW "
            },
            theme::ACCENT,
            format!("new> {name}"),
        ),
        InputMode::RenameEntry { name, .. } => (" REN ", theme::MAGENTA, format!("rename> {name}")),
        InputMode::TransferEntry {
            copy, directory, ..
        } => (
            if *copy { " CPY " } else { " MOV " },
            theme::ACCENT,
            format!("{}>{directory}", if *copy { "copy" } else { "move" }),
        ),
        InputMode::ConfirmClose { index } => (
            " CLS ",
            theme::WARNING,
            format!("Save {} before closing? [Y/n/c]", model.buffer_name(*index)),
        ),
        InputMode::ConfirmReload { path } => (
            " REL ",
            theme::WARNING,
            format!("File changed on disk: {} [y/N]", path.display()),
        ),
        InputMode::ConfirmDelete { path } => (
            " DEL ",
            theme::ERROR,
            format!(
                "delete {}? (y/N)",
                path.file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default()
            ),
        ),
        InputMode::Normal => (
            " NOR ",
            theme::ACCENT,
            model.status_text(usize::from(area.width).saturating_sub(7)),
        ),
    };
    let status = if let Some(busy) = &model.busy {
        format!("{} {busy}…", SPINNER[model.spinner % SPINNER.len()])
    } else {
        status
    };
    let mode = Span::styled(mode_text, theme::mode_badge(mode_color));
    let block = panel_block(title)
        .title_style(theme::title())
        .title_bottom(Line::from(vec![
            mode,
            Span::styled(status, theme::status()),
        ]));
    frame.render_widget(block, area);
    let mut inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    if !model.welcome && (model.buffers.len() > 1 || model.help_back.is_some()) {
        let bar = Rect {
            height: inner.height.min(1),
            ..inner
        };
        frame.render_widget(
            Paragraph::new(buffer_bar_line(model, bar.width)).style(theme::panel()),
            bar,
        );
        inner.y = inner.y.saturating_add(1);
        inner.height = inner.height.saturating_sub(1);
    }
    let mut content = inner;
    if model.welcome {
        render_welcome(frame, inner, model);
        return;
    }
    if model.tree_visible && inner.width > 50 {
        let sidebar_width = (inner.width / 5).clamp(18, 40).min(inner.width - 20);
        let tree_area = Rect {
            width: sidebar_width,
            ..inner
        };
        let mut lines = Vec::new();
        let visible_height = tree_area.height.saturating_sub(2) as usize;
        let start = model
            .tree_selected
            .unwrap_or(0)
            .saturating_sub(visible_height.saturating_sub(1));
        for (index, entry) in model
            .tree
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_height)
        {
            let name = entry
                .path
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_default();
            let marker = if entry.is_dir {
                if entry.expanded { "v " } else { "> " }
            } else {
                "  "
            };
            let text = format!("{}{}{}", "  ".repeat(entry.depth), marker, name);
            let style = if model.tree_selected == Some(index) {
                theme::selected()
            } else if entry.is_dir {
                Style::default().fg(theme::ACCENT).bg(theme::SURFACE)
            } else {
                theme::panel()
            };
            lines.push(Line::styled(text, style));
        }
        if lines.is_empty() {
            lines.push(Line::styled("empty", theme::panel().fg(theme::MUTED)));
        }
        let tree_title = if model.tree_stale {
            "Files · stale"
        } else if model.tree_watch_limited {
            "Files · watch limit"
        } else if model.show_hidden {
            "Files · hidden"
        } else {
            "Files"
        };
        let mut tree_block = panel_block(tree_title);
        if model.ui.focus.tree {
            tree_block = tree_block.border_style(theme::border(true));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(tree_block),
            tree_area,
        );
        content = Rect {
            x: inner.x + sidebar_width,
            width: inner.width.saturating_sub(sidebar_width),
            ..inner
        };
    }
    let gutter_width = model.gutter_width().min(content.width);
    let gutter_area = Rect {
        width: gutter_width,
        ..content
    };
    let text_area = Rect {
        x: content.x + gutter_width,
        width: content.width.saturating_sub(gutter_width),
        ..content
    };
    frame.render_widget(
        Paragraph::new(model.visible_gutter(inner.height)).style(theme::text()),
        gutter_area,
    );
    let mut lines = model.visible_lines(inner.height);
    apply_code_block_background(model, &mut lines, text_area.width);
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::text())
            .scroll((0, u16::try_from(model.hscroll).unwrap_or(u16::MAX))),
        text_area,
    );
    let visible_end =
        (model.scroll + usize::from(inner.height)).min(model.editor.index().len_lines());
    for line_number in model.scroll..visible_end {
        let Some(start) = model.editor.index().line_to_offset(line_number) else {
            continue;
        };
        let Some(line) = model.editor.index().offset_to_line(start) else {
            continue;
        };
        let slice = model.editor.text().slice(start..line.end);
        let mut columns = 0;
        for character in slice.chars() {
            match character {
                ' ' => columns += 1,
                '\t' => columns += 4 - columns % 4,
                _ => break,
            }
        }
        let style = if model.current_line() == Some(line_number) {
            Style::default().fg(theme::MUTED)
        } else {
            Style::default().fg(theme::DIM)
        };
        for level in 1..=columns / 4 {
            let column = level * 4;
            if column >= columns || column < model.hscroll {
                continue;
            }
            let x = text_area.x + (column - model.hscroll) as u16;
            let y = inner.y + (line_number - model.scroll) as u16;
            if x < text_area.right() && y < text_area.bottom() {
                frame.render_widget(
                    Paragraph::new("│").style(style),
                    Rect {
                        x,
                        y,
                        width: 1,
                        height: 1,
                    },
                );
            }
        }
    }
    if let Some(prefix) = model.pending {
        let entries = model.pending_hints();
        if !entries.is_empty() {
            let available = usize::from(area.width.saturating_sub(2)).max(1);
            let width = entries
                .iter()
                .map(|(key, action)| key.len() + action.len() + 5)
                .max()
                .unwrap_or(24)
                .clamp(20.min(available), available) as u16;
            let height = (entries.len() as u16 + 2).min(area.height.saturating_sub(2));
            let popup = Rect {
                x: area.x + area.width.saturating_sub(width + 2),
                y: area.y + area.height.saturating_sub(height + 2),
                width,
                height,
            };
            frame.render_widget(Clear, popup);
            let lines = entries
                .iter()
                .map(|(key, action)| {
                    Line::from(vec![
                        Span::styled(format!(" {key:<2}"), Style::default().fg(theme::ACCENT)),
                        Span::raw(format!(" {action}")),
                    ])
                })
                .collect::<Vec<_>>();
            let title = if prefix == ' ' {
                "space".to_owned()
            } else {
                prefix.to_string()
            };
            frame.render_widget(
                Paragraph::new(lines)
                    .style(theme::panel())
                    .block(panel_block(title)),
                popup,
            );
        }
    }
    if let Some(position) = model
        .editor
        .index()
        .position(model.editor.text(), model.cursor)
    {
        let x = text_area.x + position.display_column.saturating_sub(model.hscroll) as u16;
        let y = inner.y + position.line.saturating_sub(model.scroll) as u16;
        if x < inner.right() && y < inner.bottom() {
            frame.set_cursor_position((x, y));
        }
    }
    for selection in editor_view.selections.iter().skip(1) {
        let line = selection.head.line as usize;
        let column = selection.head.display_column as usize;
        if line < model.scroll || column < model.hscroll {
            continue;
        }
        let x = text_area.x + (column - model.hscroll) as u16;
        let y = inner.y + (line - model.scroll) as u16;
        if x < text_area.right() && y < text_area.bottom() {
            frame.render_widget(
                Paragraph::new("│").style(
                    Style::default()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::DIM),
                ),
                Rect {
                    x,
                    y,
                    width: 1,
                    height: 1,
                },
            );
        }
    }
    if let Some(split) = &model.split {
        let half = inner.width / 2;
        let split_area = if model.ui.focus.split {
            Rect {
                width: half,
                ..inner
            }
        } else {
            Rect {
                x: inner.x + half,
                width: inner.width.saturating_sub(half),
                ..inner
            }
        };
        render_split_pane(frame, split_area, &split.buffer, model.ui.focus.split);
    }
    if let Some(palette) = &model.command_palette {
        let width = (area.width * 3 / 5)
            .max(40)
            .min(area.width.saturating_sub(2));
        let height = (area.height * 3 / 5)
            .max(10)
            .min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        let visible = height.saturating_sub(3) as usize;
        let start = palette.selected.saturating_sub(visible.saturating_sub(1));
        let mut lines = vec![Line::styled(
            format!("> {}", palette.query),
            Style::default().fg(theme::ACCENT),
        )];
        for (row, index) in palette
            .filtered
            .iter()
            .skip(start)
            .take(visible)
            .enumerate()
        {
            let style = if start + row == palette.selected {
                theme::selected()
            } else {
                theme::panel()
            };
            lines.push(Line::styled(palette.items[*index].label.clone(), style));
        }
        if palette.filtered.is_empty() {
            lines.push(Line::styled("no commands", theme::panel().fg(theme::MUTED)));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(panel_block("Commands")),
            popup,
        );
    }

    if let Some(fuzzy) = &model.fuzzy {
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
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
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
        let mut lines = Vec::new();
        let visible = list_area.height.saturating_sub(3) as usize;
        let start = fuzzy.selected.saturating_sub(visible.saturating_sub(1));
        for (row, index) in fuzzy.filtered.iter().skip(start).take(visible).enumerate() {
            let name = &fuzzy.entries[*index];
            lines.push(fuzzy_path_line(
                name,
                &fuzzy.query,
                list_area.width.saturating_sub(2) as usize,
                start + row == fuzzy.selected,
            ));
        }
        lines.push(Line::styled(
            format!("> {}", fuzzy.query),
            Style::default().fg(theme::ACCENT).bg(theme::SURFACE),
        ));
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(panel_block(if model.show_hidden {
                    "Files · hidden"
                } else {
                    "Files"
                })),
            list_area,
        );
        let preview_area = if horizontal {
            Some(Rect {
                x: popup.x + list_width,
                width: popup.width - list_width,
                ..popup
            })
        } else if stacked {
            Some(Rect {
                x: popup.x,
                y: popup.y + list_height,
                width: popup.width,
                height: popup.height - list_height,
            })
        } else {
            None
        };
        if let Some(preview_area) = preview_area {
            frame.render_widget(
                Paragraph::new(fuzzy_preview_lines(model))
                    .style(theme::panel())
                    .block(panel_block("Preview")),
                preview_area,
            );
        }
    }
    if let Some(diagnostic) = &model.diagnostic_popup {
        let width = (area.width * 3 / 4)
            .max(30)
            .min(area.width.saturating_sub(2));
        let height = 6_u16.min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(diagnostic.as_str())
                .style(theme::panel())
                .block(panel_block("Diagnostic")),
            popup,
        );
    }
    if let Some(actions) = &model.code_actions {
        let width = (area.width * 3 / 5)
            .max(36)
            .min(area.width.saturating_sub(2));
        let height = (actions.items.len() as u16 + 2)
            .min(14)
            .min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        let mut lines = Vec::new();
        for (row, item) in actions
            .items
            .iter()
            .take(height.saturating_sub(2) as usize)
            .enumerate()
        {
            let text = if let Some(reason) = &item.disabled {
                format!("{}  [{}]", item.title, reason)
            } else {
                item.title.clone()
            };
            let style = if row == actions.selected {
                theme::selected()
            } else if item.disabled.is_some() {
                theme::panel().fg(theme::MUTED)
            } else {
                theme::panel()
            };
            lines.push(Line::styled(text, style));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(panel_block("Code Actions")),
            popup,
        );
    }
    if let Some(symbols) = &model.document_symbols {
        let width = (area.width * 3 / 5)
            .max(36)
            .min(area.width.saturating_sub(2));
        let height = (symbols.items.len() as u16 + 2)
            .min(16)
            .min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        let visible = height.saturating_sub(2) as usize;
        let start = symbols.selected.saturating_sub(visible.saturating_sub(1));
        let mut lines = Vec::new();
        for (row, item) in symbols.items.iter().skip(start).take(visible).enumerate() {
            let indent = "  ".repeat(item.depth.min(8));
            let text = if let Some(detail) = &item.detail {
                format!("{indent}{}  {detail}", item.name)
            } else {
                format!("{indent}{}", item.name)
            };
            let style = if start + row == symbols.selected {
                theme::selected()
            } else {
                theme::panel()
            };
            lines.push(Line::styled(text, style));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(panel_block("Document Symbols")),
            popup,
        );
    }

    if let Some(signature) = &model.signature_help {
        let item = &signature.signatures[signature.active_signature];
        let width = (area.width * 3 / 5)
            .max(40)
            .min(area.width.saturating_sub(2));
        let height = 7.min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + area.height.saturating_sub(height + 2),
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        let mut lines = vec![Line::styled(
            item.label.clone(),
            Style::default().fg(theme::ACCENT).bg(theme::SURFACE),
        )];
        if !item.documentation.is_empty() {
            lines.push(Line::raw(item.documentation.clone()));
        }
        lines.push(Line::styled(
            format!("parameter {}", signature.active_parameter + 1),
            Style::default().fg(theme::MUTED).bg(theme::SURFACE),
        ));
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(panel_block("Signature")),
            popup,
        );
    }
    if let Some(symbols) = &model.workspace_symbols {
        let width = (area.width * 3 / 5)
            .max(40)
            .min(area.width.saturating_sub(2));
        let height = (symbols.filtered.len() as u16 + 3)
            .min(16)
            .min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        let visible = height.saturating_sub(3) as usize;
        let start = symbols.selected.saturating_sub(visible.saturating_sub(1));
        let mut lines = vec![Line::styled(
            format!("> {}", symbols.query),
            Style::default().fg(theme::ACCENT).bg(theme::SURFACE),
        )];
        for (row, index) in symbols
            .filtered
            .iter()
            .skip(start)
            .take(visible)
            .enumerate()
        {
            let item = &symbols.items[*index];
            let detail = if item.detail.is_empty() {
                item.path.display().to_string()
            } else {
                format!("{} · {}", item.detail, item.path.display())
            };
            let style = if start + row == symbols.selected {
                theme::selected()
            } else {
                theme::panel()
            };
            lines.push(Line::styled(format!("{}  {detail}", item.name), style));
        }
        if lines.len() == 1 {
            lines.push(Line::styled(
                "no symbols",
                Style::default().fg(theme::MUTED).bg(theme::SURFACE),
            ));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(panel_block("Workspace Symbols")),
            popup,
        );
    }
    if let Some(completion) = &model.completion {
        let width = (area.width * 2 / 5)
            .max(36)
            .min(area.width.saturating_sub(2));
        let height = (completion.filtered.len() as u16 + 2)
            .min(14)
            .min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        let mut lines = Vec::new();
        for (row, index) in completion
            .filtered
            .iter()
            .take(height.saturating_sub(2) as usize)
            .enumerate()
        {
            let item = &completion.items[*index];
            let text = if item.detail.is_empty() {
                item.label.clone()
            } else {
                format!("{}  {}", item.label, item.detail)
            };
            let style = if row == completion.selected {
                theme::selected()
            } else {
                theme::panel()
            };
            lines.push(Line::styled(text, style));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(panel_block(format!("Completion #{}", completion.id))),
            popup,
        );
    }
    if let Some(hover) = &model.hover {
        let width = (area.width * 3 / 5)
            .max(30)
            .min(area.width.saturating_sub(2));
        let height = (hover.lines().count() as u16 + 2)
            .min(14)
            .min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(hover.as_str())
                .style(theme::panel())
                .block(panel_block("Hover")),
            popup,
        );
    }
}

fn render_welcome(frame: &mut Frame, area: Rect, model: &Model) {
    let width = area.width.saturating_sub(4).clamp(12, 64).min(area.width);
    let height = area.height.saturating_sub(2).clamp(4, 18).min(area.height);
    let panel = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, panel);
    let mut lines = vec![
        Line::from(vec![Span::styled(
            "ANVIL",
            Style::default()
                .fg(theme::ACCENT)
                .bg(theme::SURFACE)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::styled(
            "selection-first terminal editor",
            Style::default().fg(theme::MUTED).bg(theme::SURFACE),
        ),
        Line::raw(""),
        Line::from(vec![
            Span::styled("i", Style::default().fg(theme::ACCENT)),
            Span::raw("      insert"),
        ]),
        Line::from(vec![
            Span::styled("Space f", Style::default().fg(theme::ACCENT)),
            Span::raw(" find file"),
        ]),
        Line::from(vec![
            Span::styled("Space e", Style::default().fg(theme::ACCENT)),
            Span::raw(" file tree"),
        ]),
        Line::from(vec![
            Span::styled("?", Style::default().fg(theme::ACCENT)),
            Span::raw("      help"),
        ]),
        Line::from(vec![
            Span::styled(":q", Style::default().fg(theme::ACCENT)),
            Span::raw("      quit"),
        ]),
        Line::raw(""),
        Line::styled(
            format!("workspace  {}", model.workspace.display()),
            Style::default().fg(theme::MUTED).bg(theme::SURFACE),
        ),
        Line::styled(
            format!("version    {}", env!("CARGO_PKG_VERSION")),
            Style::default().fg(theme::MUTED).bg(theme::SURFACE),
        ),
    ];
    lines.truncate(usize::from(height.saturating_sub(2)));
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::panel())
            .alignment(Alignment::Center)
            .block(panel_block(" start ")),
        panel,
    );
}

fn render_split_pane(frame: &mut Frame, area: Rect, buffer: &Buffer, is_active: bool) {
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
                Style::default().fg(theme::MUTED),
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

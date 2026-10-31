//! Ratatui rendering for the editor, tree, completion, and hover overlays.

use super::*;
use crate::syntax::{Highlight, SyntaxClass};
use anvil_api::EditorView;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
                span.style.fg(Color::Red).add_modifier(Modifier::UNDERLINED)
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

fn syntax_style(class: SyntaxClass) -> Style {
    let color = match class {
        SyntaxClass::Keyword => Color::Magenta,
        SyntaxClass::String => Color::Green,
        SyntaxClass::Comment => Color::DarkGray,
        SyntaxClass::Number => Color::Yellow,
        SyntaxClass::Function => Color::Blue,
        SyntaxClass::Type => Color::Cyan,
        SyntaxClass::Property => Color::LightCyan,
        SyntaxClass::Constant => Color::Yellow,
    };
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
    let clipped = clip_path(path, max_width);
    let base = if selected {
        Style::default().fg(Color::Black).bg(Color::Yellow)
    } else {
        Style::default()
    };
    let ranges = fuzzy_match_ranges(&clipped, query);
    if ranges.is_empty() {
        return Line::styled(clipped, base);
    }
    let match_style = base.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let mut spans = Vec::new();
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

pub fn view(model: &Model, frame: &mut Frame) {
    let area = frame.area();
    let editor_view: EditorView = model.editor.snapshot().view();
    let title = if model.help_back.is_some() {
        "[help]".to_owned()
    } else {
        let name = model
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "[no file]".to_owned());
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
    let (mode_text, mode_color, status) = match &model.input_mode {
        InputMode::Rename { name } => (" REN ", Color::Magenta, format!("rename> {name}")),
        InputMode::CommandLine { buffer } => (" CMD ", Color::Yellow, format!(":{buffer}")),
        InputMode::Search { query } => (" SRC ", Color::Yellow, format!("/{query}")),
        InputMode::Insert => (" INS ", Color::Green, String::new()),
        InputMode::Normal => {
            let position = model
                .editor
                .index()
                .position(model.editor.text(), model.cursor);
            let location = match position {
                Some(position) => format!(
                    " Ln {}, Col {} ",
                    position.line + 1,
                    position.grapheme_column + 1
                ),
                None => " Ln 1, Col 1 ".to_owned(),
            };
            let status = if model.status.is_empty() {
                location
            } else {
                format!("{} ·{location}", model.status)
            };
            (" NOR ", Color::Blue, status)
        }
    };
    let mode = Span::styled(mode_text, Style::default().fg(Color::Black).bg(mode_color));
    let block = Block::bordered()
        .title(title)
        .title_bottom(Line::from(vec![mode, Span::raw(status)]));
    frame.render_widget(block, area);
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
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
        for (index, entry) in model.tree.iter().enumerate() {
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
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else if entry.is_dir {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            lines.push(Line::styled(text, style));
        }
        if lines.is_empty() {
            lines.push(Line::raw("empty"));
        }
        let tree_title = if model.show_hidden {
            "Files · hidden"
        } else {
            "Files"
        };
        let mut tree_block = Block::bordered().title(tree_title);
        if model.tree_focus {
            tree_block = tree_block.border_style(Style::default().fg(Color::Yellow));
        }
        frame.render_widget(Paragraph::new(lines).block(tree_block), tree_area);
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
        Paragraph::new(model.visible_gutter(inner.height)),
        gutter_area,
    );
    frame.render_widget(
        Paragraph::new(model.visible_lines(inner.height))
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
            Style::default().fg(Color::Indexed(244))
        } else {
            Style::default().fg(Color::DarkGray)
        };
        for level in 1..=columns / 4 {
            let column = level * 4;
            if column < model.hscroll {
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
    if model.pending == Some(' ') {
        let entries = [
            ("e", "file tree"),
            ("f", "fuzzy files"),
            ("r", "rename"),
            ("a", "code actions"),
            ("s", "symbols"),
            ("S", "workspace symbols"),
            ("F", "format"),
        ];
        let width = 30.min(area.width.saturating_sub(4));
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
            .map(|(key, description)| {
                Line::from(vec![
                    Span::styled(format!(" {key:<2}"), Style::default().fg(Color::Yellow)),
                    Span::raw(format!(" {description}")),
                ])
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title("Space")),
            popup,
        );
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
                Paragraph::new("│")
                    .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM)),
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
        let split_area = if model.split_active {
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
        render_split_pane(frame, split_area, &split.buffer, model.split_active);
    }
    if let Some(fuzzy) = &model.fuzzy {
        let width = (area.width * 3 / 5)
            .max(30)
            .min(area.width.saturating_sub(2));
        let height = (area.height / 2).max(8).min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        let mut lines = Vec::new();
        for (row, index) in fuzzy
            .filtered
            .iter()
            .take(height.saturating_sub(3) as usize)
            .enumerate()
        {
            let name = &fuzzy.entries[*index];
            lines.push(fuzzy_path_line(
                name,
                &fuzzy.query,
                width.saturating_sub(2) as usize,
                row == fuzzy.selected,
            ));
        }
        lines.push(Line::raw(format!("> {}", fuzzy.query)));
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(if model.show_hidden {
                "Files · hidden"
            } else {
                "Files"
            })),
            popup,
        );
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
            Paragraph::new(diagnostic.as_str()).block(Block::bordered().title("Diagnostic")),
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
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default()
            };
            lines.push(Line::styled(text, style));
        }
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title("Code Actions")),
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
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default()
            };
            lines.push(Line::styled(text, style));
        }
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title("Document Symbols")),
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
            Style::default().fg(Color::Cyan),
        )];
        if !item.documentation.is_empty() {
            lines.push(Line::raw(item.documentation.clone()));
        }
        lines.push(Line::styled(
            format!("parameter {}", signature.active_parameter + 1),
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title("Signature")),
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
            Style::default().fg(Color::Cyan),
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
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default()
            };
            lines.push(Line::styled(format!("{}  {detail}", item.name), style));
        }
        if lines.len() == 1 {
            lines.push(Line::styled(
                "no symbols",
                Style::default().fg(Color::DarkGray),
            ));
        }
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title("Workspace Symbols")),
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
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default()
            };
            lines.push(Line::styled(text, style));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().title(format!("Completion #{}", completion.id))),
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
            Paragraph::new(hover.as_str()).block(Block::bordered().title("Hover")),
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
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::raw("selection-first terminal editor"),
        Line::raw(""),
        Line::from(vec![
            Span::styled("i", Style::default().fg(Color::Cyan)),
            Span::raw("      insert"),
        ]),
        Line::from(vec![
            Span::styled("Space f", Style::default().fg(Color::Cyan)),
            Span::raw(" find file"),
        ]),
        Line::from(vec![
            Span::styled("Space e", Style::default().fg(Color::Cyan)),
            Span::raw(" file tree"),
        ]),
        Line::from(vec![
            Span::styled("?", Style::default().fg(Color::Cyan)),
            Span::raw("      help"),
        ]),
        Line::from(vec![
            Span::styled(":q", Style::default().fg(Color::Cyan)),
            Span::raw("      quit"),
        ]),
        Line::raw(""),
        Line::raw(format!("workspace  {}", model.workspace.display())),
        Line::raw(format!("version    {}", env!("CARGO_PKG_VERSION"))),
    ];
    lines.truncate(usize::from(height.saturating_sub(2)));
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(Block::bordered().title(" start ")),
        panel,
    );
}

fn render_split_pane(frame: &mut Frame, area: Rect, buffer: &Buffer, is_active: bool) {
    let block = Block::bordered()
        .title(
            buffer
                .path
                .as_ref()
                .map_or("[no file]".to_owned(), |path| path.display().to_string()),
        )
        .border_style(if is_active {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        });
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
        lines.push(Line::raw(format!(
            "{:>digits$} {text}",
            line_number + 1,
            digits = digits
        )));
    }
    if lines.is_empty() {
        lines.push(Line::raw(""));
    }
    frame.render_widget(
        Paragraph::new(lines).scroll((0, u16::try_from(buffer.hscroll).unwrap_or(u16::MAX))),
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

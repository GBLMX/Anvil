//! Ratatui rendering for the editor, tree, completion, and hover overlays.

use super::*;
use crate::syntax::{Highlight, SyntaxClass};
use anvil_api::EditorView;
use ratatui::{
    Frame,
    layout::Rect,
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
        let mut tree_block = Block::bordered().title("Files");
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
            let style = if row == fuzzy.selected {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default()
            };
            lines.push(Line::styled(name.as_str(), style));
        }
        lines.push(Line::raw(format!("> {}", fuzzy.query)));
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title("Files")),
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

//! Main editor frame and overlays.

use super::*;
use crate::CallHierarchyDirection;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
pub fn view(model: &Model, frame: &mut Frame) {
    let area = frame.area();
    let editor_view: EditorView = model.editor.snapshot().view();
    let title = if model.help_back.is_some() {
        format!("[{}]", model.read_only_title.as_deref().unwrap_or("help"))
    } else {
        let name = if model.stdin_buffer {
            "[stdin]".to_owned()
        } else {
            model.breadcrumb_text(usize::from(area.width).saturating_sub(6))
        };
        let name = if model.modified {
            format!("[+] {name}")
        } else {
            name
        };
        let name = if model.read_only {
            format!("{name} [RO]")
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
        InputMode::Rename { name } => (" REN ", theme::magenta(), format!("rename> {name}")),
        InputMode::CommandLine { buffer } => {
            (" CMD ", theme::warning(), format!(":{}", buffer.text()))
        }
        InputMode::Search { query } => {
            (" SRC ", theme::warning(), model.search_status(query.text()))
        }
        InputMode::Insert => (" INS ", theme::success(), String::new()),
        InputMode::Create { name, kind, .. } => (
            if matches!(kind, CreateKind::Directory) {
                " DIR "
            } else {
                " NEW "
            },
            theme::accent(),
            format!("new> {name}"),
        ),
        InputMode::RenameEntry { name, .. } => {
            (" REN ", theme::magenta(), format!("rename> {name}"))
        }
        InputMode::TransferEntry {
            copy, directory, ..
        } => (
            if *copy { " CPY " } else { " MOV " },
            theme::accent(),
            format!("{}>{directory}", if *copy { "copy" } else { "move" }),
        ),
        InputMode::ConfirmClose { index } => (
            " CLS ",
            theme::warning(),
            format!("Save {} before closing? [Y/n/c]", model.buffer_name(*index)),
        ),
        InputMode::ConfirmReload { path } => (
            " REL ",
            theme::warning(),
            format!("File changed on disk: {} [y/N]", path.display()),
        ),
        InputMode::ConfirmDelete { path } => (
            " DEL ",
            theme::error(),
            format!(
                "delete {}? (y/N)",
                path.file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default()
            ),
        ),
        InputMode::Normal => (
            " NOR ",
            theme::accent(),
            model.status_text(usize::from(area.width).saturating_sub(7)),
        ),
    };
    let status = if let Some(busy) = &model.busy {
        format!("{} {busy}…", SPINNER[model.spinner % SPINNER.len()])
    } else {
        status
    };

    let status_cursor = match &model.ui.input_mode {
        InputMode::CommandLine { buffer } => Some((1, buffer.cursor(), buffer.text())),
        InputMode::Search { query } => Some((1, query.cursor(), query.text())),
        _ => None,
    }
    .map(|(prefix, cursor, text)| {
        let before = text.get(..cursor).unwrap_or(text);
        let x = area
            .x
            .saturating_add(1)
            .saturating_add(UnicodeWidthStr::width(mode_text) as u16)
            .saturating_add(prefix)
            .saturating_add(UnicodeWidthStr::width(before) as u16);
        (x, area.y.saturating_add(area.height.saturating_sub(1)))
    });
    let mode = Span::styled(mode_text, theme::mode_badge(mode_color));
    let block = panel_block(title)
        .title_style(theme::title())
        .title_bottom(Line::from(vec![
            mode,
            Span::styled(status, theme::status()),
        ]));
    frame.render_widget(block, area);
    if let Some((x, y)) = status_cursor
        && x < area.right()
        && y < area.bottom()
    {
        frame.set_cursor_position((x, y));
    }
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
                Style::default().fg(theme::accent()).bg(theme::surface())
            } else {
                theme::panel()
            };
            lines.push(Line::styled(text, style));
        }
        if lines.is_empty() {
            lines.push(Line::styled("empty", theme::panel().fg(theme::muted())));
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
    let locations_area = crate::pipe_locations::panel_rect(model, content);
    if let Some(area) = locations_area {
        content.height = area.y.saturating_sub(content.y);
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
        Paragraph::new(model.visible_gutter(content.height)).style(theme::text()),
        gutter_area,
    );
    let mut lines = model.visible_lines(content.height, text_area.width);
    apply_code_block_background(model, &mut lines, text_area.width);
    frame.render_widget(Paragraph::new(lines).style(theme::text()), text_area);
    if let Some(area) = locations_area {
        render_pipe_locations(model, frame, area);
    }
    let visible_end =
        (model.scroll + usize::from(content.height)).min(model.editor.index().len_lines());
    for line_number in model.scroll..visible_end {
        let Some(start) = model.editor.index().line_to_offset(line_number) else {
            continue;
        };
        let Some(line) = model.editor.index().offset_to_line(start) else {
            continue;
        };
        let mut columns = 0;
        'indent: for chunk in model.editor.text().chunks(start..line.end) {
            for character in chunk.chars() {
                match character {
                    ' ' => columns += 1,
                    '\t' => columns += 4 - columns % 4,
                    _ => break 'indent,
                }
            }
        }
        let style = if model.current_line() == Some(line_number) {
            Style::default().fg(theme::muted())
        } else {
            Style::default().fg(theme::dim())
        };
        for level in 1..=columns / 4 {
            let column = level * 4;
            if column >= columns || column < model.hscroll {
                continue;
            }
            let Some(row) = model.row_of_source_line(line_number, usize::from(content.height))
            else {
                continue;
            };
            let x = text_area.x + (column - model.hscroll) as u16;
            let y = content.y + row as u16;
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
                        Span::styled(format!(" {key:<2}"), Style::default().fg(theme::accent())),
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
    if status_cursor.is_none()
        && let Some(position) = model
            .editor
            .index()
            .position(model.editor.text(), model.cursor)
    {
        let x = text_area.x + position.display_column.saturating_sub(model.hscroll) as u16;
        if let Some(row) = model.row_of_source_line(position.line, usize::from(content.height)) {
            let y = content.y + row as u16;
            if x < content.right() && y < content.bottom() {
                frame.set_cursor_position((x, y));
            }
        }
    }
    for selection in editor_view.selections.iter().skip(1) {
        let line = selection.head.line as usize;
        let column = selection.head.display_column as usize;
        if line < model.scroll || column < model.hscroll {
            continue;
        }
        let x = text_area.x + (column - model.hscroll) as u16;
        let Some(row) = model.row_of_source_line(line, usize::from(content.height)) else {
            continue;
        };
        let y = content.y + row as u16;
        if x < text_area.right() && y < text_area.bottom() {
            frame.render_widget(
                Paragraph::new("│").style(
                    Style::default()
                        .fg(theme::accent())
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
        let half = content.width.saturating_mul(model.split_ratio.min(100)) / 100;
        let split_area = if model.ui.focus.split {
            Rect {
                width: half,
                ..content
            }
        } else {
            Rect {
                x: content.x + half,
                width: content.width.saturating_sub(half),
                ..content
            }
        };
        render_split_pane(frame, split_area, &split.buffer, model.ui.focus.split);
    }
    if let Some(palette) = &model.command_palette {
        let popup = command_palette_rect(area);
        frame.render_widget(Clear, popup);
        let visible = popup.height.saturating_sub(3) as usize;
        let selected = palette.table.selected_index();
        let start = selected.saturating_sub(visible.saturating_sub(1));
        let mut lines = vec![Line::styled(
            format!("> {}", palette.table.query()),
            Style::default().fg(theme::accent()),
        )];
        for (row, index) in palette
            .table
            .filtered()
            .iter()
            .skip(start)
            .take(visible)
            .enumerate()
        {
            let style = if start + row == selected {
                theme::selected()
            } else {
                theme::panel()
            };
            lines.push(Line::styled(
                palette.table.items()[*index].label.clone(),
                style,
            ));
        }
        if palette.table.is_empty() {
            lines.push(Line::styled(
                "no commands",
                theme::panel().fg(theme::muted()),
            ));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(panel_block("Commands")),
            popup,
        );
    }

    if let Some(completion) = &model.command_completion {
        let popup = crate::render::command_completion_rect(area, completion.filtered().len());
        frame.render_widget(Clear, popup);
        let visible = popup.height.saturating_sub(2) as usize;
        let selected = completion.selected_index();
        let start = selected.saturating_sub(visible.saturating_sub(1));
        let mut lines = Vec::new();
        for (row, index) in completion
            .filtered()
            .iter()
            .skip(start)
            .take(visible)
            .enumerate()
        {
            let style = if start + row == selected {
                theme::selected()
            } else {
                theme::panel()
            };
            lines.push(Line::styled(
                completion.items()[*index].label.to_owned(),
                style,
            ));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(panel_block("Commands · Tab accept · Esc close")),
            popup,
        );
    }

    if let Some(fuzzy) = &model.fuzzy {
        let horizontal = area.width >= 80;
        let stacked = !horizontal && area.height >= 16;
        let popup = fuzzy_rect(area);

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
            Style::default().fg(theme::accent()).bg(theme::surface()),
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
    let notifications = notification_lines(model);
    if !notifications.is_empty() {
        let width = (area.width / 2).max(24).min(area.width.saturating_sub(2));
        let height = (notifications.len() as u16 + 2)
            .min(7)
            .min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + area.width.saturating_sub(width + 1),
            y: area.y + 1,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(notifications)
                .style(theme::panel())
                .block(panel_block("Notifications")),
            popup,
        );
    }
    let progress = progress_lines(model);
    if !progress.is_empty() {
        let width = (area.width / 2).max(24).min(area.width.saturating_sub(2));
        let height = (progress.len() as u16 + 2)
            .min(7)
            .min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + 1,
            y: area.y + area.height.saturating_sub(height + 2),
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(progress)
                .style(theme::panel())
                .block(panel_block("Progress")),
            popup,
        );
    }
    if model.ui.problems.visible {
        let width = (area.width * 3 / 4)
            .max(30)
            .min(area.width.saturating_sub(2));
        let (all_lines, selected_line) = problem_lines(model, width.saturating_sub(2) as usize);
        let height = (all_lines.len() as u16 + 2)
            .min(18)
            .min(area.height.saturating_sub(2));
        let visible = height.saturating_sub(2) as usize;
        let start = selected_line
            .unwrap_or(0)
            .saturating_sub(visible.saturating_sub(1));
        let mut lines: Vec<Line<'static>> =
            all_lines.into_iter().skip(start).take(visible).collect();
        if lines.is_empty() {
            lines.push(Line::styled(
                "no problems",
                theme::panel().fg(theme::muted()),
            ));
        }
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(panel_block("Problems")),
            popup,
        );
    }
    if let Some(peek) = &model.ui.diagnostic_peek {
        let width = (area.width * 2 / 3)
            .max(30)
            .min(area.width.saturating_sub(2));
        let lines = diagnostic_peek_lines(peek);
        let height = (lines.len() as u16 + 2)
            .min(12)
            .min(area.height.saturating_sub(2));
        let cursor_line = model
            .editor
            .index()
            .position(model.editor.text(), model.cursor)
            .map(|position| position.line as u16)
            .unwrap_or(0);
        let cursor_y = area
            .y
            .saturating_add(cursor_line.saturating_sub(model.scroll as u16))
            .min(area.y + area.height.saturating_sub(1));
        if let Some(popup) = diagnostic_peek_rect(area, cursor_y, width, height) {
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(lines)
                    .style(theme::panel())
                    .scroll((u16::try_from(peek.scroll).unwrap_or(u16::MAX), 0))
                    .block(panel_block(format!(
                        "Diagnostics ({})",
                        peek.diagnostics.len()
                    ))),
                popup,
            );
        }
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
                theme::panel().fg(theme::muted())
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
    if let Some(references) = &model.references {
        let popup = crate::render::document_symbols_rect(area, references.items.len());
        frame.render_widget(Clear, popup);
        let visible = popup.height.saturating_sub(2) as usize;
        let start = references
            .selected
            .saturating_sub(visible.saturating_sub(1));
        let mut lines = Vec::new();
        for (row, item) in references
            .items
            .iter()
            .skip(start)
            .take(visible)
            .enumerate()
        {
            let text = format!(
                "{}:{}:{}",
                item.path.display(),
                item.line + 1,
                item.character + 1
            );
            let style = if start + row == references.selected {
                theme::selected()
            } else {
                theme::panel()
            };
            lines.push(Line::styled(text, style));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(panel_block("References")),
            popup,
        );
    }
    if let Some(calls) = &model.call_hierarchy {
        let popup = crate::render::document_symbols_rect(area, calls.items.len());
        frame.render_widget(Clear, popup);
        let visible = popup.height.saturating_sub(2) as usize;
        let start = calls.selected.saturating_sub(visible.saturating_sub(1));
        let mut lines = Vec::new();
        for (row, item) in calls.items.iter().skip(start).take(visible).enumerate() {
            let text = format!("{}  {}:{}", item.name, item.path.display(), item.line + 1);
            let style = if start + row == calls.selected {
                theme::selected()
            } else {
                theme::panel()
            };
            lines.push(Line::styled(text, style));
        }
        let title = match calls.direction {
            CallHierarchyDirection::Incoming => "Incoming Calls",
            CallHierarchyDirection::Outgoing => "Outgoing Calls",
        };
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(panel_block(title)),
            popup,
        );
    }
    if let Some(overlay) = &model.settings_overlay {
        let popup =
            crate::render::document_symbols_rect(area, crate::settings_ui::SETTINGS_ROWS + 1);
        frame.render_widget(Clear, popup);
        let mut lines = Vec::new();
        for row in 0..crate::settings_ui::SETTINGS_ROWS {
            let marker = if overlay.changed(row) { "*" } else { " " };
            let text = format!(
                "{marker} {:<24} {}",
                crate::settings_ui::SettingsOverlayState::label(row),
                crate::settings_ui::SettingsOverlayState::value(&overlay.draft, row)
            );
            let style = if row == overlay.selected {
                theme::selected()
            } else {
                theme::panel()
            };
            lines.push(Line::styled(text, style));
        }
        lines.push(Line::styled(
            "Enter toggle · ←/→ adjust · r reset · s save · Esc cancel",
            theme::panel().fg(theme::muted()),
        ));
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(panel_block("Settings")),
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
            Style::default().fg(theme::accent()).bg(theme::surface()),
        )];
        if !item.documentation.is_empty() {
            lines.push(Line::raw(item.documentation.clone()));
        }
        lines.push(Line::styled(
            format!("parameter {}", signature.active_parameter + 1),
            Style::default().fg(theme::muted()).bg(theme::surface()),
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
            Style::default().fg(theme::accent()).bg(theme::surface()),
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
                Style::default().fg(theme::muted()).bg(theme::surface()),
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
        let documentation = completion
            .filtered
            .get(completion.selected)
            .and_then(|index| completion.items.get(*index))
            .and_then(|item| item.documentation.as_deref())
            .and_then(|text| text.lines().next())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned);
        let extra = u16::from(documentation.is_some());
        let width = (area.width * 2 / 5)
            .max(36)
            .min(area.width.saturating_sub(2));
        let height = (completion.filtered.len() as u16 + 2 + extra)
            .min(15)
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
            .take(height.saturating_sub(2 + extra) as usize)
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
        if let Some(documentation) = documentation {
            lines.push(Line::styled(
                format!("  {documentation}"),
                theme::panel().fg(theme::muted()),
            ));
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
    crate::pkg_install::render(model, frame, frame.area());
    #[cfg(target_os = "linux")]
    crate::plugin_manager::render(model, frame, frame.area());
}

fn render_pipe_locations(model: &Model, frame: &mut Frame, area: Rect) {
    let rows = usize::from(area.height.saturating_sub(2));
    let start = model
        .stdin_location_scroll
        .min(model.stdin_locations.len().saturating_sub(rows.max(1)));
    let lines = model
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
            let style = if model.stdin_location_selected == Some(index) {
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
                "Locations · {}",
                model.stdin_locations.len()
            ))),
        area,
    );
}

fn truncate_width(text: &str, max: u16) -> String {
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
                .fg(theme::accent())
                .bg(theme::surface())
                .add_modifier(Modifier::BOLD),
        )]),
        Line::styled(
            "selection-first terminal editor",
            Style::default().fg(theme::muted()).bg(theme::surface()),
        ),
        Line::raw(""),
        Line::from(vec![
            Span::styled("i", Style::default().fg(theme::accent())),
            Span::raw("      insert"),
        ]),
        Line::from(vec![
            Span::styled("Space f", Style::default().fg(theme::accent())),
            Span::raw(" find file"),
        ]),
        Line::from(vec![
            Span::styled("Space e", Style::default().fg(theme::accent())),
            Span::raw(" file tree"),
        ]),
        Line::from(vec![
            Span::styled("?", Style::default().fg(theme::accent())),
            Span::raw("      help"),
        ]),
        Line::from(vec![
            Span::styled(":q", Style::default().fg(theme::accent())),
            Span::raw("      quit"),
        ]),
        Line::raw(""),
        Line::styled(
            format!("workspace  {}", model.workspace.display()),
            Style::default().fg(theme::muted()).bg(theme::surface()),
        ),
        Line::styled(
            format!("version    {}", env!("CARGO_PKG_VERSION")),
            Style::default().fg(theme::muted()).bg(theme::surface()),
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

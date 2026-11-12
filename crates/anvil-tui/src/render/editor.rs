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
    let (mode_text, _mode_color, status) = match &model.ui.input_mode {
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
        format!("{} {busy}…", model.spinner_glyph())
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
    let mode_style = match &model.ui.input_mode {
        InputMode::Normal => theme::mode_normal_badge(),
        InputMode::Insert => theme::mode_insert_badge(),
        InputMode::CommandLine { .. } => theme::mode_command_badge(),
        InputMode::Search { .. } => theme::mode_search_badge(),
        InputMode::Rename { .. } | InputMode::RenameEntry { .. } => theme::mode_rename_badge(),
        InputMode::Create { .. } | InputMode::TransferEntry { .. } => theme::mode_create_badge(),
        InputMode::ConfirmClose { .. }
        | InputMode::ConfirmReload { .. }
        | InputMode::ConfirmDelete { .. } => theme::mode_confirm_badge(),
    };
    let mode = Span::styled(mode_text, mode_style);
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
    if crate::breadcrumbs::visible(model, inner) {
        let breadcrumb = Rect { height: 1, ..inner };
        crate::breadcrumbs::render(model, frame, breadcrumb);
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
        let ascii_icons = model.ascii_icons;
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
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let icon = if entry.is_dir {
                icon_theme::folder_icon(entry.expanded)
            } else {
                icon_theme::icon_for(&name).unwrap_or_else(icon_theme::default_file_icon)
            };
            let glyph = model.icons.glyph(icon, ascii_icons);
            let icon_padding = if UnicodeWidthStr::width(glyph) == 1 {
                " "
            } else {
                ""
            };
            let style = if model.tree_selected == Some(index) {
                theme::selected()
            } else if entry.is_dir {
                Style::default().fg(theme::accent()).bg(theme::surface())
            } else {
                theme::panel()
            };
            let mut spans = vec![
                Span::raw("  ".repeat(entry.depth)),
                Span::styled(glyph, style.fg((icon.color)())),
                Span::raw(icon_padding),
                Span::styled(name, style),
            ];
            let modified = model.path.as_ref() == Some(&entry.path) && model.modified
                || model
                    .buffers
                    .iter()
                    .any(|buffer| buffer.path.as_ref() == Some(&entry.path) && buffer.modified)
                || model.split.as_ref().is_some_and(|pane| {
                    pane.buffer.path.as_ref() == Some(&entry.path) && pane.buffer.modified
                });
            if modified {
                spans.push(Span::styled(
                    " *",
                    style.fg(theme::warning()).add_modifier(Modifier::BOLD),
                ));
            }
            let git_marker = if entry.is_dir {
                tree_rollup_git(model, &model.tree, index)
            } else {
                model.git_status.get(&entry.path).copied()
            };
            if let Some(marker) = git_marker {
                let color = match marker {
                    'A' => theme::success(),
                    'D' => theme::error(),
                    'R' | 'C' => theme::accent(),
                    '?' => theme::warning(),
                    _ => theme::warning(),
                };
                spans.push(Span::styled(
                    format!(" {marker}"),
                    style.fg(color).add_modifier(Modifier::BOLD),
                ));
            }
            let diagnostic_severity = if entry.is_dir {
                tree_rollup_diagnostic(model, &model.tree, index)
            } else {
                model
                    .language
                    .diagnostics
                    .by_path
                    .get(&entry.path)
                    .and_then(|diagnostics| {
                        diagnostics
                            .iter()
                            .filter_map(|diagnostic| diagnostic.severity)
                            .min()
                    })
            };
            if let Some(severity) = diagnostic_severity {
                let color = match severity {
                    1 => theme::error(),
                    2 => theme::warning(),
                    _ => theme::accent(),
                };
                spans.push(Span::styled(
                    " !",
                    style.fg(color).add_modifier(Modifier::BOLD),
                ));
            }
            lines.push(Line::from(spans));
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
                    .block(panel_block(title.to_owned())),
                popup,
            );
        }
    }
    crate::sticky::render(model, frame, text_area);
    if status_cursor.is_none()
        && let Some(position) = model
            .editor
            .index()
            .position(model.editor.text(), model.cursor)
    {
        if let Some(row) = model.row_of_position(
            position.line,
            position.display_column,
            usize::from(content.height),
        ) {
            let base = model
                .position_at_visible_row(row)
                .map_or(model.hscroll, |(_, base)| base);
            let x = text_area.x + position.display_column.saturating_sub(base) as u16;
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
        let Some(row) = model.row_of_position(line, column, usize::from(content.height)) else {
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
    if let Some(palette) = &model.overlays.command_palette {
        let popup = command_palette_rect(area);
        frame.render_widget(Clear, popup);
        let visible = popup.height.saturating_sub(4) as usize;
        let selected = palette.table.selected_index();
        let start = selected.saturating_sub(visible.saturating_sub(1));
        let mut lines = vec![Line::styled(
            format!("> {}", palette.table.query()),
            Style::default().fg(theme::accent()),
        )];
        let detail_width = usize::from(popup.width).saturating_sub(34).clamp(12, 40);
        for (row, index) in palette
            .table
            .filtered()
            .iter()
            .skip(start)
            .take(visible)
            .enumerate()
        {
            let selected_row = start + row == selected;
            let style = if selected_row {
                theme::selected()
            } else {
                theme::panel()
            };
            let item = &palette.table.items()[*index];
            let title = truncate_width(
                &item.title,
                (popup.width.saturating_sub(4) as usize).min(30) as u16,
            );
            let detail = truncate_width(&item.detail, detail_width as u16);
            lines.push(Line::from(vec![
                Span::styled(if selected_row { "❯ " } else { "  " }, style),
                Span::styled(
                    format!("{title:<30}"),
                    if selected_row {
                        style
                    } else {
                        style.fg(theme::text_color())
                    },
                ),
                Span::styled(
                    detail,
                    if selected_row {
                        style
                    } else {
                        style.fg(theme::muted())
                    },
                ),
            ]));
        }
        if palette.table.is_empty() {
            lines.push(Line::styled(
                "no commands",
                theme::panel().fg(theme::muted()),
            ));
        }
        lines.push(Line::styled(
            "Enter run · ↑/↓ move · Esc close",
            theme::panel().fg(theme::muted()),
        ));
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(crate::render::focused_panel_block(format!(
                    "Commands · {} · Esc close",
                    palette.table.filtered().len()
                ))),
            popup,
        );
    }

    if let Some(completion) = &model.overlays.command_completion {
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
                .block(crate::render::focused_panel_block(
                    "Commands · Tab accept · Esc close",
                )),
            popup,
        );
    }

    if let Some(fuzzy) = &model.overlays.fuzzy {
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
        let ascii_icons = model.ascii_icons;
        let start = fuzzy.selected.saturating_sub(visible.saturating_sub(1));
        for (row, index) in fuzzy.filtered.iter().skip(start).take(visible).enumerate() {
            let name = model.fuzzy_display_path(&fuzzy.entries[*index]);
            lines.push(fuzzy_path_line(
                &model.icons,
                ascii_icons,
                &name,
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
                .block(crate::render::focused_panel_block(if model.show_hidden {
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
    #[cfg(unix)]
    crate::terminal_control::render_panel(model, frame, area);
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
                .block(crate::render::focused_panel_block("Problems")),
            popup,
        );
    }
    crate::test_explorer::render(model, frame, area);
    crate::theme_browser::render(model, frame, area);
    crate::debug_panel::render(model, frame, area);
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
    if let Some(actions) = &model.overlays.lsp_results.code_actions {
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
                .block(crate::render::focused_panel_block("Code Actions")),
            popup,
        );
    }
    if let Some(references) = &model.overlays.lsp_results.references {
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
                .block(crate::render::focused_panel_block("References")),
            popup,
        );
    }
    if let Some(calls) = &model.overlays.lsp_results.call_hierarchy {
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
                .block(crate::render::focused_panel_block(title.to_owned())),
            popup,
        );
    }
    if let Some(overlay) = &model.overlays.settings_overlay {
        let popup = crate::settings_ui::rect(area);
        frame.render_widget(Clear, popup);
        frame.render_widget(crate::render::focused_panel_block("Settings"), popup);
        let inner = Rect {
            x: popup.x + 2,
            y: popup.y + 1,
            width: popup.width.saturating_sub(4),
            height: popup.height.saturating_sub(2),
        };
        if inner.width < 20 || inner.height < 6 {
            return;
        }
        let group = crate::settings_ui::SettingsOverlayState::group(overlay.selected);
        let active_tab = crate::settings_ui::SettingsOverlayState::tab(overlay.selected);
        let tab_style = |name| {
            if name == active_tab {
                theme::panel()
                    .fg(theme::accent())
                    .add_modifier(Modifier::BOLD)
            } else {
                theme::panel().fg(theme::dim())
            }
        };
        let tabs = Line::from(vec![
            Span::styled(" Editor ", tab_style("Editor")),
            Span::styled("│", theme::panel().fg(theme::border_color())),
            Span::styled(" Interface ", tab_style("Interface")),
            Span::styled("│", theme::panel().fg(theme::border_color())),
            Span::styled(" Language ", tab_style("Language")),
        ]);
        frame.render_widget(
            Paragraph::new(tabs).style(theme::panel()),
            Rect { height: 1, ..inner },
        );
        let body = Rect {
            y: inner.y + 2,
            height: inner.height.saturating_sub(4),
            ..inner
        };
        let sidebar_width = (inner.width / 5).clamp(18, 24);
        let sidebar = Rect {
            width: sidebar_width,
            ..body
        };
        let content = Rect {
            x: body.x + sidebar_width,
            width: body.width.saturating_sub(sidebar_width),
            ..body
        };
        let mut sidebar_lines = Vec::new();
        for name in ["Editor", "Search", "LSP", "Accessibility"] {
            let style = if name == group {
                theme::selected()
            } else {
                theme::panel().fg(theme::muted())
            };
            sidebar_lines.push(Line::styled(format!("  {name}"), style));
        }
        frame.render_widget(
            Paragraph::new(sidebar_lines)
                .style(theme::panel())
                .block(panel_block("Sections")),
            sidebar,
        );
        let mut lines = Vec::new();
        let mut rendered_group = "";
        for row in 0..crate::settings_ui::SETTINGS_ROWS {
            let next = crate::settings_ui::SettingsOverlayState::group(row);
            if next != rendered_group {
                rendered_group = next;
                lines.push(Line::styled(
                    rendered_group.to_ascii_uppercase(),
                    theme::panel().fg(theme::dim()).add_modifier(Modifier::BOLD),
                ));
            }
            let marker = if overlay.changed(row) { "*" } else { " " };
            let label = crate::settings_ui::SettingsOverlayState::label(row);
            let value = crate::settings_ui::SettingsOverlayState::value(&overlay.draft, row);
            let style = if row == overlay.selected {
                theme::selected()
            } else {
                theme::panel()
            };
            let value_style = if row == overlay.selected {
                style
            } else {
                theme::panel().fg(theme::warning())
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{marker} {label:<27} "), style),
                Span::styled(value, value_style),
            ]));
        }
        let visible = body.height.saturating_sub(2) as usize;
        let start = overlay
            .selected
            .saturating_add(crate::settings_ui::SETTINGS_ROWS / 2)
            .saturating_sub(visible)
            .min(lines.len().saturating_sub(visible));
        frame.render_widget(
            Paragraph::new(
                lines
                    .into_iter()
                    .skip(start)
                    .take(visible)
                    .collect::<Vec<_>>(),
            )
            .style(theme::panel())
            .block(panel_block("Values")),
            content,
        );
        let description = Rect {
            y: inner.y + inner.height.saturating_sub(1),
            height: 1,
            ..inner
        };
        frame.render_widget(
            Paragraph::new(Line::styled(
                crate::settings_ui::SettingsOverlayState::description(overlay.selected),
                theme::panel().fg(theme::muted()),
            )),
            description,
        );
    }
    if let Some(symbols) = &model.overlays.lsp_results.document_symbols {
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
                .block(crate::render::focused_panel_block("Document Symbols")),
            popup,
        );
    }

    if let Some(signature) = &model.overlays.lsp_results.signature_help {
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
                .block(crate::render::focused_panel_block("Signature")),
            popup,
        );
    }
    if let Some(symbols) = &model.overlays.lsp_results.workspace_symbols {
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
                .block(crate::render::focused_panel_block("Workspace Symbols")),
            popup,
        );
    }
    if let Some(completion) = &model.overlays.lsp_results.completion {
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
                .block(crate::render::focused_panel_block(format!(
                    "Completion #{}",
                    completion.id
                ))),
            popup,
        );
    }
    if let Some(hover) = &model.overlays.lsp_results.hover {
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

pub(crate) fn welcome_recent_at(
    model: &Model,
    column: u16,
    row: u16,
    width: u16,
    height: u16,
) -> Option<usize> {
    if !model.welcome {
        return None;
    }
    let area = Rect {
        x: 1,
        y: 1,
        width: width.saturating_sub(2),
        height: height.saturating_sub(2),
    };
    let panel_width = area.width.saturating_sub(4).clamp(12, 92).min(area.width);
    let panel_height = area.height.saturating_sub(2).clamp(4, 28).min(area.height);
    let panel = Rect {
        x: area.x + area.width.saturating_sub(panel_width) / 2,
        y: area.y + area.height.saturating_sub(panel_height) / 2,
        width: panel_width,
        height: panel_height,
    };
    let files = model.openable_recent_files();
    if files.is_empty() {
        return None;
    }
    if panel.width >= 68 && panel.height >= 14 {
        let left_width = ((panel.width as usize * 2) / 5).clamp(26, 38) as u16;
        let right = Rect {
            x: panel.x + left_width + 2,
            width: panel.width.saturating_sub(left_width + 2),
            ..panel
        };
        if column <= right.x || column >= right.right().saturating_sub(1) {
            return None;
        }
        let first = right.y.saturating_add(3);
        let offset = usize::from(row.saturating_sub(first));
        return (row >= first && offset < files.len().min(8)).then_some(offset);
    }
    let first = panel.y.saturating_add(15);
    let offset = usize::from(row.saturating_sub(first));
    if column <= panel.x || column >= panel.right().saturating_sub(1) {
        return None;
    }
    (row >= first && offset < files.len().min(5)).then_some(offset)
}

fn render_welcome(frame: &mut Frame, area: Rect, model: &Model) {
    let width = area.width.saturating_sub(4).clamp(12, 92).min(area.width);
    let height = area.height.saturating_sub(2).clamp(4, 28).min(area.height);
    let panel = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, panel);
    if width >= 68 && height >= 14 {
        let gap = 2;
        let left_width = ((width as usize * 2) / 5).clamp(26, 38) as u16;
        let left = Rect {
            width: left_width,
            ..panel
        };
        let right = Rect {
            x: panel.x + left_width + gap,
            width: panel.width.saturating_sub(left_width + gap),
            ..panel
        };
        render_welcome_actions(frame, left, model);
        render_welcome_recent(frame, right, model);
    } else {
        let mut lines = welcome_actions(model, usize::from(width));
        lines.push(Line::styled("", theme::panel()));
        lines.extend(welcome_recent(
            model,
            usize::from(width.saturating_sub(2)),
            5,
        ));
        render_welcome_panel(frame, panel, "Start", lines);
    }
}

fn render_welcome_actions(frame: &mut Frame, area: Rect, model: &Model) {
    let lines = welcome_actions(model, usize::from(area.width.saturating_sub(2)));
    render_welcome_panel(frame, area, "Start", lines);
}

fn render_welcome_recent(frame: &mut Frame, area: Rect, model: &Model) {
    let lines = welcome_recent(model, usize::from(area.width.saturating_sub(2)), 8);
    render_welcome_panel(frame, area, "Recent", lines);
}

fn render_welcome_panel(frame: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'_>>) {
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::panel())
            .block(panel_block(title.to_owned())),
        area,
    );
}

fn welcome_actions(model: &Model, width: usize) -> Vec<Line<'static>> {
    let key = Style::default()
        .fg(theme::accent())
        .add_modifier(Modifier::BOLD);
    let muted = theme::panel().fg(theme::muted());
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "ANVIL",
                Style::default()
                    .fg(theme::accent())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", env!("CARGO_PKG_VERSION")),
                theme::panel().fg(theme::dim()),
            ),
        ]),
        Line::styled("selection-first terminal editor", muted),
        Line::styled("", theme::panel()),
        Line::from(vec![
            Span::styled(format!(" {:<8}", "i"), key),
            Span::raw("insert"),
        ]),
        Line::from(vec![
            Span::styled(format!(" {:<8}", "Space f"), key),
            Span::raw("find file"),
        ]),
        Line::from(vec![
            Span::styled(format!(" {:<8}", "Space e"), key),
            Span::raw("file tree"),
        ]),
        Line::from(vec![
            Span::styled(format!(" {:<8}", "r"), key),
            Span::raw("recent files"),
        ]),
        Line::from(vec![
            Span::styled(format!(" {:<8}", "?"), key),
            Span::raw("help"),
        ]),
        Line::from(vec![
            Span::styled(format!(" {:<8}", ":q"), key),
            Span::raw("quit"),
        ]),
        Line::styled("", theme::panel()),
        Line::styled(
            truncate_width(
                &format!("workspace  {}", model.workspace.display()),
                u16::try_from(width.saturating_sub(2)).unwrap_or(u16::MAX),
            ),
            muted,
        ),
    ];
    lines
}

fn welcome_recent(model: &Model, width: usize, limit: usize) -> Vec<Line<'static>> {
    let muted = theme::panel().fg(theme::muted());
    let dim = theme::panel().fg(theme::dim());
    let files = model.openable_recent_files();
    let mut lines = vec![Line::styled("FILES", dim), Line::styled("", theme::panel())];
    if files.is_empty() {
        lines.push(Line::styled("no recent files in this workspace", muted));
    } else {
        for path in files.iter().take(limit) {
            let display = path
                .strip_prefix(&model.workspace)
                .unwrap_or(path)
                .display()
                .to_string();
            lines.push(Line::styled(
                format!(
                    " {}",
                    truncate_width(
                        &display,
                        u16::try_from(width.saturating_sub(1)).unwrap_or(u16::MAX)
                    )
                ),
                muted,
            ));
        }
    }
    lines.push(Line::styled("", theme::panel()));
    lines.push(Line::styled("WORKSPACES", dim));
    lines.push(Line::styled("", theme::panel()));
    let workspaces = model.recent_workspaces.iter().take(3).collect::<Vec<_>>();
    if workspaces.is_empty() {
        lines.push(Line::styled("no recent workspaces", muted));
    } else {
        for workspace in workspaces {
            let name = workspace
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| workspace.display().to_string());
            lines.push(Line::styled(
                format!(
                    " {}",
                    truncate_width(
                        &name,
                        u16::try_from(width.saturating_sub(1)).unwrap_or(u16::MAX)
                    )
                ),
                muted,
            ));
        }
    }
    lines.push(Line::styled("", theme::panel()));
    lines.push(Line::styled("r open recent files", dim));
    lines
}

fn tree_rollup_git(model: &Model, entries: &[TreeEntry], index: usize) -> Option<char> {
    let depth = entries[index].depth;
    let mut marker = None;
    for child in entries.iter().skip(index + 1) {
        if child.depth <= depth {
            break;
        }
        if let Some(next) = model.git_status.get(&child.path)
            && git_priority(*next) > marker.map_or(0, git_priority)
        {
            marker = Some(*next);
        }
    }
    marker
}

fn git_priority(marker: char) -> u8 {
    match marker {
        'D' => 4,
        'R' | 'C' => 3,
        'A' => 2,
        'M' => 1,
        _ => 0,
    }
}

fn tree_rollup_diagnostic(model: &Model, entries: &[TreeEntry], index: usize) -> Option<u32> {
    let depth = entries[index].depth;
    let mut severity = None;
    for child in entries.iter().skip(index + 1) {
        if child.depth <= depth {
            break;
        }
        if let Some(diagnostics) = model.language.diagnostics.by_path.get(&child.path)
            && let Some(next) = diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.severity)
                .min()
        {
            severity = Some(severity.map_or(next, |current: u32| current.min(next)));
        }
    }
    severity
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

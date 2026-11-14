//! Main editor frame and overlays.

use super::*;
use unicode_width::UnicodeWidthStr;
pub fn view(model: &Model, frame: &mut Frame) {
    let area = frame.area();
    let editor_view: EditorView = model.session.editor.snapshot().view();
    let title = if model.pager.back.is_some() {
        format!("[{}]", model.pager.title.as_deref().unwrap_or("help"))
    } else {
        let name = if model.file.stdin_buffer {
            "[stdin]".to_owned()
        } else {
            model.breadcrumb_text(usize::from(area.width).saturating_sub(6))
        };
        let name = if model.file.modified {
            format!("[+] {name}")
        } else {
            name
        };
        let name = if model.file.read_only {
            format!("{name} [RO]")
        } else {
            name
        };
        if model.buffers.entries.is_empty() {
            name
        } else {
            format!(
                "[{}/{}] {name}",
                model.buffers.active + 1,
                model.buffers.entries.len()
            )
        }
    };
    let title = if let Some(split) = &model.buffers.split {
        let secondary = split
            .buffer
            .path
            .as_ref()
            .map_or("[no file]", |path| path.to_str().unwrap_or("[no file]"));
        format!("{title} │ {secondary}")
    } else {
        title
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
    let status = if let Some(busy) = &model.status.busy {
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
        .border_style(theme::border(!model.ui.focus.tree && !model.ui.focus.split))
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
    if !model.start_page.welcome && (model.buffers.entries.len() > 1 || model.pager.back.is_some())
    {
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
    if model.start_page.welcome {
        render_welcome(frame, inner, model);
        return;
    }
    if let Some(next) = render_tree(model, frame, inner) {
        content = next;
    }
    let locations_area = crate::pipe_locations::panel_rect(model, content);
    if let Some(area) = locations_area {
        content.height = area.y.saturating_sub(content.y);
    }
    let mut split_area = None;
    if model.buffers.split.is_some() {
        let half = content
            .width
            .saturating_mul(model.view.split_ratio.min(100))
            / 100;
        let left = Rect {
            width: half,
            ..content
        };
        let right = Rect {
            x: content.x + half,
            width: content.width.saturating_sub(half),
            ..content
        };
        if model.ui.focus.split {
            content = right;
            split_area = Some(left);
        } else {
            content = left;
            split_area = Some(right);
        }
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
    let visible_rows = model.visible_rows(model.view.scroll, usize::from(content.height));
    let mut source_rows = HashMap::new();
    for (row, display_row) in visible_rows.iter().enumerate() {
        match display_row {
            crate::layout::DisplayRow::Source(line) => {
                source_rows.insert(*line, row);
            }
            crate::layout::DisplayRow::Wrapped {
                line, first: true, ..
            } => {
                source_rows.insert(*line, row);
            }
            _ => {}
        }
    }
    let visible_end = (model.view.scroll + usize::from(content.height))
        .min(model.session.editor.index().len_lines());
    let active_indent = model
        .current_line()
        .and_then(|line_number| indentation_columns(model, line_number));
    for line_number in model.view.scroll..visible_end {
        let Some(columns) = indentation_columns(model, line_number) else {
            continue;
        };
        for level in 1..=columns / 4 {
            let column = level * 4;
            if column >= columns || column < model.view.hscroll {
                continue;
            }
            let style = if model.current_line() == Some(line_number) {
                Style::default().fg(theme::muted())
            } else {
                Style::default().fg(theme::dim())
            };
            let Some(&row) = source_rows.get(&line_number) else {
                continue;
            };
            let x = text_area.x + (column - model.view.hscroll) as u16;
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
        if let Some(active) = active_indent
            && active > 0
            && columns >= active
        {
            let column = active - 1;
            if column >= model.view.hscroll
                && let Some(&row) = source_rows.get(&line_number)
            {
                let x =
                    text_area.x + u16::try_from(column - model.view.hscroll).unwrap_or(u16::MAX);
                let y = content.y + row as u16;
                if x < text_area.right() && y < content.bottom() {
                    frame.render_widget(
                        Paragraph::new("│").style(Style::default().fg(theme::accent())),
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
    }
    if let Some(prefix) = model.input.pending {
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
            .session
            .editor
            .index()
            .position(model.session.editor.text(), model.session.cursor)
    {
        if let Some(row) = model.row_of_position(
            position.line,
            position.display_column,
            usize::from(content.height),
        ) {
            let base = model
                .position_at_visible_row(row)
                .map_or(model.view.hscroll, |(_, base)| base);
            let x = text_area.x + position.display_column.saturating_sub(base) as u16;
            let y = content.y + row as u16;
            if x < content.right() && y < content.bottom() {
                frame.set_cursor_position((x, y));
            }
        }
    }
    if let Some(selection) = editor_view.selections.first()
        && selection.anchor != selection.head
        && selection.anchor.display_column == 0
    {
        let line = selection.anchor.line as usize;
        if line >= model.view.scroll
            && let Some(row) = model.row_of_position(
                line,
                selection.anchor.display_column as usize,
                usize::from(content.height),
            )
        {
            let x = text_area.x.saturating_sub(1);
            let y = content.y + row as u16;
            if x >= content.x && y < content.bottom() {
                frame.render_widget(
                    Paragraph::new("│").style(Style::default().fg(theme::warning())),
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
    for selection in editor_view.selections.iter().skip(1) {
        let line = selection.head.line as usize;
        let column = selection.head.display_column as usize;
        if line < model.view.scroll || column < model.view.hscroll {
            continue;
        }
        let x = text_area.x + (column - model.view.hscroll) as u16;
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
    if let (Some(split), Some(area)) = (&model.buffers.split, split_area) {
        render_split_pane(
            frame,
            area,
            &split.buffer,
            model.ui.focus.split,
            model.config.settings.editor.soft_wrap,
        );
    }
    render_overlays(model, frame, area);
}

fn indentation_columns(model: &Model, line_number: usize) -> Option<usize> {
    let start = model.session.editor.index().line_to_offset(line_number)?;
    let line = model.session.editor.index().offset_to_line(start)?;
    let mut columns = 0;
    'indent: for chunk in model.session.editor.text().chunks(start..line.end) {
        for character in chunk.chars() {
            match character {
                ' ' => columns += 1,
                '\t' => columns += 4 - columns % 4,
                _ => break 'indent,
            }
        }
    }
    Some(columns)
}

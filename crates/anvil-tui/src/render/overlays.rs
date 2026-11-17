//! Overlay rendering for completion, LSP, settings, and tool panels.

use super::*;
use crate::CallHierarchyDirection;

fn completion_kind_label(item: &crate::state::CompletionItem) -> &'static str {
    let kind = item
        .raw
        .get("kind")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    match kind {
        1 => "text",
        2 => "method",
        3 => "function",
        4 => "constructor",
        5 => "field",
        6 => "variable",
        7 => "class",
        8 => "interface",
        9 => "module",
        10 => "property",
        11 => "unit",
        12 => "value",
        13 => "enum",
        14 => "keyword",
        15 => "snippet",
        16 => "color",
        17 => "file",
        18 => "reference",
        19 => "folder",
        20 => "enum member",
        21 => "constant",
        22 => "struct",
        23 => "event",
        24 => "operator",
        25 => "type parameter",
        _ => match item.source.as_str() {
            "builtin.buffer-words" => "text",
            "builtin.paths" => "path",
            "builtin.lsp" => "lsp",
            _ => "text",
        },
    }
}

fn completion_label_spans(label: &str, query: &str, style: Style) -> Vec<Span<'static>> {
    let prefix_chars = query.chars().count();
    if prefix_chars == 0 {
        return vec![Span::styled(label.to_owned(), style)];
    }
    let split = label
        .char_indices()
        .nth(prefix_chars)
        .map_or(label.len(), |(index, _)| index);
    if label[..split].to_lowercase() != query.to_lowercase() {
        return vec![Span::styled(label.to_owned(), style)];
    }
    vec![
        Span::styled(
            label[..split].to_owned(),
            style.fg(theme::accent()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(label[split..].to_owned(), style),
    ]
}

pub(crate) fn render_overlays(model: &Model, frame: &mut Frame, area: Rect) {
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
                &format!("{}: {}", item.group, item.title),
                (popup.width.saturating_sub(6) as usize).min(28) as u16,
            );
            let detail = item.key.as_ref().map_or_else(
                || item.detail.clone(),
                |key| format!("{key} · {}", item.detail),
            );
            let detail = truncate_width(&detail, detail_width as u16);
            let marker = if selected_row {
                "❯ "
            } else if item.recent {
                "↺ "
            } else {
                "  "
            };
            lines.push(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(
                    format!("{title:<28}"),
                    if selected_row {
                        style
                    } else {
                        style.fg(theme::text_color())
                    },
                ),
                Span::raw("  "),
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
        let (label, empty) = match palette.mode {
            crate::state::CommandPaletteMode::All => ("Commands", "no commands"),
            crate::state::CommandPaletteMode::History => ("Command history", "no command history"),
            crate::state::CommandPaletteMode::Keybinds => {
                ("Keymapped commands", "no keymapped commands")
            }
        };
        if palette.table.is_empty() {
            lines.push(Line::styled(empty, theme::panel().fg(theme::muted())));
        }
        lines.push(Line::styled(
            "Enter run · ↑/↓ move · Esc close",
            theme::panel().fg(theme::muted()),
        ));
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(crate::render::focused_panel_block(format!(
                    "{label} · {} · Esc close",
                    palette.table.filtered().len()
                ))),
            popup,
        );
    }

    if let Some(completion) = &model.overlays.command_completion
        && !completion.filtered().is_empty()
    {
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

    if let Some(picker) = &model.overlays.buffer_picker {
        let popup = fuzzy_rect(area);
        frame.render_widget(Clear, popup);
        let visible = popup.height.saturating_sub(3) as usize;
        let start = picker.selected.saturating_sub(visible.saturating_sub(1));
        let mut lines = Vec::new();
        for (row, index) in picker.filtered.iter().skip(start).take(visible).enumerate() {
            let selected = start + row == picker.selected;
            let style = if selected {
                theme::selected()
            } else {
                theme::panel()
            };
            let Some(entry) = picker.entries.get(*index) else {
                continue;
            };
            let icon = if matches!(picker.mode, crate::state::BufferPickerMode::Buffers) {
                entry
                    .value
                    .as_u64()
                    .and_then(|index| usize::try_from(index).ok())
                    .and_then(|index| model.buffers.entries.get(index))
                    .and_then(|buffer| {
                        let name = buffer.path.as_deref()?.file_name()?;
                        let icon = icon_theme::icon_for(&name.to_string_lossy())
                            .unwrap_or_else(icon_theme::default_file_icon);
                        let text = model
                            .appearance
                            .icons
                            .icon_text(icon, model.appearance.ascii_icons)?
                            .trim_end()
                            .to_owned();
                        let color = if buffer.missing {
                            theme::dim()
                        } else {
                            (icon.color)()
                        };
                        Some((text, color))
                    })
            } else {
                None
            };
            let icon_width = icon
                .as_ref()
                .map(|(text, _)| UnicodeWidthStr::width(text.as_str()))
                .unwrap_or(0);
            let max_width = popup.width.saturating_sub(2) as usize;
            let display_width =
                max_width.saturating_sub(2 + icon_width + usize::from(icon.is_some())) as u16;
            let mut spans = vec![Span::styled(if selected { "❯ " } else { "  " }, style)];
            if let Some((text, color)) = icon {
                spans.push(Span::styled(text, style.fg(color)));
                spans.push(Span::styled(" ", style));
            }
            spans.push(Span::styled(
                truncate_width(&entry.display, display_width),
                style,
            ));
            lines.push(Line::from(spans));
        }
        let (title, empty) = match picker.mode {
            crate::state::BufferPickerMode::Buffers => {
                ("Buffers · Enter switch · Esc close", "no matching buffers")
            }
            crate::state::BufferPickerMode::Plugin { .. } => {
                ("Provider · Enter apply · Esc close", "no matching items")
            }
            crate::state::BufferPickerMode::ChangedFiles { .. } => (
                "Changed files · Enter open · Esc close",
                if model.workspace.git.status_pending.is_some() {
                    "loading changed files"
                } else {
                    "no changed files"
                },
            ),
            crate::state::BufferPickerMode::Jumps { .. } => {
                ("Jumps · Enter jump · Esc close", "no jumps")
            }
            crate::state::BufferPickerMode::SearchHistory { .. } => {
                ("Search history · Enter replay · Esc close", "no searches")
            }
        };
        if picker.filtered.is_empty() {
            lines.push(Line::styled(empty, theme::muted()));
        }
        lines.push(Line::styled(
            format!("> {}", picker.query),
            Style::default().fg(theme::accent()).bg(theme::surface()),
        ));
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(crate::render::focused_panel_block(title)),
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
        let ascii_icons = model.appearance.ascii_icons;
        let start = fuzzy.selected.saturating_sub(visible.saturating_sub(1));
        for (row, index) in fuzzy.filtered.iter().skip(start).take(visible).enumerate() {
            let name = model.fuzzy_display_path(&fuzzy.entries[*index]);
            lines.push(fuzzy_path_line(
                &model.appearance.icons,
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
                .block(crate::render::focused_panel_block(
                    if model.workspace.tree.show_hidden {
                        "Files · hidden"
                    } else {
                        "Files"
                    },
                )),
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
        let popup = crate::render::problems_popup_rect(model, area);
        let (all_lines, selected_line) =
            problem_lines(model, popup.width.saturating_sub(2) as usize);
        let visible = popup.height.saturating_sub(2) as usize;
        let start = selected_line
            .unwrap_or(0)
            .saturating_sub(visible.saturating_sub(1));
        let mut lines: Vec<Line<'static>> =
            all_lines.into_iter().skip(start).take(visible).collect();
        if lines.is_empty() {
            let text = if model.ui.problems.items.is_empty() {
                "no problems"
            } else {
                "no matching problems"
            };
            lines.push(Line::styled(text, theme::panel().fg(theme::muted())));
        }
        let query = &model.ui.problems.query;
        let title = if query.is_empty() {
            format!(
                "Problems · {}/{}",
                model.ui.problems.visible_len(),
                model.ui.problems.items.len()
            )
        } else {
            format!(
                "Problems /{query} · {}/{}",
                model.ui.problems.visible_len(),
                model.ui.problems.items.len()
            )
        };
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(crate::render::focused_panel_block(title)),
            popup,
        );
    }
    crate::test_explorer::render(model, frame, area);
    if let Some(browser) = &model.overlays.theme_browser {
        crate::theme_browser::render(browser, frame, area);
    }
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
            .session
            .editor
            .index()
            .position(model.session.editor.text(), model.session.cursor)
            .map(|position| position.line as u16)
            .unwrap_or(0);
        let cursor_y = area
            .y
            .saturating_add(cursor_line.saturating_sub(model.view.scroll as u16))
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
    if let Some(overlay) = &model.overlays.settings_overlay
        && !crate::settings_ui::render(overlay, frame, area)
    {
        return;
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
    if let Some(completion) = &model.language.completion.completion {
        let selected_item = completion
            .filtered
            .get(completion.selected)
            .and_then(|index| completion.items.get(*index));
        let auto_import = selected_item
            .and_then(|item| crate::completion_sources::completion_auto_import(&item.raw));
        let detail_panel = auto_import.map(|text| ("Auto-import", text)).or_else(|| {
            selected_item.and_then(|item| {
                let mut body = item.detail.clone();
                if let Some(documentation) = &item.documentation {
                    let documentation =
                        crate::completion_sources::clean_completion_documentation(documentation);
                    if !documentation.is_empty() {
                        if !body.is_empty() {
                            body.push_str("\n\n");
                        }
                        body.push_str(&documentation);
                    }
                }
                (!body.trim().is_empty()).then_some(("Details", body))
            })
        });
        let side_width = if detail_panel.is_some() && area.width >= 80 {
            (area.width * 2 / 5).max(30)
        } else {
            0
        };
        let gap = u16::from(side_width > 0);
        let max_total = area.width.saturating_sub(2);
        let desired_list = (area.width * 2 / 5).max(44);
        let total_width = (desired_list + gap + side_width).min(max_total);
        let width = if side_width == 0 {
            total_width
        } else {
            total_width.saturating_sub(side_width + gap)
        };
        let documentation = if side_width == 0 {
            selected_item
                .and_then(|item| item.documentation.as_deref())
                .and_then(|text| text.lines().next())
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        } else {
            None
        };
        let extra = u16::from(documentation.is_some());
        let detail_height = detail_panel
            .as_ref()
            .map(|(_, body)| body.lines().count() as u16 + 2)
            .unwrap_or(0);
        let height = (completion.filtered.len() as u16 + 3 + extra)
            .max(detail_height)
            .min(15)
            .min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(total_width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        let visible = height.saturating_sub(3 + extra) as usize;
        let start = completion
            .selected
            .saturating_sub(visible.saturating_sub(1));
        let has_scrollbar = completion.filtered.len() > visible;
        let inner_width = usize::from(width.saturating_sub(2 + u16::from(has_scrollbar)));
        let mut lines = Vec::new();
        for (row, index) in completion
            .filtered
            .iter()
            .skip(start)
            .take(visible)
            .enumerate()
        {
            let item = &completion.items[*index];
            let selected_row = start + row == completion.selected;
            let style = if selected_row {
                theme::selected()
            } else {
                theme::panel()
            };
            let kind = completion_kind_label(item);
            let kind_width = unicode_width::UnicodeWidthStr::width(kind);
            let label_max = inner_width.saturating_sub(2 + kind_width + 2).max(1);
            let label = crate::render::truncate_width(
                &item.label,
                u16::try_from(label_max).unwrap_or(u16::MAX),
            );
            let label_width = unicode_width::UnicodeWidthStr::width(label.as_str());
            let padding = inner_width
                .saturating_sub(2 + label_width + kind_width)
                .max(1);
            let mut spans = vec![Span::styled(if selected_row { "❯ " } else { "  " }, style)];
            spans.extend(completion_label_spans(&label, &completion.query, style));
            spans.push(Span::styled(" ".repeat(padding), style));
            spans.push(Span::styled(kind, style.fg(theme::muted())));
            lines.push(Line::from(spans));
        }
        if let Some(documentation) = documentation {
            lines.push(Line::styled(
                format!("  {documentation}"),
                theme::panel().fg(theme::muted()),
            ));
        }
        lines.push(Line::styled(
            format!(
                " {}/{} · Enter/Tab accept · ↑/↓ move · Esc",
                completion.selected.saturating_add(1),
                completion.filtered.len()
            ),
            theme::panel().fg(theme::muted()),
        ));
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme::panel())
                .block(crate::render::focused_panel_block(format!(
                    "Completion · {}/{}",
                    completion.selected.saturating_add(1),
                    completion.filtered.len()
                ))),
            popup,
        );
        if has_scrollbar {
            let mut state = ratatui::widgets::ScrollbarState::new(completion.filtered.len())
                .position(start)
                .viewport_content_length(visible);
            frame.render_stateful_widget(
                ratatui::widgets::Scrollbar::new(
                    ratatui::widgets::ScrollbarOrientation::VerticalRight,
                )
                .style(theme::panel().fg(theme::border_color()))
                .thumb_style(theme::panel().fg(theme::accent())),
                Rect {
                    x: popup.right().saturating_sub(1),
                    y: popup.y.saturating_add(1),
                    width: 1,
                    height: popup.height.saturating_sub(2),
                },
                &mut state,
            );
        }
        if let Some((title, body)) = detail_panel {
            let side = Rect {
                x: popup.x.saturating_add(width).saturating_add(gap),
                y: popup.y,
                width: side_width,
                height,
            };
            frame.render_widget(Clear, side);
            frame.render_widget(
                Paragraph::new(body)
                    .style(theme::panel())
                    .wrap(ratatui::widgets::Wrap { trim: false })
                    .block(crate::render::panel_block(Line::styled(
                        title,
                        theme::panel().fg(theme::accent()),
                    ))),
                side,
            );
        }
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

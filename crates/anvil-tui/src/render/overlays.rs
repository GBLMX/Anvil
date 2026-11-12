//! Overlay rendering for completion, LSP, settings, and tool panels.

use super::*;
use crate::CallHierarchyDirection;

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
                    if model.tree.show_hidden {
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
        for name in ["Editor", "Search", "LSP", "Accessibility", "Appearance"] {
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
            let value = overlay.value(row);
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

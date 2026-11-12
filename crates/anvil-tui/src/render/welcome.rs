//! Start dashboard rendering.

use super::*;
use crate::theme;

pub(crate) fn welcome_recent_at(
    model: &Model,
    column: u16,
    row: u16,
    width: u16,
    height: u16,
) -> Option<usize> {
    if !model.start_page.welcome {
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

pub(crate) fn render_welcome(frame: &mut Frame, area: Rect, model: &Model) {
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
                &format!("workspace  {}", model.workspace.path.display()),
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
                .strip_prefix(&model.workspace.path)
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
    let workspaces = model.recent.workspaces.iter().take(3).collect::<Vec<_>>();
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

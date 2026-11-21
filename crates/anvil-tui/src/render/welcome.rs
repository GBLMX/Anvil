//! Start dashboard rendering.

use super::*;
use crate::start_page::{START_ACTIONS, StartWorkspaceState};
use crate::state::StartPageFocus;
use crate::theme;

const ACTION_HEADER_LINES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WelcomeHit {
    Action(usize),
    Continue,
    File(usize),
    Workspace(usize),
}

fn panel_rect(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).clamp(12, 92).min(area.width);
    let height = area.height.saturating_sub(2).clamp(4, 22).min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn recent_area(panel: Rect) -> Option<(Rect, bool)> {
    if panel.width >= 68 && panel.height >= 14 {
        let left_width = ((panel.width as usize * 2) / 5).clamp(26, 38) as u16;
        return Some((
            Rect {
                x: panel.x + left_width + 2,
                width: panel.width.saturating_sub(left_width + 2),
                ..panel
            },
            true,
        ));
    }
    if panel.height >= 16 {
        let action_height = (ACTION_HEADER_LINES + START_ACTIONS.len()) as u16 + 1;
        return Some((
            Rect {
                x: panel.x,
                y: panel.y.saturating_add(1).saturating_add(action_height),
                width: panel.width,
                height: panel.height.saturating_sub(action_height + 1),
            },
            false,
        ));
    }
    None
}

fn action_index(row: u16, panel: Rect) -> Option<usize> {
    let first = panel
        .y
        .saturating_add(1)
        .saturating_add(ACTION_HEADER_LINES as u16);
    if row < first {
        return None;
    }
    let index = usize::from(row - first);
    (index < START_ACTIONS.len()).then_some(index)
}

fn recent_rows(model: &Model) -> Vec<WelcomeHit> {
    let mut rows = Vec::new();
    if model.start_page.session_available {
        rows.push(WelcomeHit::Continue);
    }
    rows.push(WelcomeHit::File(usize::MAX));
    rows.extend((0..model.start_recent_files().len()).map(WelcomeHit::File));
    rows.push(WelcomeHit::File(usize::MAX));
    rows.push(WelcomeHit::Workspace(usize::MAX));
    rows.extend((0..model.start_recent_workspaces().len()).map(WelcomeHit::Workspace));
    rows
}

pub(crate) fn welcome_hit_at(
    model: &Model,
    column: u16,
    row: u16,
    area: Rect,
) -> Option<WelcomeHit> {
    if !model.start_page.welcome {
        return None;
    }
    let panel = panel_rect(area);
    if column <= panel.x || column >= panel.right().saturating_sub(1) {
        return None;
    }
    if panel.width >= 68 && panel.height >= 14 {
        let left_width = ((panel.width as usize * 2) / 5).clamp(26, 38) as u16;
        if column < panel.x + left_width {
            return action_index(row, panel).map(WelcomeHit::Action);
        }
    } else if let Some(index) = action_index(row, panel) {
        return Some(WelcomeHit::Action(index));
    }
    let (recent, wide) = recent_area(panel)?;
    if wide && (column <= recent.x || column >= recent.right().saturating_sub(1)) {
        return None;
    }
    let first = recent.y.saturating_add(1);
    if row < first {
        return None;
    }
    recent_rows(model)
        .get(usize::from(row - first))
        .copied()
        .filter(|hit| {
            !matches!(
                hit,
                WelcomeHit::File(usize::MAX) | WelcomeHit::Workspace(usize::MAX)
            )
        })
}

pub(crate) fn render_welcome(frame: &mut Frame, area: Rect, model: &Model) {
    let panel = panel_rect(area);
    frame.render_widget(Clear, panel);
    if panel.width >= 68 && panel.height >= 14 {
        let gap = 2;
        let left_width = ((panel.width as usize * 2) / 5).clamp(26, 38) as u16;
        let left = Rect {
            width: left_width,
            ..panel
        };
        let right = Rect {
            x: panel.x + left_width + gap,
            width: panel.width.saturating_sub(left_width + gap),
            ..panel
        };
        render_panel(
            frame,
            left,
            "Start",
            welcome_actions(model, usize::from(left.width.saturating_sub(2))),
        );
        render_panel(
            frame,
            right,
            "Recent",
            welcome_recent(model, usize::from(right.width.saturating_sub(2))),
        );
    } else {
        let mut lines = welcome_actions(model, usize::from(panel.width.saturating_sub(2)));
        lines.push(Line::styled("", theme::panel()));
        lines.extend(welcome_recent(
            model,
            usize::from(panel.width.saturating_sub(2)),
        ));
        render_panel(frame, panel, "Start", lines);
    }
}

fn render_panel(frame: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'_>>) {
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::panel())
            .block(panel_block(title.to_owned())),
        area,
    );
}

fn selected_style(selected: bool) -> Style {
    if selected {
        theme::selected()
    } else {
        theme::panel()
    }
}

fn welcome_actions(model: &Model, width: usize) -> Vec<Line<'static>> {
    let key_style = Style::default()
        .fg(theme::accent())
        .add_modifier(Modifier::BOLD);
    let muted = theme::panel().fg(theme::muted());
    let mut lines = vec![
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
        Line::styled(
            "─".repeat(width.saturating_sub(2).min(34)),
            theme::panel().fg(theme::dim()),
        ),
        Line::styled("", theme::panel()),
    ];
    for (index, action) in START_ACTIONS.iter().enumerate() {
        let selected = model.start_page.focus == StartPageFocus::Actions
            && model.start_page.action_selected == index;
        let base = selected_style(selected);
        let key = model.start_action_key(action);
        let reason = model.start_action_disabled_reason(action.action);
        let label = match reason {
            Some(reason) => format!("{} · {reason}", action.label),
            None => action.label.to_owned(),
        };
        let label = truncate_width(
            &label,
            u16::try_from(width.saturating_sub(9)).unwrap_or(u16::MAX),
        );
        let marker = if selected { crate::art::SELECTED } else { " " };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker}{key:<8}"), key_style.patch(base)),
            Span::styled(label, base),
        ]));
    }
    lines.push(Line::styled("", theme::panel()));
    lines.push(Line::styled(
        truncate_width(
            &format!("workspace  {}", model.workspace.path.display()),
            u16::try_from(width.saturating_sub(2)).unwrap_or(u16::MAX),
        ),
        muted,
    ));
    lines
}

fn welcome_recent(model: &Model, width: usize) -> Vec<Line<'static>> {
    let muted = theme::panel().fg(theme::muted());
    let dim = theme::panel().fg(theme::dim());
    let files = model.start_recent_files();
    let workspaces = model.start_recent_workspaces();
    let mut lines = Vec::new();
    if model.start_page.session_available {
        let selected =
            model.start_page.focus == StartPageFocus::Files && model.start_page.file_selected == 0;
        lines.push(selected_line(" Continue", selected));
        lines.push(Line::styled("", theme::panel()));
    }
    lines.push(Line::styled("FILES", dim));
    if files.is_empty() {
        lines.push(Line::styled(" Space f find file", muted));
    } else {
        for (index, file) in files.iter().enumerate() {
            let selected = model.start_page.focus == StartPageFocus::Files
                && model.start_page.file_selected
                    == index + usize::from(model.start_page.session_available);
            let suffix = if file.missing { " [missing]" } else { "" };
            let text = format!(" {}{suffix}", file.label);
            lines.push(selected_line(
                &truncate_width(&text, u16::try_from(width).unwrap_or(u16::MAX)),
                selected,
            ));
        }
    }
    lines.push(Line::styled("", theme::panel()));
    lines.push(Line::styled("WORKSPACES", dim));
    if workspaces.is_empty() {
        lines.push(Line::styled(" no recent workspaces · w", muted));
    } else {
        for (index, workspace) in workspaces.iter().enumerate() {
            let selected = model.start_page.focus == StartPageFocus::Workspaces
                && model.start_page.workspace_selected == index;
            let state = match workspace.state {
                StartWorkspaceState::Current => " [current]",
                StartWorkspaceState::Available => "",
                StartWorkspaceState::Missing => " [missing]",
                StartWorkspaceState::Untrusted => " [untrusted]",
            };
            let text = format!(" {}{state} · {}", workspace.label, workspace.detail);
            lines.push(selected_line(
                &truncate_width(&text, u16::try_from(width).unwrap_or(u16::MAX)),
                selected,
            ));
        }
    }
    lines.push(Line::styled("", theme::panel()));
    lines.push(Line::styled("r recent files · w workspaces", dim));
    lines
}

fn selected_line(text: &str, selected: bool) -> Line<'static> {
    let marker = if selected { crate::art::SELECTED } else { " " };
    Line::styled(format!("{marker}{text}"), selected_style(selected))
}

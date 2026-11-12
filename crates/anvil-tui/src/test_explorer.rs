//! Test failure explorer overlay with jump-to-failure.

use super::*;
use crate::theme;
use anvil_session::ui::Problem;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

#[derive(Clone, Debug)]
pub(crate) struct TestExplorerState {
    pub(crate) items: Vec<Problem>,
    pub(crate) selected: usize,
}

impl TestExplorerState {
    pub(crate) fn new(items: Vec<Problem>) -> Self {
        Self { items, selected: 0 }
    }

    pub(crate) fn move_by(&mut self, delta: isize) {
        if self.items.is_empty() {
            self.selected = 0;
            return;
        }
        let last = self.items.len() - 1;
        self.selected = if delta < 0 {
            self.selected.saturating_sub(delta.unsigned_abs())
        } else {
            (self.selected + delta.unsigned_abs()).min(last)
        };
    }

    pub(crate) fn selected(&self) -> Option<&Problem> {
        self.items.get(self.selected)
    }
}

impl Model {
    pub(crate) fn move_test_explorer(&mut self, delta: isize) {
        let Some(explorer) = self.overlays.test_explorer.as_mut() else {
            return;
        };
        explorer.move_by(delta);
        self.dirty = true;
    }

    pub(crate) fn close_test_explorer(&mut self) {
        self.overlays.test_explorer = None;
        self.dirty = true;
    }

    pub(crate) fn jump_test_failure(&mut self) {
        let Some(problem) = self
            .overlays
            .test_explorer
            .as_ref()
            .and_then(TestExplorerState::selected)
            .cloned()
        else {
            self.status.text = "no test failures".to_owned();
            self.dirty = true;
            return;
        };
        if self.path.as_deref() != Some(problem.path.as_path())
            && let Err(error) = self.open_path(&problem.path)
        {
            self.status.text = format!("E: {error}");
            self.dirty = true;
            return;
        }
        self.cursor = self.offset_at_utf16(problem.line as usize, problem.character as usize);
        self.ui.focus.tree = false;
        self.ui.focus.split = false;
        self.overlays.test_explorer = None;
        self.ui.diagnostic_peek = None;
        self.status.text = format!(
            "{}:{}: {}",
            problem.path.display(),
            problem.line + 1,
            problem.message
        );
        self.dirty = true;
    }
}

pub(crate) fn render(model: &Model, frame: &mut Frame, area: Rect) {
    let Some(explorer) = &model.overlays.test_explorer else {
        return;
    };
    let width = (area.width * 3 / 4)
        .max(36)
        .min(area.width.saturating_sub(2));
    let height = (explorer.items.len() as u16 + 3)
        .clamp(6, 18)
        .min(area.height.saturating_sub(2));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);
    let content_width = width.saturating_sub(4) as usize;
    let mut lines = Vec::new();
    let mut current_path = None;
    let mut selected_line = None;
    for (index, problem) in explorer.items.iter().enumerate() {
        if current_path.as_ref() != Some(&problem.path) {
            current_path = Some(problem.path.clone());
            lines.push(Line::styled(
                problem.path.display().to_string(),
                theme::panel().fg(theme::accent()),
            ));
        }
        let message = crate::status_line::middle_ellipsis(&problem.message, content_width);
        let text = format!("  {}:{} {message}", problem.line + 1, problem.character + 1);
        if index == explorer.selected {
            selected_line = Some(lines.len());
            lines.push(Line::styled(text, theme::selected()));
        } else {
            lines.push(Line::styled(text, theme::panel()));
        }
    }
    if lines.is_empty() {
        lines.push(Line::styled(
            "no located test failures",
            theme::panel().fg(theme::muted()),
        ));
    }
    let visible = height.saturating_sub(2) as usize;
    let start = selected_line
        .unwrap_or(0)
        .saturating_sub(visible.saturating_sub(1));
    let visible_lines: Vec<Line<'static>> = lines.into_iter().skip(start).take(visible).collect();
    frame.render_widget(
        Paragraph::new(visible_lines).style(theme::panel()).block(
            ratatui::widgets::Block::bordered()
                .border_type(ratatui::widgets::BorderType::Rounded)
                .title(Span::styled(
                    "Test failures",
                    Style::default()
                        .fg(theme::accent())
                        .add_modifier(Modifier::BOLD),
                ))
                .border_style(theme::border(true)),
        ),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(line: u32) -> Problem {
        Problem {
            path: PathBuf::from("/tmp/test.rs"),
            line,
            character: 0,
            severity: None,
            source: Some("test".to_owned()),
            code: None,
            message: format!("failure {line}"),
        }
    }

    #[test]
    fn selection_clamps() {
        let mut explorer = TestExplorerState::new(vec![problem(0), problem(1)]);
        explorer.move_by(5);
        assert_eq!(explorer.selected, 1);
        explorer.move_by(-5);
        assert_eq!(explorer.selected, 0);
    }
}

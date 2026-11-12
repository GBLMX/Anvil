//! Full theme browser with live preview and cancel ownership.

use super::*;
use crate::theme::{self, Choice};

#[derive(Clone, Debug)]
pub(crate) struct ThemeBrowser {
    original: Choice,
    pub(crate) selected: usize,
}

impl Model {
    pub(crate) fn open_theme_browser(&mut self) {
        if self.overlays.theme_browser.is_some() {
            return;
        }
        let names = theme::names();
        let selected = names
            .iter()
            .position(|name| *name == theme::name())
            .unwrap_or(0);
        self.overlays.theme_browser = Some(ThemeBrowser {
            original: theme::choice(),
            selected,
        });
        self.status = "themes · Enter apply · Esc cancel".to_owned();
        self.dirty = true;
    }

    pub(crate) fn move_theme_browser(&mut self, delta: isize) {
        let Some(browser) = self.overlays.theme_browser.as_mut() else {
            return;
        };
        let names = theme::names();
        if names.is_empty() {
            return;
        }
        browser.selected = browser
            .selected
            .saturating_add_signed(delta)
            .min(names.len() - 1);
        if let Some(name) = names.get(browser.selected) {
            let _ = theme::apply_choice(Choice::Fixed(name), false);
        }
        self.dirty = true;
    }

    pub(crate) fn apply_theme_browser(&mut self) {
        let Some(browser) = self.overlays.theme_browser.take() else {
            return;
        };
        let names = theme::names();
        let Some(name) = names.get(browser.selected) else {
            return;
        };
        self.status = match theme::set_theme(name) {
            Ok(()) => {
                let status = format!("theme: {}", theme::name());
                self.persist_theme_status(status)
            }
            Err(error) => format!("E: {error}"),
        };
        self.dirty = true;
    }

    pub(crate) fn cancel_theme_browser(&mut self) {
        let Some(browser) = self.overlays.theme_browser.take() else {
            return;
        };
        let _ = theme::apply_choice(browser.original, false);
        self.status = "theme preview cancelled".to_owned();
        self.dirty = true;
    }
}

pub(crate) fn rect(area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let names = theme::names();
    let width = (area.width * 4 / 5)
        .max(48)
        .min(area.width.saturating_sub(2));
    let height = (names.len() as u16 + 5)
        .clamp(10, 22)
        .min(area.height.saturating_sub(2));
    ratatui::layout::Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

pub(crate) fn render(model: &Model, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let Some(browser) = &model.overlays.theme_browser else {
        return;
    };
    let names = theme::names();
    let popup = rect(area);
    frame.render_widget(ratatui::widgets::Clear, popup);
    let gap = 2;
    let left_width = (popup.width / 3)
        .max(20)
        .min(popup.width.saturating_sub(30));
    let left = ratatui::layout::Rect {
        width: left_width,
        ..popup
    };
    let right = ratatui::layout::Rect {
        x: popup.x + left_width + gap,
        width: popup.width.saturating_sub(left_width + gap),
        ..popup
    };
    let mut rows = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let style = if index == browser.selected {
            theme::selected()
        } else {
            theme::panel()
        };
        rows.push(ratatui::text::Line::styled(format!(" {name}"), style));
    }
    frame.render_widget(
        ratatui::widgets::Paragraph::new(rows)
            .style(theme::panel())
            .block(crate::render::focused_panel_block("Themes")),
        left,
    );
    let sample = theme::name();
    let lines = vec![
        ratatui::text::Line::styled(
            format!("{sample} preview"),
            theme::panel()
                .fg(theme::accent())
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        ratatui::text::Line::styled("", theme::panel()),
        ratatui::text::Line::styled(
            "surface   panel   elevated",
            theme::panel().fg(theme::muted()),
        ),
        ratatui::text::Line::styled("fn main() { let value = 42; }", theme::text()),
        ratatui::text::Line::styled(
            "// comment  \"string\"  123",
            theme::panel().fg(theme::syntax_comment()),
        ),
        ratatui::text::Line::styled(
            "● error   ▲ warning   ✓ success",
            theme::panel().fg(theme::muted()),
        ),
        ratatui::text::Line::styled("", theme::panel()),
        ratatui::text::Line::styled(
            format!("{}  ·  Enter apply  ·  Esc cancel", theme::name()),
            theme::panel().fg(theme::muted()),
        ),
    ];
    frame.render_widget(
        ratatui::widgets::Paragraph::new(lines)
            .style(theme::panel())
            .block(crate::render::focused_panel_block("Preview")),
        right,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_previews_cancels_and_applies() {
        theme::set_startup_theme("anvil-dark").expect("reset");
        let mut model = Model::new(EditorState::from_text("x"), None);
        model.execute_command("themes");
        assert!(model.overlays.theme_browser.is_some());
        model.move_theme_browser(1);
        assert_ne!(theme::name(), "anvil-dark");
        model.cancel_theme_browser();
        assert_eq!(theme::name(), "anvil-dark");
        model.open_theme_browser();
        model.move_theme_browser(1);
        model.apply_theme_browser();
        assert_ne!(theme::name(), "anvil-dark");
        theme::set_startup_theme("anvil-dark").expect("reset");
    }
}

//! Full theme browser with live preview and cancel ownership.

use super::*;
use crate::theme::{self, Choice};

#[derive(Clone, Debug)]
pub(crate) struct ThemeBrowser {
    original: Choice,
    pub(crate) selected: usize,
}

impl ThemeBrowser {
    pub(crate) fn new() -> Self {
        let names = theme::names();
        let selected = names
            .iter()
            .position(|name| *name == theme::name())
            .unwrap_or(0);
        Self {
            original: theme::choice(),
            selected,
        }
    }

    pub(crate) fn move_selection(&mut self, delta: isize) -> Option<&'static str> {
        let names = theme::names();
        if names.is_empty() {
            return None;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(names.len() - 1);
        names.get(self.selected).copied()
    }

    pub(crate) fn selected_name(&self) -> Option<&'static str> {
        theme::names().get(self.selected).copied()
    }

    pub(crate) fn original(&self) -> Choice {
        self.original
    }
}

pub(crate) enum ThemeBrowserEffect {
    Open {
        browser: ThemeBrowser,
        warnings: Vec<String>,
    },
    Preview(Option<&'static str>),
    Apply {
        name: Option<&'static str>,
    },
    Cancel {
        original: Choice,
    },
}

pub(crate) fn open() -> ThemeBrowserEffect {
    ThemeBrowserEffect::Open {
        browser: ThemeBrowser::new(),
        warnings: theme::custom_theme_warnings(),
    }
}

pub(crate) fn move_selection(browser: &mut ThemeBrowser, delta: isize) -> ThemeBrowserEffect {
    ThemeBrowserEffect::Preview(browser.move_selection(delta))
}

pub(crate) fn apply(browser: ThemeBrowser) -> ThemeBrowserEffect {
    ThemeBrowserEffect::Apply {
        name: browser.selected_name(),
    }
}

pub(crate) fn cancel(browser: ThemeBrowser) -> ThemeBrowserEffect {
    ThemeBrowserEffect::Cancel {
        original: browser.original(),
    }
}

pub(crate) fn apply_effect(model: &mut Model, effect: ThemeBrowserEffect) {
    match effect {
        ThemeBrowserEffect::Open { browser, warnings } => {
            if model.overlays.theme_browser.is_some() {
                return;
            }
            model.overlays.theme_browser = Some(browser);
            model.push_overlay(OverlayKind::ThemeBrowser);
            model.status.text = if warnings.is_empty() {
                "themes · Enter apply · Esc cancel".to_owned()
            } else {
                format!(
                    "themes · {} warning(s) · {} · Enter apply · Esc cancel",
                    warnings.len(),
                    warnings[0]
                )
            };
        }
        ThemeBrowserEffect::Preview(Some(name)) => {
            let _ = theme::apply_choice(Choice::Fixed(name), false);
        }
        ThemeBrowserEffect::Preview(None) => {}
        ThemeBrowserEffect::Apply { name } => {
            model.close_overlay(OverlayKind::ThemeBrowser);
            let Some(name) = name else {
                return;
            };
            model.status.text = match theme::set_theme(name) {
                Ok(()) => {
                    let status = format!("theme: {}", theme::name());
                    model.persist_theme_status(status)
                }
                Err(error) => format!("E: {error}"),
            };
        }
        ThemeBrowserEffect::Cancel { original } => {
            model.close_overlay(OverlayKind::ThemeBrowser);
            let _ = theme::apply_choice(original, false);
            model.status.text = "theme preview cancelled".to_owned();
        }
    }
    model.file.dirty = true;
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

fn list_start(selected: usize, len: usize, visible: usize) -> usize {
    if len == 0 || visible == 0 {
        return 0;
    }
    selected
        .saturating_sub(visible.saturating_sub(1))
        .min(len.saturating_sub(visible))
}

fn list_area(area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let popup = rect(area);
    let left_width = (popup.width / 3)
        .max(20)
        .min(popup.width.saturating_sub(30));
    ratatui::layout::Rect {
        width: left_width,
        ..popup
    }
}

pub(crate) fn hit(
    browser: &ThemeBrowser,
    area: ratatui::layout::Rect,
    col: u16,
    row: u16,
) -> Option<usize> {
    let left = list_area(area);
    let inner = ratatui::layout::Rect {
        x: left.x + 1,
        y: left.y + 1,
        width: left.width.saturating_sub(2),
        height: left.height.saturating_sub(2),
    };
    if col < inner.x || col >= inner.right() || row < inner.y || row >= inner.bottom() {
        return None;
    }
    let names = theme::names();
    let visible = usize::from(inner.height).max(1);
    let start = list_start(browser.selected, names.len(), visible);
    let index = start + usize::from(row - inner.y);
    (index < names.len()).then_some(index)
}

pub(crate) fn render(
    browser: &ThemeBrowser,
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
) {
    let names = theme::names();
    let popup = rect(area);
    frame.render_widget(ratatui::widgets::Clear, popup);
    let gap = 2;
    let left = list_area(area);
    let right = ratatui::layout::Rect {
        x: left.right() + gap,
        width: popup.width.saturating_sub(left.width + gap),
        ..popup
    };
    let visible = usize::from(left.height.saturating_sub(2)).max(1);
    let start = list_start(browser.selected, names.len(), visible);
    let mut rows = Vec::with_capacity(visible.min(names.len()));
    for (index, name) in names.iter().enumerate().skip(start).take(visible) {
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
    fn theme_list_scrolls_to_selection() {
        assert_eq!(list_start(0, 25, 20), 0);
        assert_eq!(list_start(19, 25, 20), 0);
        assert_eq!(list_start(20, 25, 20), 1);
        assert_eq!(list_start(24, 25, 20), 5);
    }

    #[test]
    fn theme_browser_state_clamps_selection_and_keeps_original() {
        theme::set_startup_theme("anvil-dark").expect("reset");
        let mut browser = ThemeBrowser::new();
        let original = browser.original();
        let first = theme::names().first().copied();
        let last = theme::names().last().copied();
        assert_eq!(browser.move_selection(-1000), first);
        assert_eq!(browser.move_selection(1000), last);
        assert_eq!(browser.original(), original);
        assert_eq!(browser.selected_name(), last);
        theme::set_startup_theme("anvil-dark").expect("reset");
    }

    #[test]
    fn theme_list_hit_maps_visible_rows() {
        let area = ratatui::layout::Rect::new(0, 0, 100, 30);
        let mut browser = ThemeBrowser::new();
        browser.selected = 0;
        let left = list_area(area);
        let first = hit(&browser, area, left.x + 1, left.y + 1).expect("first row");
        assert_eq!(first, 0);
        let second = hit(&browser, area, left.x + 1, left.y + 2).expect("second row");
        assert_eq!(second, 1);
        assert!(hit(&browser, area, left.x, left.y + 1).is_none());
    }

    #[test]
    fn theme_browser_state_owns_selection_and_original() {
        theme::set_startup_theme("anvil-dark").expect("reset");
        let mut browser = ThemeBrowser::new();
        let original = browser.original();
        assert_eq!(browser.selected_name(), Some("anvil-dark"));
        let next = browser.move_selection(1).expect("next theme");
        assert_eq!(browser.selected_name(), Some(next));
        assert_eq!(browser.original(), original);
    }
}

//! Minimal settings overlay and runtime edits.

use super::*;
use crate::settings::{self, Settings};

pub(crate) const SETTINGS_ROWS: usize = 10;

pub(crate) fn rect(area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let width = (area.width * 9 / 10)
        .max(56)
        .min(area.width.saturating_sub(2));
    let height = (area.height * 4 / 5)
        .max(16)
        .min(area.height.saturating_sub(2));
    ratatui::layout::Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SettingsOverlayState {
    pub(crate) selected: usize,
    pub(crate) original: Settings,
    pub(crate) draft: Settings,
    pub(crate) theme: theme::Choice,
    pub(crate) opened_theme: theme::Choice,
    pub(crate) theme_touched: bool,
}

impl SettingsOverlayState {
    pub(crate) fn label(index: usize) -> &'static str {
        match index {
            0 => "editor.scroll_margin",
            1 => "editor.auto_save",
            2 => "editor.soft_wrap",
            3 => "editor.format_on_save",
            4 => "editor.max_buffers",
            5 => "search.case_sensitive",
            6 => "lsp.inlay_hints",
            7 => "ui.reduced_motion",
            8 => "ui.high_contrast",
            _ => "theme.name",
        }
    }

    pub(crate) fn description(index: usize) -> &'static str {
        match index {
            0 => "Lines kept visible around the cursor before scrolling.",
            1 => "Save the active buffer when focus moves to another surface.",
            2 => "Wrap long lines to the pane width and scroll by display row.",
            3 => "Run LSP formatting after a successful write.",
            4 => "Maximum foreground buffers; Alt-1..9 still select the first nine.",
            5 => "Use case-sensitive matching for `/` and project search.",
            6 => "Request and render language-server inlay hints.",
            7 => "Freeze spinners and avoid continuous animation.",
            8 => "Increase text, muted, and border contrast for the active theme.",
            _ => "Select the semantic palette used by the TUI.",
        }
    }

    pub(crate) fn group(index: usize) -> &'static str {
        match index {
            0..=4 => "Editor",
            5 => "Search",
            6 => "LSP",
            7..=8 => "Accessibility",
            _ => "Appearance",
        }
    }

    pub(crate) fn tab(index: usize) -> &'static str {
        match Self::group(index) {
            "Editor" => "Editor",
            "Search" | "LSP" => "Language",
            _ => "Interface",
        }
    }

    pub(crate) fn value(&self, index: usize) -> String {
        if index == 9 {
            return theme_label(self.theme);
        }
        Self::value_from(&self.draft, index)
    }

    fn value_from(settings: &Settings, index: usize) -> String {
        match index {
            0 => settings.editor.scroll_margin.to_string(),
            1 => settings.editor.auto_save.to_string(),
            2 => settings.editor.soft_wrap.to_string(),
            3 => settings.editor.format_on_save.to_string(),
            4 => settings.editor.max_buffers.to_string(),
            5 => settings.search.case_sensitive.to_string(),
            6 => settings.lsp.inlay_hints.to_string(),
            7 => settings.ui.reduced_motion.to_string(),
            8 => settings.ui.high_contrast.to_string(),
            _ => theme_label(settings.theme),
        }
    }

    pub(crate) fn changed(&self, index: usize) -> bool {
        if index == 9 {
            return self.theme_touched;
        }
        self.value(index) != Self::value_from(&self.original, index)
    }
}
fn theme_label(choice: theme::Choice) -> String {
    match choice {
        theme::Choice::Fixed(name) => name.to_owned(),
        theme::Choice::Auto { dark, light } => format!("auto ({dark} / {light})"),
    }
}
fn cycle_theme(choice: theme::Choice, delta: isize) -> theme::Choice {
    let names = theme::names();
    if names.is_empty() {
        return choice;
    }
    let current = match choice {
        theme::Choice::Fixed(name) => names.iter().position(|candidate| *candidate == name),
        theme::Choice::Auto { .. } => None,
    };
    let index = match current {
        Some(index) if delta < 0 => (index + names.len() - 1) % names.len(),
        Some(index) => (index + 1) % names.len(),
        None if delta < 0 => names.len() - 1,
        None => 0,
    };
    theme::Choice::Fixed(names[index])
}

pub(crate) fn hit(area: ratatui::layout::Rect, col: u16, row: u16) -> Option<usize> {
    let popup = rect(area);
    if !crate::render::rect_contains(popup, col, row) {
        return None;
    }
    let inner = ratatui::layout::Rect {
        x: popup.x + 2,
        y: popup.y + 1,
        width: popup.width.saturating_sub(4),
        height: popup.height.saturating_sub(2),
    };
    if inner.width < 20 || inner.height < 6 {
        return None;
    }
    if row == inner.y {
        let x = col.saturating_sub(inner.x);
        if x < 8 {
            return Some(0);
        }
        if (9..20).contains(&x) {
            return Some(5);
        }
        if (21..31).contains(&x) {
            return Some(7);
        }
    }
    let body = ratatui::layout::Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(4),
        ..inner
    };
    let sidebar_width = (inner.width / 5).clamp(18, 24);
    let sidebar = ratatui::layout::Rect {
        width: sidebar_width,
        ..body
    };
    if crate::render::rect_contains(sidebar, col, row)
        && col > sidebar.x
        && col < sidebar.right().saturating_sub(1)
        && row > sidebar.y
    {
        let index = usize::from(row.saturating_sub(sidebar.y).saturating_sub(1));
        return [0usize, 5, 6, 7, 9].get(index).copied();
    }
    None
}

impl Model {
    pub(crate) fn select_settings_row(&mut self, selected: usize) {
        if let Some(overlay) = self.overlays.settings_overlay.as_mut() {
            overlay.selected = selected.min(SETTINGS_ROWS - 1);
            self.file.dirty = true;
        }
    }

    pub(crate) fn cycle_settings_tab(&mut self, backwards: bool) {
        let Some(overlay) = self.overlays.settings_overlay.as_ref() else {
            return;
        };
        let tabs = [0usize, 4, 6];
        let current = tabs
            .iter()
            .position(|row| {
                SettingsOverlayState::tab(overlay.selected) == SettingsOverlayState::tab(*row)
            })
            .unwrap_or(0);
        let next = if backwards {
            (current + tabs.len() - 1) % tabs.len()
        } else {
            (current + 1) % tabs.len()
        };
        self.select_settings_row(tabs[next]);
    }

    pub(crate) fn open_settings(&mut self) {
        if self.overlays.settings_overlay.is_some() {
            return;
        }
        let active_theme = theme::choice();
        self.overlays.settings_overlay = Some(SettingsOverlayState {
            selected: 0,
            original: self.config.settings,
            draft: self.config.settings,
            theme: active_theme,
            opened_theme: active_theme,
            theme_touched: false,
        });
        self.push_overlay(OverlayKind::Settings);
        self.status.text = "settings".to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn cancel_settings(&mut self) {
        if let Some(overlay) = self.overlays.settings_overlay.take() {
            self.apply_settings(overlay.original);
            let _ = theme::apply_choice(overlay.opened_theme, false);
            self.close_overlay(OverlayKind::Settings);
            self.status.text = "settings cancelled".to_owned();
            self.file.dirty = true;
        }
    }

    pub(crate) fn move_settings(&mut self, delta: isize) {
        if let Some(overlay) = self.overlays.settings_overlay.as_mut() {
            overlay.selected = overlay
                .selected
                .saturating_add_signed(delta)
                .min(SETTINGS_ROWS - 1);
            self.file.dirty = true;
        }
    }

    pub(crate) fn adjust_settings(&mut self, delta: isize) {
        let Some(mut overlay) = self.overlays.settings_overlay.take() else {
            return;
        };
        if overlay.selected == 9 {
            overlay.theme = cycle_theme(overlay.theme, delta);
            overlay.theme_touched = true;
            let choice = overlay.theme;
            self.overlays.settings_overlay = Some(overlay);
            let _ = theme::apply_choice(choice, false);
            self.file.dirty = true;
            return;
        }
        let mut draft = overlay.draft;
        match overlay.selected {
            0 => {
                draft.editor.scroll_margin = draft
                    .editor
                    .scroll_margin
                    .saturating_add_signed(delta)
                    .min(settings::MAX_SCROLL_MARGIN);
            }
            1 => draft.editor.auto_save = !draft.editor.auto_save,
            2 => draft.editor.soft_wrap = !draft.editor.soft_wrap,
            3 => draft.editor.format_on_save = !draft.editor.format_on_save,
            4 => {
                draft.editor.max_buffers = draft
                    .editor
                    .max_buffers
                    .saturating_add_signed(delta)
                    .clamp(1, settings::MAX_MAX_BUFFERS);
            }
            5 => draft.search.case_sensitive = !draft.search.case_sensitive,
            6 => draft.lsp.inlay_hints = !draft.lsp.inlay_hints,
            7 => draft.ui.reduced_motion = !draft.ui.reduced_motion,
            _ => draft.ui.high_contrast = !draft.ui.high_contrast,
        }
        overlay.draft = draft;
        self.apply_settings(draft);
        self.overlays.settings_overlay = Some(overlay);
    }

    pub(crate) fn reset_settings_row(&mut self) {
        let Some(mut overlay) = self.overlays.settings_overlay.take() else {
            return;
        };
        let defaults = Settings::default();
        if overlay.selected == 9 {
            overlay.theme = defaults.theme;
            overlay.theme_touched = true;
            let choice = overlay.theme;
            self.overlays.settings_overlay = Some(overlay);
            let _ = theme::apply_choice(choice, false);
            self.file.dirty = true;
            return;
        }
        match overlay.selected {
            0 => overlay.draft.editor.scroll_margin = defaults.editor.scroll_margin,
            1 => overlay.draft.editor.auto_save = defaults.editor.auto_save,
            2 => overlay.draft.editor.soft_wrap = defaults.editor.soft_wrap,
            3 => overlay.draft.editor.format_on_save = defaults.editor.format_on_save,
            4 => overlay.draft.editor.max_buffers = defaults.editor.max_buffers,
            5 => overlay.draft.search.case_sensitive = defaults.search.case_sensitive,
            6 => overlay.draft.lsp.inlay_hints = defaults.lsp.inlay_hints,
            7 => overlay.draft.ui.reduced_motion = defaults.ui.reduced_motion,
            _ => overlay.draft.ui.high_contrast = defaults.ui.high_contrast,
        }
        self.apply_settings(overlay.draft);
        self.overlays.settings_overlay = Some(overlay);
    }

    pub(crate) fn save_settings(&mut self) {
        let Some(mut overlay) = self.overlays.settings_overlay.take() else {
            return;
        };
        if overlay.theme_touched {
            overlay.draft.theme = overlay.theme;
        }
        match settings::save(overlay.draft) {
            Ok(path) => {
                self.apply_settings(overlay.draft);
                overlay.original = overlay.draft;
                if overlay.theme_touched {
                    overlay.opened_theme = overlay.theme;
                }
                overlay.theme_touched = false;
                self.overlays.settings_overlay = Some(overlay);
                self.status.text = format!("settings saved: {}", path.display());
            }
            Err(error) => {
                self.overlays.settings_overlay = Some(overlay);
                self.status.text = format!("E: settings: {error}");
            }
        }
        self.file.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_hit_test_maps_tabs_and_sections() {
        let area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 30,
        };
        let popup = rect(area);
        let inner_y = popup.y + 1;
        assert_eq!(hit(area, popup.x + 4, inner_y), Some(0));
        assert_eq!(hit(area, popup.x + 14, inner_y), Some(5));
        assert_eq!(hit(area, popup.x + 25, inner_y), Some(7));
        let body_y = inner_y + 2;
        let sidebar_x = popup.x + 3;
        assert_eq!(hit(area, sidebar_x, body_y + 1), Some(0));
        assert_eq!(hit(area, sidebar_x, body_y + 2), Some(5));
        assert_eq!(hit(area, sidebar_x, body_y + 3), Some(6));
        assert_eq!(hit(area, sidebar_x, body_y + 4), Some(7));
        assert_eq!(hit(area, sidebar_x, body_y + 5), Some(9));
    }

    #[test]
    fn settings_tabs_map_every_group() {
        assert_eq!(SettingsOverlayState::tab(0), "Editor");
        assert_eq!(SettingsOverlayState::tab(4), "Editor");
        assert_eq!(SettingsOverlayState::tab(5), "Language");
        assert_eq!(SettingsOverlayState::tab(6), "Language");
        assert_eq!(SettingsOverlayState::tab(7), "Interface");
        assert_eq!(SettingsOverlayState::tab(9), "Interface");
        assert_eq!(SettingsOverlayState::group(9), "Appearance");
    }

    #[test]
    fn overlay_edits_apply_and_cancel_restores() {
        let mut model = Model::new(crate::EditorState::from_text("x"), None);
        model.open_settings();
        assert!(model.overlays.settings_overlay.is_some());
        model.adjust_settings(1);
        assert_eq!(model.config.settings.editor.scroll_margin, 4);
        assert!(
            model
                .overlays
                .settings_overlay
                .as_ref()
                .expect("overlay")
                .changed(0)
        );
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(model.config.settings.editor.auto_save);
        model.reset_settings_row();
        assert!(!model.config.settings.editor.auto_save);
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(model.config.settings.editor.soft_wrap);
        model.reset_settings_row();
        assert!(!model.config.settings.editor.soft_wrap);
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(model.config.settings.editor.format_on_save);
        model.reset_settings_row();
        assert!(!model.config.settings.editor.format_on_save);
        model.move_settings(1);
        model.adjust_settings(1);
        assert_eq!(model.config.settings.editor.max_buffers, 10);
        model.reset_settings_row();
        assert_eq!(model.config.settings.editor.max_buffers, 9);
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(model.config.settings.search.case_sensitive);
        model.reset_settings_row();
        assert!(!model.config.settings.search.case_sensitive);
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(!model.config.settings.lsp.inlay_hints);
        model.reset_settings_row();
        assert!(model.config.settings.lsp.inlay_hints);
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(model.config.settings.ui.reduced_motion);
        model.reset_settings_row();
        assert!(!model.config.settings.ui.reduced_motion);
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(model.config.settings.ui.high_contrast);
        model.reset_settings_row();
        assert!(!model.config.settings.ui.high_contrast);
        model.cancel_settings();
        assert!(model.overlays.settings_overlay.is_none());
        assert_eq!(model.config.settings, Settings::default());
    }

    #[test]
    fn settings_edits_preserve_active_startup_theme() {
        theme::set_startup_theme("nord").expect("active theme");
        let mut model = Model::new(crate::EditorState::from_text("x"), None);
        model.open_settings();
        model.adjust_settings(1);
        assert_eq!(model.config.settings.editor.scroll_margin, 4);
        assert_eq!(theme::name(), "nord");
        model.cancel_settings();
        assert_eq!(theme::name(), "nord");
        theme::set_startup_theme("anvil-dark").expect("reset theme");
    }

    #[test]
    fn theme_row_cycles_builtins_and_resets() {
        let first = theme::Choice::Fixed("anvil-dark");
        let next = cycle_theme(first, 1);
        assert_ne!(next, first);
        assert_eq!(cycle_theme(next, -1), first);
        assert_eq!(
            cycle_theme(
                theme::Choice::Auto {
                    dark: "anvil-dark",
                    light: "solarized-light",
                },
                1,
            ),
            first
        );
        assert_eq!(theme_label(first), "anvil-dark");
    }
}

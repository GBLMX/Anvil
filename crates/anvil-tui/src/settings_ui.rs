//! Minimal settings overlay and runtime edits.

use super::*;
use crate::settings::{self, Settings};

pub(crate) const SETTINGS_ROWS: usize = 8;

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
}

impl SettingsOverlayState {
    pub(crate) fn label(index: usize) -> &'static str {
        match index {
            0 => "editor.scroll_margin",
            1 => "editor.auto_save",
            2 => "editor.soft_wrap",
            3 => "editor.format_on_save",
            4 => "search.case_sensitive",
            5 => "lsp.inlay_hints",
            6 => "ui.reduced_motion",
            _ => "ui.high_contrast",
        }
    }

    pub(crate) fn description(index: usize) -> &'static str {
        match index {
            0 => "Lines kept visible around the cursor before scrolling.",
            1 => "Save the active buffer when focus moves to another surface.",
            2 => "Wrap long lines to the pane width and scroll by display row.",
            3 => "Run LSP formatting after a successful write.",
            4 => "Use case-sensitive matching for `/` and project search.",
            5 => "Request and render language-server inlay hints.",
            6 => "Freeze spinners and avoid continuous animation.",
            _ => "Increase text, muted, and border contrast for the active theme.",
        }
    }

    pub(crate) fn group(index: usize) -> &'static str {
        match index {
            0..=3 => "Editor",
            4 => "Search",
            5 => "LSP",
            _ => "Accessibility",
        }
    }

    pub(crate) fn value(settings: &Settings, index: usize) -> String {
        match index {
            0 => settings.editor.scroll_margin.to_string(),
            1 => settings.editor.auto_save.to_string(),
            2 => settings.editor.soft_wrap.to_string(),
            3 => settings.editor.format_on_save.to_string(),
            4 => settings.search.case_sensitive.to_string(),
            5 => settings.lsp.inlay_hints.to_string(),
            6 => settings.ui.reduced_motion.to_string(),
            _ => settings.ui.high_contrast.to_string(),
        }
    }

    pub(crate) fn changed(&self, index: usize) -> bool {
        Self::value(&self.draft, index) != Self::value(&self.original, index)
    }
}

impl Model {
    pub(crate) fn open_settings(&mut self) {
        if self.settings_overlay.is_some() {
            return;
        }
        self.settings_overlay = Some(SettingsOverlayState {
            selected: 0,
            original: self.settings,
            draft: self.settings,
        });
        self.status = "settings".to_owned();
        self.dirty = true;
    }

    pub(crate) fn cancel_settings(&mut self) {
        if let Some(overlay) = self.settings_overlay.take() {
            self.apply_settings(overlay.original);
            self.status = "settings cancelled".to_owned();
            self.dirty = true;
        }
    }

    pub(crate) fn move_settings(&mut self, delta: isize) {
        if let Some(overlay) = self.settings_overlay.as_mut() {
            overlay.selected = overlay
                .selected
                .saturating_add_signed(delta)
                .min(SETTINGS_ROWS - 1);
            self.dirty = true;
        }
    }

    pub(crate) fn adjust_settings(&mut self, delta: isize) {
        let Some(mut overlay) = self.settings_overlay.take() else {
            return;
        };
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
            4 => draft.search.case_sensitive = !draft.search.case_sensitive,
            5 => draft.lsp.inlay_hints = !draft.lsp.inlay_hints,
            6 => draft.ui.reduced_motion = !draft.ui.reduced_motion,
            _ => draft.ui.high_contrast = !draft.ui.high_contrast,
        }
        overlay.draft = draft;
        self.apply_settings(draft);
        self.settings_overlay = Some(overlay);
    }

    pub(crate) fn reset_settings_row(&mut self) {
        let Some(mut overlay) = self.settings_overlay.take() else {
            return;
        };
        let defaults = Settings::default();
        match overlay.selected {
            0 => overlay.draft.editor.scroll_margin = defaults.editor.scroll_margin,
            1 => overlay.draft.editor.auto_save = defaults.editor.auto_save,
            2 => overlay.draft.editor.soft_wrap = defaults.editor.soft_wrap,
            3 => overlay.draft.editor.format_on_save = defaults.editor.format_on_save,
            4 => overlay.draft.search.case_sensitive = defaults.search.case_sensitive,
            5 => overlay.draft.lsp.inlay_hints = defaults.lsp.inlay_hints,
            6 => overlay.draft.ui.reduced_motion = defaults.ui.reduced_motion,
            _ => overlay.draft.ui.high_contrast = defaults.ui.high_contrast,
        }
        self.apply_settings(overlay.draft);
        self.settings_overlay = Some(overlay);
    }

    pub(crate) fn save_settings(&mut self) {
        let Some(mut overlay) = self.settings_overlay.take() else {
            return;
        };
        match settings::save(overlay.draft) {
            Ok(path) => {
                self.apply_settings(overlay.draft);
                overlay.original = overlay.draft;
                self.settings_overlay = Some(overlay);
                self.status = format!("settings saved: {}", path.display());
            }
            Err(error) => {
                self.settings_overlay = Some(overlay);
                self.status = format!("E: settings: {error}");
            }
        }
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_edits_apply_and_cancel_restores() {
        let mut model = Model::new(crate::EditorState::from_text("x"), None);
        model.open_settings();
        assert!(model.settings_overlay.is_some());
        model.adjust_settings(1);
        assert_eq!(model.settings.editor.scroll_margin, 4);
        assert!(model.settings_overlay.as_ref().expect("overlay").changed(0));
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(model.settings.editor.auto_save);
        model.reset_settings_row();
        assert!(!model.settings.editor.auto_save);
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(model.settings.editor.soft_wrap);
        model.reset_settings_row();
        assert!(!model.settings.editor.soft_wrap);
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(model.settings.editor.format_on_save);
        model.reset_settings_row();
        assert!(!model.settings.editor.format_on_save);
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(model.settings.search.case_sensitive);
        model.reset_settings_row();
        assert!(!model.settings.search.case_sensitive);
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(!model.settings.lsp.inlay_hints);
        model.reset_settings_row();
        assert!(model.settings.lsp.inlay_hints);
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(model.settings.ui.reduced_motion);
        model.reset_settings_row();
        assert!(!model.settings.ui.reduced_motion);
        model.move_settings(1);
        model.adjust_settings(1);
        assert!(model.settings.ui.high_contrast);
        model.reset_settings_row();
        assert!(!model.settings.ui.high_contrast);
        model.cancel_settings();
        assert!(model.settings_overlay.is_none());
        assert_eq!(model.settings, Settings::default());
    }
}

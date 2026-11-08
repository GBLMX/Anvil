//! Minimal settings overlay and runtime edits.

use super::*;
use crate::settings::{self, Settings};

pub(crate) const SETTINGS_ROWS: usize = 3;

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
            1 => "search.case_sensitive",
            _ => "lsp.inlay_hints",
        }
    }

    pub(crate) fn value(settings: &Settings, index: usize) -> String {
        match index {
            0 => settings.editor.scroll_margin.to_string(),
            1 => settings.search.case_sensitive.to_string(),
            _ => settings.lsp.inlay_hints.to_string(),
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
            1 => draft.search.case_sensitive = !draft.search.case_sensitive,
            _ => draft.lsp.inlay_hints = !draft.lsp.inlay_hints,
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
            1 => overlay.draft.search.case_sensitive = defaults.search.case_sensitive,
            _ => overlay.draft.lsp.inlay_hints = defaults.lsp.inlay_hints,
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
        assert!(model.settings.search.case_sensitive);
        model.reset_settings_row();
        assert!(!model.settings.search.case_sensitive);
        model.cancel_settings();
        assert!(model.settings_overlay.is_none());
        assert_eq!(model.settings, Settings::default());
    }
}

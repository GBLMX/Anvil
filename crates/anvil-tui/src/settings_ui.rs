//! Minimal settings overlay and runtime edits.

use super::*;
use crate::settings::{self, Settings};
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Clear, Paragraph},
};

pub(crate) const SETTINGS_ROWS: usize = 11;

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
    pub(crate) user: settings::SettingsPatch,
    pub(crate) inherited: Settings,
    pub(crate) sources: settings::SettingsSources,
}

pub(crate) enum SettingsChange {
    Apply(Settings),
    Theme(theme::Choice),
}

pub(crate) enum SettingsAction {
    Select(usize),
    Move(isize),
    CycleTab(bool),
    Adjust(isize),
    Reset,
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
            9 => "editor.file_watching",
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
            9 => "Reload buffers, tree, Git, and LSP state on external changes.",
            _ => "Select the semantic palette used by the TUI.",
        }
    }

    pub(crate) fn group(index: usize) -> &'static str {
        match index {
            0..=4 | 9 => "Editor",
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
        if index == 10 {
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
            9 => settings.editor.file_watching.to_string(),
            _ => theme_label(settings.theme),
        }
    }

    pub(crate) fn changed(&self, index: usize) -> bool {
        if index == 10 {
            return self.theme_touched;
        }
        self.value(index) != Self::value_from(&self.original, index)
    }

    pub(crate) fn select(&mut self, selected: usize) {
        self.selected = selected.min(SETTINGS_ROWS - 1);
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(SETTINGS_ROWS - 1);
    }

    pub(crate) fn cycle_tab(&mut self, backwards: bool) {
        let tabs = [0usize, 4, 6];
        let current = tabs
            .iter()
            .position(|row| Self::tab(self.selected) == Self::tab(*row))
            .unwrap_or(0);
        let next = if backwards {
            (current + tabs.len() - 1) % tabs.len()
        } else {
            (current + 1) % tabs.len()
        };
        self.selected = tabs[next];
    }

    pub(crate) fn adjust(&mut self, delta: isize) -> Option<SettingsChange> {
        if self.selected == 10 {
            self.theme = cycle_theme(self.theme, delta);
            self.theme_touched = true;
            self.user.set_theme(self.theme);
            return Some(SettingsChange::Theme(self.theme));
        }
        let mut draft = self.draft;
        match self.selected {
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
            9 => draft.editor.file_watching = !draft.editor.file_watching,
            _ => draft.ui.high_contrast = !draft.ui.high_contrast,
        }
        self.draft = draft;
        self.user.set_field(settings_key(self.selected), &draft);
        Some(SettingsChange::Apply(draft))
    }

    pub(crate) fn reset(&mut self) -> Option<SettingsChange> {
        if self.selected == 10 {
            self.draft.theme = self.inherited.theme;
            self.theme = self.inherited.theme;
            self.theme_touched = false;
            self.user.clear_field(settings::SettingsKey::Theme);
            return Some(SettingsChange::Theme(self.theme));
        }
        match self.selected {
            0 => self.draft.editor.scroll_margin = self.inherited.editor.scroll_margin,
            1 => self.draft.editor.auto_save = self.inherited.editor.auto_save,
            2 => self.draft.editor.soft_wrap = self.inherited.editor.soft_wrap,
            3 => self.draft.editor.format_on_save = self.inherited.editor.format_on_save,
            4 => self.draft.editor.max_buffers = self.inherited.editor.max_buffers,
            5 => self.draft.search.case_sensitive = self.inherited.search.case_sensitive,
            6 => self.draft.lsp.inlay_hints = self.inherited.lsp.inlay_hints,
            7 => self.draft.ui.reduced_motion = self.inherited.ui.reduced_motion,
            9 => self.draft.editor.file_watching = self.inherited.editor.file_watching,
            _ => self.draft.ui.high_contrast = self.inherited.ui.high_contrast,
        }
        self.user.clear_field(settings_key(self.selected));
        Some(SettingsChange::Apply(self.draft))
    }

    pub(crate) fn update(&mut self, action: SettingsAction) -> Option<SettingsChange> {
        match action {
            SettingsAction::Select(selected) => {
                self.select(selected);
                None
            }
            SettingsAction::Move(delta) => {
                self.move_selection(delta);
                None
            }
            SettingsAction::CycleTab(backwards) => {
                self.cycle_tab(backwards);
                None
            }
            SettingsAction::Adjust(delta) => self.adjust(delta),
            SettingsAction::Reset => self.reset(),
        }
    }
}
fn settings_key(index: usize) -> settings::SettingsKey {
    match index {
        0 => settings::SettingsKey::ScrollMargin,
        1 => settings::SettingsKey::AutoSave,
        2 => settings::SettingsKey::SoftWrap,
        3 => settings::SettingsKey::FormatOnSave,
        4 => settings::SettingsKey::MaxBuffers,
        5 => settings::SettingsKey::CaseSensitive,
        6 => settings::SettingsKey::InlayHints,
        7 => settings::SettingsKey::ReducedMotion,
        8 => settings::SettingsKey::HighContrast,
        9 => settings::SettingsKey::FileWatching,
        _ => settings::SettingsKey::Theme,
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

/// Paints the settings page from page state alone.
///
/// Returns `false` when the popup is too small to hold the page; the caller
/// stops rendering the remaining overlays, matching the previous inline
/// early-return.
pub(crate) fn render(overlay: &SettingsOverlayState, frame: &mut Frame, area: Rect) -> bool {
    let popup = rect(area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        crate::render::focused_panel_block("Settings · s save · Esc cancel"),
        popup,
    );
    let inner = Rect {
        x: popup.x + 2,
        y: popup.y + 1,
        width: popup.width.saturating_sub(4),
        height: popup.height.saturating_sub(2),
    };
    if inner.width < 20 || inner.height < 6 {
        return false;
    }
    let group = SettingsOverlayState::group(overlay.selected);
    let active_tab = SettingsOverlayState::tab(overlay.selected);
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
        height: inner.height.saturating_sub(5),
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
            .block(crate::render::panel_block("Sections")),
        sidebar,
    );
    let mut lines = Vec::new();
    let mut rendered_group = "";
    for row in 0..SETTINGS_ROWS {
        let next = SettingsOverlayState::group(row);
        if next != rendered_group {
            rendered_group = next;
            lines.push(Line::styled(
                rendered_group.to_ascii_uppercase(),
                theme::panel().fg(theme::dim()).add_modifier(Modifier::BOLD),
            ));
        }
        let marker = if overlay.changed(row) { "*" } else { " " };
        let label = SettingsOverlayState::label(row);
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
        .saturating_add(SETTINGS_ROWS / 2)
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
        .block(crate::render::panel_block("Values")),
        content,
    );
    let keys = Rect {
        y: inner.y + inner.height.saturating_sub(2),
        height: 1,
        ..inner
    };
    frame.render_widget(
        Paragraph::new(Line::styled(
            "↑/↓ or j/k select · ←/→ or h/l change · r reset · s save · Esc cancel",
            theme::panel().fg(theme::dim()),
        )),
        keys,
    );
    let description = Rect {
        y: inner.y + inner.height.saturating_sub(1),
        height: 1,
        ..inner
    };
    let mut source = overlay.sources.label(settings_key(overlay.selected));
    for (path, reason) in overlay.sources.ignored() {
        source.push_str(&format!("; ignored {} ({reason})", path.display()));
    }
    frame.render_widget(
        Paragraph::new(Line::styled(
            format!(
                "{} · source: {source}",
                SettingsOverlayState::description(overlay.selected)
            ),
            theme::panel().fg(theme::muted()),
        )),
        description,
    );
    true
}

impl Model {
    pub(crate) fn select_settings_row(&mut self, selected: usize) {
        if let Some(overlay) = self.overlays.settings_overlay.as_mut() {
            overlay.update(SettingsAction::Select(selected));
            self.file.dirty = true;
        }
    }

    pub(crate) fn cycle_settings_tab(&mut self, backwards: bool) {
        if let Some(overlay) = self.overlays.settings_overlay.as_mut() {
            overlay.update(SettingsAction::CycleTab(backwards));
            self.file.dirty = true;
        }
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
            user: self.config.user_settings.clone(),
            inherited: self.config.inherited_settings,
            sources: self.config.sources.clone(),
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
            overlay.update(SettingsAction::Move(delta));
            self.file.dirty = true;
        }
    }

    pub(crate) fn adjust_settings(&mut self, delta: isize) {
        let Some(mut overlay) = self.overlays.settings_overlay.take() else {
            return;
        };
        let Some(change) = overlay.update(SettingsAction::Adjust(delta)) else {
            self.overlays.settings_overlay = Some(overlay);
            return;
        };
        self.overlays.settings_overlay = Some(overlay);
        match change {
            SettingsChange::Theme(choice) => {
                let _ = theme::apply_choice(choice, false);
                self.file.dirty = true;
            }
            SettingsChange::Apply(settings) => self.apply_settings(settings),
        }
    }

    pub(crate) fn reset_settings_row(&mut self) {
        let Some(mut overlay) = self.overlays.settings_overlay.take() else {
            return;
        };
        let Some(change) = overlay.update(SettingsAction::Reset) else {
            self.overlays.settings_overlay = Some(overlay);
            return;
        };
        self.overlays.settings_overlay = Some(overlay);
        match change {
            SettingsChange::Theme(choice) => {
                let _ = theme::apply_choice(choice, false);
                self.file.dirty = true;
            }
            SettingsChange::Apply(settings) => self.apply_settings(settings),
        }
    }

    pub(crate) fn save_settings(&mut self) {
        let Some(mut overlay) = self.overlays.settings_overlay.take() else {
            return;
        };
        if overlay.theme_touched {
            overlay.draft.theme = overlay.theme;
            overlay.user.set_theme(overlay.theme);
        }
        match settings::save_patch(&overlay.user) {
            Ok(path) => {
                self.apply_settings(overlay.draft);
                self.config.user_settings = overlay.user.clone();
                self.reload_config();
                let reload_status = self.status.text.clone();
                overlay.user = self.config.user_settings.clone();
                overlay.inherited = self.config.inherited_settings;
                overlay.sources = self.config.sources.clone();
                overlay.original = self.config.settings;
                overlay.draft = self.config.settings;
                overlay.theme = theme::choice();
                overlay.opened_theme = overlay.theme;
                overlay.theme_touched = false;
                self.overlays.settings_overlay = Some(overlay);
                self.status.text = format!("settings saved: {}; {reload_status}", path.display());
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
mod tests;

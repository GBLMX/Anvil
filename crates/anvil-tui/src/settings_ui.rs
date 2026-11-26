//! Minimal settings overlay and runtime edits.

use super::*;
use crate::settings::{self, Settings};
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Clear, Paragraph},
};

pub(crate) const SETTINGS_ROWS: usize = settings::SETTINGS.len();
pub(crate) const TABS: [&str; 3] = ["Editor", "Interface", "Language"];
pub(crate) const SECTIONS: [&str; 5] = ["Editor", "Search", "LSP", "Accessibility", "Appearance"];

/// Shared geometry for rendering and mouse hit-testing.
pub(crate) struct SettingsLayout {
    pub(crate) popup: Rect,
    pub(crate) inner: Rect,
    pub(crate) body: Rect,
    pub(crate) sidebar: Rect,
    pub(crate) content: Rect,
}

pub(crate) fn layout(area: Rect) -> Option<SettingsLayout> {
    let popup = rect(area);
    let inner = Rect {
        x: popup.x + 2,
        y: popup.y + 1,
        width: popup.width.saturating_sub(4),
        height: popup.height.saturating_sub(2),
    };
    if inner.width < 20 || inner.height < 6 {
        return None;
    }
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
    Some(SettingsLayout {
        popup,
        inner,
        body,
        sidebar,
        content,
    })
}

/// Display width of one rendered tab, including its surrounding spaces.
fn tab_width(label: &str) -> u16 {
    unicode_width::UnicodeWidthStr::width(label) as u16 + 2
}

/// Column offset of tab `index` inside `inner`, separators included.
pub(crate) fn tab_offset(index: usize) -> u16 {
    TABS[..index].iter().map(|tab| tab_width(tab) + 1).sum()
}

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
    pub(crate) query: String,
    pub(crate) querying: bool,
    pub(crate) filtered: Vec<usize>,
    pub(crate) original: Settings,
    pub(crate) draft: Settings,
    pub(crate) theme: theme::Choice,
    pub(crate) opened_theme: theme::Choice,
    pub(crate) theme_touched: bool,
    pub(crate) user: settings::SettingsPatch,
    pub(crate) project: settings::SettingsPatch,
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
    BeginQuery,
    EndQuery,
    AppendQuery(char),
    PopQuery,
    ClearQuery,
}

impl SettingsOverlayState {
    fn key(index: usize) -> settings::SettingsKey {
        settings::SETTINGS[index.min(SETTINGS_ROWS - 1)].key
    }

    fn integer_bounds(index: usize) -> (usize, usize) {
        match settings::SETTINGS[index.min(SETTINGS_ROWS - 1)].kind {
            settings::SettingKind::Integer { min, max } => (min as usize, max as usize),
            settings::SettingKind::Bool | settings::SettingKind::Theme => (0, 0),
        }
    }

    pub(crate) fn label(index: usize) -> &'static str {
        settings::SETTINGS[index.min(SETTINGS_ROWS - 1)].label
    }

    pub(crate) fn description(index: usize) -> &'static str {
        settings::SETTINGS[index.min(SETTINGS_ROWS - 1)].description
    }

    pub(crate) fn group(index: usize) -> &'static str {
        settings::SETTINGS[index.min(SETTINGS_ROWS - 1)].group
    }

    pub(crate) fn tab(index: usize) -> &'static str {
        settings::SETTINGS[index.min(SETTINGS_ROWS - 1)].tab
    }

    pub(crate) fn value(&self, index: usize) -> String {
        if Self::key(index) == settings::SettingsKey::Theme {
            return theme_label(self.theme);
        }
        Self::value_from(&self.draft, index)
    }

    fn value_from(settings: &Settings, index: usize) -> String {
        match Self::key(index) {
            settings::SettingsKey::ScrollMargin => settings.editor.scroll_margin.to_string(),
            settings::SettingsKey::AutoSave => settings.editor.auto_save.to_string(),
            settings::SettingsKey::SoftWrap => settings.editor.soft_wrap.to_string(),
            settings::SettingsKey::FormatOnSave => settings.editor.format_on_save.to_string(),
            settings::SettingsKey::MaxBuffers => settings.editor.max_buffers.to_string(),
            settings::SettingsKey::SidebarPercent => settings.editor.sidebar_percent.to_string(),
            settings::SettingsKey::CaseSensitive => settings.search.case_sensitive.to_string(),
            settings::SettingsKey::InlayHints => settings.lsp.inlay_hints.to_string(),
            settings::SettingsKey::AutoBrackets => settings.completion.auto_brackets.to_string(),
            settings::SettingsKey::ReducedMotion => settings.ui.reduced_motion.to_string(),
            settings::SettingsKey::HighContrast => settings.ui.high_contrast.to_string(),
            settings::SettingsKey::FileWatching => settings.editor.file_watching.to_string(),
            settings::SettingsKey::Theme => theme_label(settings.theme),
        }
    }

    pub(crate) fn changed(&self, index: usize) -> bool {
        if Self::key(index) == settings::SettingsKey::Theme {
            return self.theme_touched;
        }
        self.value(index) != Self::value_from(&self.original, index)
    }

    pub(crate) fn refilter(&mut self) {
        let query = self.query.to_lowercase();
        let terms = query
            .split_whitespace()
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();
        self.filtered = (0..SETTINGS_ROWS)
            .filter(|&index| {
                let def = &settings::SETTINGS[index];
                let haystack = format!(
                    "{} {} {} {}",
                    def.path, def.label, def.description, def.group
                )
                .to_lowercase();
                terms.iter().all(|term| match *term {
                    "@modified" => self.changed(index),
                    "@explicit" => self
                        .project
                        .value(def.key)
                        .or_else(|| self.user.value(def.key))
                        .is_some(),
                    "@language" => def.scope() == settings::SettingScope::Language,
                    source if source.starts_with("@source:") => {
                        let source = &source["@source:".len()..];
                        self.sources.label(def.key).to_lowercase().contains(source)
                    }
                    text => haystack.contains(text),
                })
            })
            .collect();
        if !self.filtered.contains(&self.selected) {
            self.selected = self.filtered.first().copied().unwrap_or(0);
        }
    }

    pub(crate) fn begin_query(&mut self) {
        self.querying = true;
    }

    pub(crate) fn end_query(&mut self) {
        self.querying = false;
    }

    pub(crate) fn append_query(&mut self, character: char) {
        self.query.push(character);
        self.refilter();
    }

    pub(crate) fn pop_query(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub(crate) fn clear_query(&mut self) {
        self.query.clear();
        self.querying = false;
        self.refilter();
    }

    pub(crate) fn select(&mut self, selected: usize) {
        if self.filtered.contains(&selected) {
            self.selected = selected;
        }
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let Some(position) = self.filtered.iter().position(|row| *row == self.selected) else {
            self.selected = self.filtered.first().copied().unwrap_or(0);
            return;
        };
        let next = position
            .saturating_add_signed(delta)
            .min(self.filtered.len().saturating_sub(1));
        self.selected = self.filtered[next];
    }

    pub(crate) fn display_rows(&self) -> Vec<(Option<&'static str>, usize)> {
        let mut rows = Vec::new();
        let mut group = "";
        for index in &self.filtered {
            let next = Self::group(*index);
            if next != group {
                group = next;
                rows.push((Some(group), *index));
            }
            rows.push((None, *index));
        }
        rows
    }

    pub(crate) fn cycle_tab(&mut self, backwards: bool) {
        self.clear_query();
        let tabs = ["Editor", "Interface", "Language"]
            .into_iter()
            .filter_map(|tab| settings::SETTINGS.iter().position(|def| def.tab == tab))
            .collect::<Vec<_>>();
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
        if self.filtered.is_empty() {
            return None;
        }
        let key = Self::key(self.selected);
        if key == settings::SettingsKey::Theme {
            self.theme = cycle_theme(self.theme, delta);
            self.theme_touched = true;
            self.user.set_theme(self.theme);
            return Some(SettingsChange::Theme(self.theme));
        }
        let mut draft = self.draft;
        match key {
            settings::SettingsKey::ScrollMargin => {
                let (min, max) = Self::integer_bounds(self.selected);
                draft.editor.scroll_margin = draft
                    .editor
                    .scroll_margin
                    .saturating_add_signed(delta)
                    .clamp(min, max);
            }
            settings::SettingsKey::AutoSave => draft.editor.auto_save = !draft.editor.auto_save,
            settings::SettingsKey::SoftWrap => draft.editor.soft_wrap = !draft.editor.soft_wrap,
            settings::SettingsKey::FormatOnSave => {
                draft.editor.format_on_save = !draft.editor.format_on_save
            }
            settings::SettingsKey::MaxBuffers => {
                let (min, max) = Self::integer_bounds(self.selected);
                draft.editor.max_buffers = draft
                    .editor
                    .max_buffers
                    .saturating_add_signed(delta)
                    .clamp(min, max);
            }
            settings::SettingsKey::SidebarPercent => {
                let (min, max) = Self::integer_bounds(self.selected);
                draft.editor.sidebar_percent = (draft.editor.sidebar_percent as i16)
                    .saturating_add(delta as i16)
                    .clamp(min as i16, max as i16)
                    as u16;
            }
            settings::SettingsKey::CaseSensitive => {
                draft.search.case_sensitive = !draft.search.case_sensitive
            }
            settings::SettingsKey::InlayHints => draft.lsp.inlay_hints = !draft.lsp.inlay_hints,
            settings::SettingsKey::AutoBrackets => {
                draft.completion.auto_brackets = !draft.completion.auto_brackets
            }
            settings::SettingsKey::ReducedMotion => {
                draft.ui.reduced_motion = !draft.ui.reduced_motion
            }
            settings::SettingsKey::HighContrast => draft.ui.high_contrast = !draft.ui.high_contrast,
            settings::SettingsKey::FileWatching => {
                draft.editor.file_watching = !draft.editor.file_watching
            }
            settings::SettingsKey::Theme => unreachable!(),
        }
        self.draft = draft;
        self.user.set_field(key, &draft);
        self.refilter();
        Some(SettingsChange::Apply(draft))
    }

    pub(crate) fn reset(&mut self) -> Option<SettingsChange> {
        if self.filtered.is_empty() {
            return None;
        }
        let key = Self::key(self.selected);
        if key == settings::SettingsKey::Theme {
            self.draft.theme = self.inherited.theme;
            self.theme = self.inherited.theme;
            self.theme_touched = false;
            self.user.clear_field(key);
            self.refilter();
            return Some(SettingsChange::Theme(self.theme));
        }
        match key {
            settings::SettingsKey::ScrollMargin => {
                self.draft.editor.scroll_margin = self.inherited.editor.scroll_margin
            }
            settings::SettingsKey::AutoSave => {
                self.draft.editor.auto_save = self.inherited.editor.auto_save
            }
            settings::SettingsKey::SoftWrap => {
                self.draft.editor.soft_wrap = self.inherited.editor.soft_wrap
            }
            settings::SettingsKey::FormatOnSave => {
                self.draft.editor.format_on_save = self.inherited.editor.format_on_save
            }
            settings::SettingsKey::MaxBuffers => {
                self.draft.editor.max_buffers = self.inherited.editor.max_buffers
            }
            settings::SettingsKey::SidebarPercent => {
                self.draft.editor.sidebar_percent = self.inherited.editor.sidebar_percent
            }
            settings::SettingsKey::CaseSensitive => {
                self.draft.search.case_sensitive = self.inherited.search.case_sensitive
            }
            settings::SettingsKey::InlayHints => {
                self.draft.lsp.inlay_hints = self.inherited.lsp.inlay_hints
            }
            settings::SettingsKey::AutoBrackets => {
                self.draft.completion.auto_brackets = self.inherited.completion.auto_brackets
            }
            settings::SettingsKey::ReducedMotion => {
                self.draft.ui.reduced_motion = self.inherited.ui.reduced_motion
            }
            settings::SettingsKey::HighContrast => {
                self.draft.ui.high_contrast = self.inherited.ui.high_contrast
            }
            settings::SettingsKey::FileWatching => {
                self.draft.editor.file_watching = self.inherited.editor.file_watching
            }
            settings::SettingsKey::Theme => unreachable!(),
        }
        self.user.clear_field(key);
        self.refilter();
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
            SettingsAction::BeginQuery => {
                self.begin_query();
                None
            }
            SettingsAction::EndQuery => {
                self.end_query();
                None
            }
            SettingsAction::AppendQuery(character) => {
                self.append_query(character);
                None
            }
            SettingsAction::PopQuery => {
                self.pop_query();
                None
            }
            SettingsAction::ClearQuery => {
                self.clear_query();
                None
            }
        }
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

pub(crate) fn hit(overlay: &SettingsOverlayState, area: Rect, col: u16, row: u16) -> Option<usize> {
    let layout = layout(area)?;
    if !crate::render::rect_contains(layout.popup, col, row) {
        return None;
    }
    if row == layout.inner.y {
        return tab_at(overlay, layout.inner.x, col);
    }
    let sidebar = layout.sidebar;
    if crate::render::rect_contains(sidebar, col, row)
        && col > sidebar.x
        && col < sidebar.right().saturating_sub(1)
        && row > sidebar.y
    {
        let index = usize::from(row.saturating_sub(sidebar.y).saturating_sub(1));
        return SECTIONS.get(index).and_then(|section| {
            overlay
                .filtered
                .iter()
                .copied()
                .find(|row| SettingsOverlayState::group(*row) == *section)
        });
    }
    None
}

fn tab_at(overlay: &SettingsOverlayState, inner_x: u16, col: u16) -> Option<usize> {
    let offset = col.saturating_sub(inner_x);
    TABS.iter().enumerate().find_map(|(index, tab)| {
        let start = tab_offset(index);
        if (start..start + tab_width(tab)).contains(&offset) {
            overlay
                .filtered
                .iter()
                .copied()
                .find(|row| SettingsOverlayState::tab(*row) == *tab)
        } else {
            None
        }
    })
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
    let Some(layout) = layout(area) else {
        return false;
    };
    let SettingsLayout {
        inner,
        body,
        sidebar,
        content,
        ..
    } = layout;
    let group = SettingsOverlayState::group(overlay.selected);
    let active_tab = SettingsOverlayState::tab(overlay.selected);
    let tab_style = |name: &str| {
        if name == active_tab {
            theme::panel()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD)
        } else {
            theme::panel().fg(theme::dim())
        }
    };
    let mut tab_spans = Vec::new();
    for (index, tab) in TABS.iter().enumerate() {
        if index > 0 {
            tab_spans.push(Span::styled("│", theme::panel().fg(theme::border_color())));
        }
        tab_spans.push(Span::styled(format!(" {tab} "), tab_style(tab)));
    }
    frame.render_widget(
        Paragraph::new(Line::from(tab_spans)).style(theme::panel()),
        Rect { height: 1, ..inner },
    );
    let query_line = if overlay.querying {
        format!("Search: {}_", overlay.query)
    } else if overlay.query.is_empty() {
        "Search: /".to_owned()
    } else {
        format!("Search: {}", overlay.query)
    };
    frame.render_widget(
        Paragraph::new(Line::styled(
            query_line,
            theme::panel().fg(if overlay.querying {
                theme::accent()
            } else {
                theme::muted()
            }),
        )),
        Rect {
            y: inner.y + 1,
            height: 1,
            ..inner
        },
    );
    let mut sidebar_lines = Vec::new();
    for name in SECTIONS {
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
    let display_rows = overlay.display_rows();
    let mut lines = Vec::new();
    for (header, row) in &display_rows {
        if let Some(header) = header {
            lines.push(Line::styled(
                header.to_ascii_uppercase(),
                theme::panel().fg(theme::dim()).add_modifier(Modifier::BOLD),
            ));
            continue;
        }
        let row = *row;
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
        let explicit = if overlay
            .project
            .value(SettingsOverlayState::key(row))
            .or_else(|| overlay.user.value(SettingsOverlayState::key(row)))
            .is_some()
        {
            " *explicit"
        } else {
            ""
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} {label:<27} "), style),
            Span::styled(value, value_style),
            Span::styled(explicit, theme::panel().fg(theme::muted())),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::styled(
            "no matching settings",
            theme::panel().fg(theme::muted()),
        ));
    }
    let selected_row = display_rows
        .iter()
        .position(|(header, row)| header.is_none() && *row == overlay.selected)
        .unwrap_or(0);
    let visible = body.height.saturating_sub(2) as usize;
    let start = selected_row
        .saturating_sub(visible / 2)
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
            "↑/↓ or j/k select · ←/→ or h/l change · / search · r reset · s save · Esc cancel",
            theme::panel().fg(theme::dim()),
        )),
        keys,
    );
    let description = Rect {
        y: inner.y + inner.height.saturating_sub(1),
        height: 1,
        ..inner
    };
    let mut source = overlay
        .sources
        .label(SettingsOverlayState::key(overlay.selected));
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
            query: String::new(),
            querying: false,
            filtered: (0..SETTINGS_ROWS).collect(),
            original: self.config.settings,
            draft: self.config.settings,
            theme: active_theme,
            opened_theme: active_theme,
            theme_touched: false,
            user: self.config.user_settings.clone(),
            project: self.config.project_settings.clone(),
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
    fn update_settings_query(&mut self, action: SettingsAction) {
        if let Some(overlay) = self.overlays.settings_overlay.as_mut() {
            overlay.update(action);
            self.file.dirty = true;
        }
    }

    pub(crate) fn begin_settings_query(&mut self) {
        self.update_settings_query(SettingsAction::BeginQuery);
    }

    pub(crate) fn end_settings_query(&mut self) {
        self.update_settings_query(SettingsAction::EndQuery);
    }

    pub(crate) fn append_settings_query(&mut self, character: char) {
        self.update_settings_query(SettingsAction::AppendQuery(character));
    }

    pub(crate) fn pop_settings_query(&mut self) {
        self.update_settings_query(SettingsAction::PopQuery);
    }

    pub(crate) fn clear_settings_query(&mut self) {
        self.update_settings_query(SettingsAction::ClearQuery);
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
                overlay.project = self.config.project_settings.clone();
                overlay.inherited = self.config.inherited_settings;
                overlay.sources = self.config.sources.clone();
                overlay.original = self.config.settings;
                overlay.draft = self.config.settings;
                overlay.theme = theme::choice();
                overlay.opened_theme = overlay.theme;
                overlay.theme_touched = false;
                overlay.refilter();
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

use super::*;

fn area() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 30,
    }
}

fn state_at(selected: usize) -> SettingsOverlayState {
    let defaults = Settings::default();
    SettingsOverlayState {
        selected,
        query: String::new(),
        querying: false,
        filtered: (0..SETTINGS_ROWS).collect(),
        original: defaults,
        draft: defaults,
        theme: theme::Choice::Fixed("anvil-dark"),
        opened_theme: theme::Choice::Fixed("anvil-dark"),
        theme_touched: false,
        user: Default::default(),
        project: Default::default(),
        inherited: defaults,
        sources: Default::default(),
    }
}

fn hit(area: Rect, col: u16, row: u16) -> Option<usize> {
    super::hit(&state_at(0), area, col, row)
}

fn rendered(state: &SettingsOverlayState, width: u16, height: u16) -> (Vec<Vec<String>>, bool) {
    use ratatui::{Terminal, backend::TestBackend};

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut complete = false;
    terminal
        .draw(|frame| {
            complete = render(state, frame, frame.area());
        })
        .expect("draw");
    let buffer = terminal.backend().buffer();
    let rows = buffer
        .content()
        .chunks(usize::from(width))
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol().to_owned())
                .collect::<Vec<_>>()
        })
        .collect();
    (rows, complete)
}

fn text_at(row: &[String], x: u16, len: usize) -> String {
    row[usize::from(x)..usize::from(x) + len].concat()
}

#[test]
fn settings_hit_test_maps_tabs() {
    let area = area();
    let layout = layout(area).expect("layout");
    for (index, tab) in TABS.iter().enumerate() {
        let col = layout.inner.x + tab_offset(index) + 1;
        let row = hit(area, col, layout.inner.y).expect(tab);
        assert_eq!(
            SettingsOverlayState::tab(row),
            *tab,
            "tab {tab} should select a row on its own tab"
        );
    }
    for index in 1..TABS.len() {
        let separator = layout.inner.x + tab_offset(index) - 1;
        assert_eq!(
            hit(area, separator, layout.inner.y),
            None,
            "tab separators are not clickable"
        );
    }
    assert_eq!(hit(area, layout.inner.x, layout.inner.y + 1), None);
}

#[test]
fn settings_hit_test_maps_sections() {
    let area = area();
    let layout = layout(area).expect("layout");
    for (index, section) in SECTIONS.iter().enumerate() {
        let col = layout.sidebar.x + 2;
        let row = layout.sidebar.y + 1 + index as u16;
        let hit_row = hit(area, col, row).expect(section);
        assert_eq!(
            SettingsOverlayState::group(hit_row),
            *section,
            "section {section} should select a row in its own group"
        );
    }
    assert_eq!(hit(area, layout.sidebar.x + 2, layout.sidebar.y), None);
    assert_eq!(hit(area, layout.sidebar.x, layout.sidebar.y + 1), None);
    let empty = layout.sidebar.y + 1 + SECTIONS.len() as u16;
    assert_eq!(hit(area, layout.sidebar.x + 2, empty), None);
}

#[test]
fn settings_hit_test_respects_active_filter() {
    let mut state = state_at(0);
    state.begin_query();
    for character in "@language".chars() {
        state.append_query(character);
    }
    let area = area();
    let layout = layout(area).expect("layout");
    let editor = super::hit(
        &state,
        area,
        layout.inner.x + tab_offset(0) + 1,
        layout.inner.y,
    )
    .expect("editor tab row");
    assert_eq!(editor, 2);
    let interface = super::hit(
        &state,
        area,
        layout.inner.x + tab_offset(1) + 1,
        layout.inner.y,
    );
    assert_eq!(interface, None);
    let search = super::hit(&state, area, layout.sidebar.x + 2, layout.sidebar.y + 1 + 1);
    assert_eq!(search, None);
}

#[test]
fn settings_hit_test_rejects_outside_and_tiny_areas() {
    let area = area();
    assert_eq!(hit(area, 0, 0), None);
    assert_eq!(hit(area, area.width - 1, area.height - 1), None);

    let tiny = Rect {
        x: 0,
        y: 0,
        width: 20,
        height: 10,
    };
    assert!(layout(tiny).is_none());
    assert_eq!(hit(tiny, 1, 1), None);
}

#[test]
fn settings_tabs_map_every_group() {
    assert_eq!(SettingsOverlayState::tab(0), "Editor");
    assert_eq!(SettingsOverlayState::tab(4), "Editor");
    assert_eq!(SettingsOverlayState::tab(5), "Editor");
    for index in 6..=13 {
        assert_eq!(SettingsOverlayState::tab(index), "Language");
    }
    assert_eq!(SettingsOverlayState::tab(14), "Interface");
    assert_eq!(SettingsOverlayState::tab(15), "Interface");
    assert_eq!(SettingsOverlayState::tab(16), "Editor");
    assert_eq!(SettingsOverlayState::tab(17), "Interface");
    assert_eq!(SettingsOverlayState::group(17), "Appearance");
}

#[test]
fn settings_tab_cycle_wraps_both_directions() {
    let mut state = state_at(0);
    for expected in [14, 6, 0] {
        state.cycle_tab(false);
        assert_eq!(state.selected, expected);
    }
    for expected in [6, 14, 0] {
        state.cycle_tab(true);
        assert_eq!(state.selected, expected);
    }
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

#[test]
fn integer_rows_clamp_and_bools_toggle() {
    let mut state = state_at(0);
    let settings::SettingKind::Integer { min, max } = settings::SETTINGS[0].kind else {
        panic!("editor.scroll_margin must be an integer row");
    };
    state.draft.editor.scroll_margin = min as usize;
    match state.adjust(-1).expect("low clamp") {
        SettingsChange::Apply(settings) => assert_eq!(settings.editor.scroll_margin as i64, min),
        SettingsChange::Theme(_) => panic!("expected draft change"),
    }
    state.draft.editor.scroll_margin = max as usize;
    match state.adjust(1).expect("high clamp") {
        SettingsChange::Apply(settings) => assert_eq!(settings.editor.scroll_margin as i64, max),
        SettingsChange::Theme(_) => panic!("expected draft change"),
    }

    state.select(1);
    let before = state.draft.editor.auto_save;
    match state.adjust(1).expect("toggle") {
        SettingsChange::Apply(settings) => assert_ne!(settings.editor.auto_save, before),
        SettingsChange::Theme(_) => panic!("expected draft change"),
    }
}

#[test]
fn reset_restores_draft_and_theme() {
    let mut state = state_at(0);
    let inherited = state.inherited;
    state.adjust(3).expect("adjust");
    assert!(state.changed(0));
    state.reset().expect("draft reset");
    assert_eq!(
        state.draft.editor.scroll_margin,
        inherited.editor.scroll_margin
    );
    assert!(!state.changed(0));

    state.select(17);
    state.adjust(1).expect("theme change");
    assert!(state.theme_touched);
    assert_ne!(state.theme, inherited.theme);
    match state.reset().expect("theme reset") {
        SettingsChange::Theme(choice) => assert_eq!(choice, inherited.theme),
        SettingsChange::Apply(_) => panic!("expected theme reset"),
    }
    assert!(!state.theme_touched);
    assert_eq!(state.theme, inherited.theme);
    assert_eq!(state.draft.theme, inherited.theme);
}

#[test]
fn update_returns_none_only_for_navigation_actions() {
    let mut state = state_at(0);
    assert!(state.update(SettingsAction::Select(6)).is_none());
    assert!(state.update(SettingsAction::Move(1)).is_none());
    assert!(state.update(SettingsAction::CycleTab(false)).is_none());
    assert!(state.update(SettingsAction::Adjust(1)).is_some());
    assert!(state.update(SettingsAction::Reset).is_some());
}

#[test]
fn query_filters_by_text_and_moves_within_filtered_rows() {
    let mut state = state_at(0);
    state.begin_query();
    for character in "soft".chars() {
        state.append_query(character);
    }
    assert_eq!(state.filtered, vec![2]);
    state.move_selection(1);
    assert_eq!(state.selected, 2);
    assert_eq!(
        state.update(SettingsAction::Adjust(1)).map(|_| ()),
        Some(())
    );
    state.clear_query();
    assert_eq!(state.filtered.len(), SETTINGS_ROWS);
}

#[test]
fn language_and_explicit_filters_use_registry_metadata() {
    let mut state = state_at(0);
    state.begin_query();
    for character in "@language".chars() {
        state.append_query(character);
    }
    assert_eq!(state.filtered, vec![2, 3]);

    state.clear_query();
    state
        .user
        .set_field(settings::SettingsKey::ScrollMargin, &state.draft);
    state.begin_query();
    for character in "@explicit".chars() {
        state.append_query(character);
    }
    assert_eq!(state.filtered, vec![0]);
}

#[test]
fn empty_filter_cannot_adjust_or_reset_hidden_row() {
    let mut state = state_at(0);
    state.begin_query();
    for character in "no-such-setting".chars() {
        state.append_query(character);
    }
    assert!(state.filtered.is_empty());
    assert!(state.adjust(1).is_none());
    assert!(state.reset().is_none());
    assert_eq!(state.draft, state.original);
}

#[test]
fn rendering_shows_active_search_query() {
    let mut state = state_at(0);
    state.begin_query();
    for character in "wrap".chars() {
        state.append_query(character);
    }
    let (rows, complete) = rendered(&state, 100, 30);
    assert!(complete);
    let screen = rows
        .iter()
        .map(|row| row.join(""))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(screen.contains("Search: wrap_"));
}

#[test]
fn settings_page_places_tabs_sections_and_values() {
    let mut state = state_at(6);
    state.draft.search.case_sensitive = true;
    state.theme_touched = true;
    let (rows, complete) = rendered(&state, 100, 30);
    assert!(complete, "render should complete");
    let layout = layout(area()).expect("layout");

    let title = rows[usize::from(layout.popup.y)].concat();
    assert!(
        title.contains("Settings · s save · Esc cancel"),
        "title missing: {title}"
    );

    let tab_row = &rows[usize::from(layout.inner.y)];
    let tabs = " Editor │ Interface │ Language ";
    assert_eq!(text_at(tab_row, layout.inner.x, tabs.chars().count()), tabs);

    for (index, section) in SECTIONS.iter().enumerate() {
        let row = &rows[usize::from(layout.sidebar.y + 1 + index as u16)];
        let expected = format!("  {section}");
        assert_eq!(
            text_at(row, layout.sidebar.x + 1, expected.len()),
            expected,
            "section {section} position"
        );
    }

    let screen: String = rows.iter().flatten().map(String::as_str).collect();
    for expected in [
        "SEARCH",
        "case_sensitive",
        "Use case-sensitive matching for `/` and project search.",
        "* search.case_sensitive",
        "source: default",
        "↑/↓ or j/k select · ←/→ or h/l change · / search · r reset · s save · Esc cancel",
    ] {
        assert!(screen.contains(expected), "missing {expected:?}");
    }
}

#[test]
fn settings_page_stops_when_popup_is_too_small() {
    let area = Rect {
        x: 0,
        y: 0,
        width: 20,
        height: 10,
    };
    assert!(layout(area).is_none());
    let (rows, complete) = rendered(&state_at(0), 20, 10);
    assert!(!complete);
    let screen: String = rows.iter().flatten().map(String::as_str).collect();
    assert!(screen.contains("Settings · s sa"), "{screen}");
    assert!(!screen.contains("Sections"), "{screen}");
    assert!(!screen.contains("editor.scroll_margin"), "{screen}");
}

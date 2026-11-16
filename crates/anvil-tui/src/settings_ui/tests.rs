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
    assert_eq!(SettingsOverlayState::tab(9), "Editor");
    assert_eq!(SettingsOverlayState::tab(10), "Interface");
    assert_eq!(SettingsOverlayState::group(10), "Appearance");
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
fn settings_state_owns_draft_and_theme_actions() {
    let defaults = Settings::default();
    let mut state = SettingsOverlayState {
        selected: 0,
        original: defaults,
        draft: defaults,
        theme: theme::Choice::Fixed("anvil-dark"),
        opened_theme: theme::Choice::Fixed("anvil-dark"),
        theme_touched: false,
        user: Default::default(),
        inherited: defaults,
        sources: Default::default(),
    };
    match state.adjust(1).expect("draft change") {
        SettingsChange::Apply(settings) => {
            assert_eq!(
                settings.editor.scroll_margin,
                defaults.editor.scroll_margin + 1
            );
        }
        SettingsChange::Theme(_) => panic!("expected draft change"),
    }
    state.select(10);
    match state.adjust(1).expect("theme change") {
        SettingsChange::Theme(choice) => assert_ne!(choice, defaults.theme),
        SettingsChange::Apply(_) => panic!("expected theme change"),
    }
    assert!(state.theme_touched);
    match state.reset().expect("theme reset") {
        SettingsChange::Theme(choice) => assert_eq!(choice, defaults.theme),
        SettingsChange::Apply(_) => panic!("expected theme reset"),
    }
}

fn state_at(selected: usize) -> SettingsOverlayState {
    let defaults = Settings::default();
    SettingsOverlayState {
        selected,
        original: defaults,
        draft: defaults,
        theme: theme::Choice::Fixed("anvil-dark"),
        opened_theme: theme::Choice::Fixed("anvil-dark"),
        theme_touched: false,
        user: Default::default(),
        inherited: defaults,
        sources: Default::default(),
    }
}

fn rendered_text(state: &SettingsOverlayState, width: u16, height: u16) -> (String, bool) {
    use ratatui::{Terminal, backend::TestBackend};

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut complete = false;
    terminal
        .draw(|frame| {
            complete = render(state, frame, frame.area());
        })
        .expect("draw");
    let text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    (text, complete)
}

#[test]
fn settings_page_renders_from_state_alone() {
    let mut state = state_at(5);
    state.draft.search.case_sensitive = true;
    state.theme_touched = true;
    let (text, complete) = rendered_text(&state, 100, 30);
    assert!(complete);
    for expected in [
        "Settings · s save · Esc cancel",
        "Editor",
        "Interface",
        "Language",
        "Sections",
        "Values",
        "SEARCH",
        "case_sensitive",
        "Use case-sensitive matching for `/` and project search.",
        "source: default",
        "↑/↓ or j/k select · ←/→ or h/l change · r reset · s save · Esc cancel",
    ] {
        assert!(text.contains(expected), "missing {expected:?}: {text}");
    }
    assert!(text.contains("* search.case_sensitive"), "{text}");
}

#[test]
fn settings_page_stops_when_popup_is_too_small() {
    let (text, complete) = rendered_text(&state_at(0), 20, 10);
    assert!(!complete);
    assert!(text.contains("Settings · s sa"), "{text}");
    assert!(!text.contains("Sections"), "{text}");
    assert!(!text.contains("editor.scroll_margin"), "{text}");
}

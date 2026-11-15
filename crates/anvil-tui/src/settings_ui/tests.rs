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
fn settings_theme_row_saves_choice() {
    let _guard = crate::settings::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory =
        std::env::temp_dir().join(format!("anvil-settings-theme-{}", std::process::id()));
    let path = directory.join("settings.toml");
    unsafe { std::env::set_var("ANVIL_SETTINGS", &path) };
    let mut model = Model::new(crate::EditorState::from_text("x"), None);
    model.open_settings();
    for _ in 0..9 {
        model.move_settings(1);
    }
    model.adjust_settings(1);
    let expected = model
        .overlays
        .settings_overlay
        .as_ref()
        .expect("overlay")
        .theme;
    assert_ne!(expected, theme::Choice::Fixed("anvil-dark"));
    model.save_settings();
    assert_eq!(model.config.settings.theme, expected);
    assert_eq!(
        settings::load_file_from_path(&path)
            .expect("settings")
            .theme,
        expected
    );
    unsafe { std::env::remove_var("ANVIL_SETTINGS") };
    std::fs::remove_dir_all(directory).expect("cleanup");
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
    state.select(9);
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

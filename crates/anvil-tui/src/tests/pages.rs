use super::*;
use crate::settings::{self, Settings};
use crate::theme;
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
    for _ in 0..10 {
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
#[test]
fn mouse_wheel_moves_theme_selection() {
    theme::set_startup_theme("anvil-dark").expect("reset");
    let mut model = Model::new(EditorState::from_text("x"), None);
    model.open_theme_browser();
    let selected = |model: &Model| {
        model
            .overlays
            .theme_browser
            .as_ref()
            .expect("theme browser")
            .selected
    };
    let before = selected(&model);
    let scroll = |kind| crossterm::event::MouseEvent {
        kind,
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    assert!(crate::mouse::handle_mouse(
        &mut model,
        scroll(crossterm::event::MouseEventKind::ScrollDown),
        120,
        40,
    ));
    assert_eq!(selected(&model), before + 1);
    assert!(crate::mouse::handle_mouse(
        &mut model,
        scroll(crossterm::event::MouseEventKind::ScrollUp),
        120,
        40,
    ));
    assert_eq!(selected(&model), before);
    theme::set_startup_theme("anvil-dark").expect("reset");
}

#[test]
fn theme_browser_renders_the_selected_tail() {
    theme::set_startup_theme("anvil-dark").expect("reset");
    let mut model = Model::new(EditorState::from_text("x"), None);
    model.open_theme_browser();
    model.move_theme_browser(theme::names().len() as isize);
    let last = *theme::names().last().expect("theme");
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).expect("terminal");
    terminal
        .draw(|frame| crate::view(&model, frame))
        .expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        rendered.contains(last),
        "{last} was clipped from the theme list"
    );
    theme::set_startup_theme("anvil-dark").expect("reset");
}

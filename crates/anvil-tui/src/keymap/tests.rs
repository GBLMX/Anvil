//! Keymap resolution tests.

use super::*;
use crossterm::event::{KeyEvent, KeyModifiers};

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyPress {
    KeyPress::from_key(KeyEvent::new(code, modifiers))
}

#[test]
fn alt_bindings_fold_case() {
    assert_eq!(
        normal_action(key(KeyCode::Char('C'), KeyModifiers::ALT), 0),
        Some(NormalCommand::ExtendLine(true))
    );
    assert_eq!(
        normal_action(key(KeyCode::Char('s'), KeyModifiers::ALT), 0),
        Some(NormalCommand::SplitSelection)
    );
}

#[test]
fn ctrl_slash_toggles_comments() {
    assert_eq!(
        normal_action(key(KeyCode::Char('/'), KeyModifiers::CONTROL), 0),
        Some(NormalCommand::ToggleComment)
    );
}

#[test]
fn ctrl_o_and_ctrl_i_navigate_jump_history() {
    assert_eq!(
        normal_action(key(KeyCode::Char('o'), KeyModifiers::CONTROL), 0),
        Some(NormalCommand::JumpBack)
    );
    assert_eq!(
        normal_action(key(KeyCode::Char('i'), KeyModifiers::CONTROL), 0),
        Some(NormalCommand::JumpForward)
    );
    assert_eq!(
        normal_action(key(KeyCode::Char('o'), KeyModifiers::ALT), 0),
        Some(NormalCommand::JumpBack)
    );
    assert_eq!(
        normal_action(key(KeyCode::Char('i'), KeyModifiers::ALT), 0),
        Some(NormalCommand::JumpForward)
    );
}

#[test]
fn location_bindings_use_enter_and_alt_j_k() {
    assert_eq!(
        normal_action(key(KeyCode::Enter, KeyModifiers::NONE), 0),
        Some(NormalCommand::OpenLocation)
    );
    assert_eq!(
        normal_action(key(KeyCode::Char('j'), KeyModifiers::ALT), 0),
        Some(NormalCommand::LocationNext)
    );
    assert_eq!(
        normal_action(key(KeyCode::Char('k'), KeyModifiers::ALT), 0),
        Some(NormalCommand::LocationPrevious)
    );
}

#[test]
fn zero_moves_home_only_without_a_count() {
    assert_eq!(
        normal_action(key(KeyCode::Char('0'), KeyModifiers::NONE), 0),
        Some(NormalCommand::MoveHome)
    );
    assert_eq!(
        normal_action(key(KeyCode::Char('0'), KeyModifiers::NONE), 2),
        Some(NormalCommand::Count('0'))
    );
}

#[test]
fn percent_matches_brackets() {
    assert_eq!(
        normal_action(key(KeyCode::Char('%'), KeyModifiers::SHIFT), 0),
        Some(NormalCommand::MatchBracket)
    );
}

#[test]
fn shift_g_resolves_to_file_end() {
    assert_eq!(
        normal_action(key(KeyCode::Char('G'), KeyModifiers::SHIFT), 0),
        Some(NormalCommand::MoveFileEnd)
    );
}

#[test]
fn modified_letters_do_not_trigger_plain_bindings() {
    assert_eq!(
        normal_action(key(KeyCode::Char('d'), KeyModifiers::ALT), 0),
        None
    );
}

#[test]
fn pending_operators_resolve_chords() {
    assert_eq!(
        pending_action('d', key(KeyCode::Char('d'), KeyModifiers::NONE), false),
        Some(PendingCommand::DeleteLines)
    );
    assert_eq!(
        pending_action('c', key(KeyCode::Char('i'), KeyModifiers::NONE), false),
        Some(PendingCommand::ObjectPending)
    );
    assert_eq!(
        pending_action('c', key(KeyCode::Char('w'), KeyModifiers::NONE), true),
        Some(PendingCommand::OperatorWord {
            operator: 'c',
            around: false,
        })
    );
}

#[test]
fn pending_g_resolves_navigation_commands() {
    assert_eq!(
        pending_action('g', key(KeyCode::Char('d'), KeyModifiers::NONE), false),
        Some(PendingCommand::Definition)
    );
    assert_eq!(
        pending_action('g', key(KeyCode::Char('r'), KeyModifiers::NONE), false),
        Some(PendingCommand::References)
    );
    assert_eq!(
        pending_action('g', key(KeyCode::Char('h'), KeyModifiers::NONE), false),
        Some(PendingCommand::DocumentHighlight)
    );
    assert_eq!(
        pending_action('g', key(KeyCode::Char('i'), KeyModifiers::NONE), false),
        Some(PendingCommand::IncomingCalls)
    );
    assert_eq!(
        pending_action('g', key(KeyCode::Char('o'), KeyModifiers::NONE), false),
        Some(PendingCommand::OutgoingCalls)
    );
    assert_eq!(
        pending_action('g', key(KeyCode::Char(';'), KeyModifiers::NONE), false),
        Some(PendingCommand::ChangeBack)
    );
    assert_eq!(
        pending_action('g', key(KeyCode::Char(','), KeyModifiers::NONE), false),
        Some(PendingCommand::ChangeForward)
    );
    assert_eq!(
        pending_action('g', key(KeyCode::Char('g'), KeyModifiers::NONE), false),
        Some(PendingCommand::FileStart)
    );
    assert_eq!(
        pending_action('z', key(KeyCode::Char('z'), KeyModifiers::NONE), false),
        Some(PendingCommand::CenterCursor)
    );
    assert_eq!(
        pending_action('z', key(KeyCode::Char('a'), KeyModifiers::NONE), false),
        Some(PendingCommand::ToggleFold)
    );
    assert_eq!(
        pending_action('z', key(KeyCode::Char('M'), KeyModifiers::NONE), false),
        Some(PendingCommand::CloseAllFolds)
    );
    assert_eq!(
        pending_action('z', key(KeyCode::Char('R'), KeyModifiers::NONE), false),
        Some(PendingCommand::OpenAllFolds)
    );
}

#[test]
fn pending_digits_preserve_the_prefix() {
    assert_eq!(
        pending_action('d', key(KeyCode::Char('2'), KeyModifiers::NONE), false),
        Some(PendingCommand::Count {
            pending: 'd',
            digit: '2',
        })
    );
}

#[test]
fn config_overrides_normal_bindings() {
    let chord = KeyChord::from_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
    let mut config = KeymapConfig::default();
    assert!(config.bind(chord, NormalCommand::Undo));
    assert_eq!(
        resolve_normal(key(KeyCode::Char('x'), KeyModifiers::NONE), 0, &config),
        Some(NormalCommand::Undo)
    );
    assert_eq!(
        normal_action(key(KeyCode::Char('x'), KeyModifiers::NONE), 0),
        Some(NormalCommand::DeleteForward)
    );
}

#[test]
fn config_rejects_state_only_commands() {
    let chord = KeyChord::from_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
    let mut config = KeymapConfig::default();
    assert!(!config.bind(chord, NormalCommand::Pending('d')));
    assert!(!config.bind(chord, NormalCommand::Count('1')));
}

#[test]
fn config_overrides_pending_bindings() {
    let chord = KeyChord::from_key(key(KeyCode::Char('d'), KeyModifiers::NONE));
    let mut config = KeymapConfig::default();
    assert!(config.bind_pending('d', chord, PendingCommand::YankLines));
    assert_eq!(
        resolve_pending(
            'd',
            key(KeyCode::Char('d'), KeyModifiers::NONE),
            false,
            &config
        ),
        Some(PendingCommand::YankLines)
    );
    assert_eq!(
        pending_action('d', key(KeyCode::Char('d'), KeyModifiers::NONE), false),
        Some(PendingCommand::DeleteLines)
    );
}

#[test]
fn config_rejects_pending_state_commands() {
    let chord = KeyChord::from_key(key(KeyCode::Char('i'), KeyModifiers::NONE));
    let mut config = KeymapConfig::default();
    assert!(!config.bind_pending('c', chord, PendingCommand::ObjectPending));
}

#[test]
fn macro_key_labels_round_trip_special_keys() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let codes = [
        KeyCode::Char('a'),
        KeyCode::Char(' '),
        KeyCode::Esc,
        KeyCode::Tab,
        KeyCode::BackTab,
        KeyCode::Enter,
        KeyCode::Backspace,
        KeyCode::Delete,
        KeyCode::Insert,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::F(1),
    ];
    for code in codes {
        let key = KeyEvent::new(code, KeyModifiers::NONE);
        let step = macro_step(key);
        let round_trip = macro_key(&step).expect("macro key");
        assert_eq!(round_trip.code, code);
    }
    let shifted = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
    let step = macro_step(shifted);
    assert!(step.shift);
    assert_eq!(
        macro_key(&step).expect("shifted backtab").code,
        KeyCode::BackTab
    );
}

#[test]
fn command_ids_are_registered() {
    assert_eq!(NormalCommand::Append.id(), Some(command::APPEND));
    assert!(is_registered(command::APPEND, CommandContext::Normal));
    assert_eq!(
        PendingCommand::DeleteLines.id(),
        Some(command::DELETE_LINES)
    );
    assert!(is_registered(
        command::DELETE_LINES,
        CommandContext::Pending
    ));
}

#[test]
fn toml_loads_normal_and_pending_overrides() {
    let config = KeymapConfig::from_toml(
        r#"
[keymap.normal]
x = "editor.undo"
alt-c = "selection.extend_up"

[keymap.pending.d]
d = "editor.yank_lines"
"#,
    )
    .expect("valid keymap");
    assert_eq!(
        resolve_normal(key(KeyCode::Char('x'), KeyModifiers::NONE), 0, &config),
        Some(NormalCommand::Undo)
    );
    assert_eq!(
        resolve_normal(key(KeyCode::Char('C'), KeyModifiers::ALT), 0, &config),
        Some(NormalCommand::ExtendLine(true))
    );
    assert_eq!(
        resolve_pending(
            'd',
            key(KeyCode::Char('d'), KeyModifiers::NONE),
            false,
            &config
        ),
        Some(PendingCommand::YankLines)
    );
}

#[test]
fn toml_rejects_unknown_commands_and_bad_keys() {
    assert!(
        KeymapConfig::from_toml(
            r#"
[keymap.normal]
x = "editor.not_a_command"
"#
        )
        .is_err()
    );
    assert!(
        KeymapConfig::from_toml(
            r#"
[keymap.normal]
"ctrl-" = "editor.undo"
"#
        )
        .is_err()
    );
}

#[test]
fn toml_rejects_duplicate_chords() {
    let error = KeymapConfig::from_toml(
        r#"
[keymap.normal]
"ctrl-w" = "editor.undo"
"control-w" = "editor.redo"
"#,
    )
    .expect_err("duplicate chord");
    assert!(error.contains("duplicate"), "{error}");
}

#[test]
fn toml_accepts_function_keys() {
    let config = KeymapConfig::from_toml(
        r#"
[keymap.normal]
F1 = "editor.undo"
f12 = "editor.redo"
"#,
    )
    .expect("function keys are configurable");
    assert_eq!(
        resolve_normal(key(KeyCode::F(1), KeyModifiers::NONE), 0, &config),
        Some(NormalCommand::Undo)
    );
    assert_eq!(
        resolve_normal(key(KeyCode::F(12), KeyModifiers::NONE), 0, &config),
        Some(NormalCommand::Redo)
    );
}

#[test]
fn context_bindings_override_normal_bindings_and_global_falls_back() {
    let config = KeymapConfig::from_toml(
        r#"
[keymap.contexts.global]
"alt-u" = "editor.redo"

[keymap.contexts.editor]
"ctrl-w" = "view.split_vertical"
"alt-u" = "editor.undo"
"#,
    )
    .expect("valid contextual keymap");
    assert_eq!(
        resolve_normal_in_context(
            KeymapContext::Editor,
            key(KeyCode::Char('w'), KeyModifiers::CONTROL),
            0,
            &config
        ),
        Some(ContextAction::Pending(PendingCommand::SplitVertical))
    );
    assert_eq!(
        resolve_normal_in_context(
            KeymapContext::Editor,
            key(KeyCode::Char('u'), KeyModifiers::ALT),
            0,
            &config
        ),
        Some(ContextAction::Normal(NormalCommand::Undo))
    );
    assert_eq!(
        resolve_normal_in_context(
            KeymapContext::FileTree,
            key(KeyCode::Char('u'), KeyModifiers::ALT),
            0,
            &config
        ),
        Some(ContextAction::Normal(NormalCommand::Redo))
    );
}

#[test]
fn context_schema_rejects_unknown_contexts_and_state_commands() {
    let unknown_context = KeymapConfig::from_toml(
        r#"
[keymap.contexts.unknown]
x = "editor.undo"
"#,
    )
    .expect_err("unknown context");
    assert!(
        unknown_context.contains("unknown keymap context"),
        "{unknown_context}"
    );

    let state_command = KeymapConfig::from_toml(
        r#"
[keymap.contexts.editor]
x = "editor.change_inner_word"
"#,
    )
    .expect("direct pending commands are allowed");
    assert_eq!(
        resolve_normal_in_context(
            KeymapContext::Editor,
            key(KeyCode::Char('x'), KeyModifiers::NONE),
            0,
            &state_command
        ),
        Some(ContextAction::Pending(PendingCommand::ChangeInnerWord))
    );

    let invalid = KeymapConfig::from_toml(
        r#"
[keymap.contexts.editor]
x = "not.a.command"
"#,
    )
    .expect_err("unknown command");
    assert!(invalid.contains("unknown or non-direct"), "{invalid}");
}

#[test]
fn bundled_presets_parse_without_function_keys() {
    for name in ["ergo", "vim"] {
        let config = KeymapConfig::from_preset(name).expect("bundled preset parses");
        assert!(
            config
                .normal
                .keys()
                .all(|chord| !matches!(chord.code, KeyCode::F(_))),
            "{name} normal bindings contain an F-key"
        );
        assert!(
            config
                .pending
                .keys()
                .all(|(_, chord)| !matches!(chord.code, KeyCode::F(_))),
            "{name} pending bindings contain an F-key"
        );
        assert!(
            config
                .contexts
                .values()
                .flatten()
                .all(|(chord, _)| !matches!(chord.code, KeyCode::F(_))),
            "{name} context bindings contain an F-key"
        );
    }
}

#[test]
fn config_can_select_a_preset_as_its_base() {
    let config = KeymapConfig::from_toml(
        r#"
preset = "vim"

[keymap.normal]
x = "editor.undo"
"#,
    )
    .expect("preset base with override");
    assert_eq!(
        resolve_normal(key(KeyCode::Char('x'), KeyModifiers::NONE), 0, &config),
        Some(NormalCommand::Undo)
    );
    assert_eq!(
        resolve_normal(key(KeyCode::Char('j'), KeyModifiers::NONE), 0, &config),
        Some(NormalCommand::MoveLine(1))
    );
    assert!(
        KeymapConfig::from_toml(r#"preset = "missing""#).is_err(),
        "unknown presets must be rejected"
    );
}

#[test]
fn pending_actions_include_text_object_extenders() {
    let config = KeymapConfig::default();
    let actions = config.pending_actions('d', false);
    assert!(
        actions
            .iter()
            .any(|(key, action)| key == "i" && action == "text object"),
        "{actions:?}"
    );
    assert!(
        actions
            .iter()
            .any(|(key, action)| key == "a" && action == "text object"),
        "{actions:?}"
    );
    let inner = config.pending_actions('d', true);
    assert!(
        inner
            .iter()
            .any(|(key, action)| key == "w" && action == "delete inner word"),
        "{inner:?}"
    );
    let around = config.pending_actions('D', true);
    assert!(
        around
            .iter()
            .any(|(key, action)| key == "w" && action == "delete around word"),
        "{around:?}"
    );
}

#[test]
fn ordered_context_resolution_prefers_the_most_specific_context() {
    let config = KeymapConfig::from_toml(
        r#"
[keymap.contexts.global]
x = "view.focus_right"
[keymap.contexts.popup]
x = "view.focus_editor"
"#,
    )
    .expect("context keymap");
    let resolved = resolve_context_binding(
        &[KeymapContext::Global, KeymapContext::Popup],
        key(KeyCode::Char('x'), KeyModifiers::NONE),
        &config,
    )
    .expect("binding");
    assert_eq!(resolved.0, KeymapContext::Popup);
    assert!(matches!(
        resolved.1,
        ContextAction::Pending(PendingCommand::FocusEditor)
    ));
}

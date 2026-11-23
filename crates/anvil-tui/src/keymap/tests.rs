//! Keymap resolution tests.

use super::*;
use crate::EditorState;
use crossterm::event::{KeyEvent, KeyModifiers};
use std::collections::HashSet;

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyPress {
    KeyPress::from_key(KeyEvent::new(code, modifiers))
}

#[test]
fn canonical_default_bindings_are_unique() {
    let bindings = [
        (
            key(KeyCode::Esc, KeyModifiers::NONE),
            "editor.collapse_selections",
        ),
        (key(KeyCode::Char('h'), KeyModifiers::NONE), "cursor.left"),
        (key(KeyCode::Char('j'), KeyModifiers::NONE), "cursor.down"),
        (key(KeyCode::Char('k'), KeyModifiers::NONE), "cursor.up"),
        (key(KeyCode::Char('l'), KeyModifiers::NONE), "cursor.right"),
        (
            key(KeyCode::Char('w'), KeyModifiers::NONE),
            "cursor.word_next",
        ),
        (
            key(KeyCode::Char('b'), KeyModifiers::NONE),
            "cursor.word_previous",
        ),
        (
            key(KeyCode::Char('e'), KeyModifiers::NONE),
            "cursor.word_end",
        ),
        (
            key(KeyCode::Char('0'), KeyModifiers::NONE),
            "cursor.line_start",
        ),
        (
            key(KeyCode::Char('$'), KeyModifiers::NONE),
            "cursor.line_end",
        ),
        (
            key(KeyCode::Char('G'), KeyModifiers::NONE),
            "cursor.file_end",
        ),
        (
            key(KeyCode::Char('S'), KeyModifiers::NONE),
            "selection.split_lines",
        ),
        (
            key(KeyCode::Char('s'), KeyModifiers::NONE),
            "selection.word_occurrences",
        ),
        (
            key(KeyCode::Char('C'), KeyModifiers::NONE),
            "selection.extend_down",
        ),
        (
            key(KeyCode::Char('C'), KeyModifiers::ALT),
            "selection.extend_up",
        ),
        (
            key(KeyCode::Char('i'), KeyModifiers::NONE),
            "editor.insert_here",
        ),
        (key(KeyCode::Char('a'), KeyModifiers::NONE), "editor.append"),
        (
            key(KeyCode::Char('A'), KeyModifiers::NONE),
            "editor.append_line_end",
        ),
        (
            key(KeyCode::Char('o'), KeyModifiers::NONE),
            "editor.open_below",
        ),
        (
            key(KeyCode::Char('O'), KeyModifiers::NONE),
            "editor.open_above",
        ),
        (
            key(KeyCode::Char('x'), KeyModifiers::NONE),
            "editor.delete_forward",
        ),
        (key(KeyCode::Char('p'), KeyModifiers::NONE), "editor.put"),
        (key(KeyCode::Char('u'), KeyModifiers::NONE), "editor.undo"),
        (
            key(KeyCode::Char('r'), KeyModifiers::CONTROL),
            "editor.redo",
        ),
        (
            key(KeyCode::Char('.'), KeyModifiers::NONE),
            "editor.repeat_change",
        ),
        (
            key(KeyCode::Char('J'), KeyModifiers::NONE),
            "editor.join_lines",
        ),
        (
            key(KeyCode::Char('>'), KeyModifiers::NONE),
            "editor.indent_lines",
        ),
        (
            key(KeyCode::Char('<'), KeyModifiers::NONE),
            "editor.outdent_lines",
        ),
        (
            key(KeyCode::Char('%'), KeyModifiers::NONE),
            "navigation.match_bracket",
        ),
        (key(KeyCode::Char('q'), KeyModifiers::NONE), "macro.record"),
        (key(KeyCode::Char('@'), KeyModifiers::NONE), "macro.replay"),
        (key(KeyCode::Char('K'), KeyModifiers::NONE), "lsp.hover"),
        (
            key(KeyCode::Char('/'), KeyModifiers::CONTROL),
            "editor.toggle_comment",
        ),
        (
            key(KeyCode::Char('o'), KeyModifiers::ALT),
            "navigation.jump_back",
        ),
        (
            key(KeyCode::Char('i'), KeyModifiers::ALT),
            "navigation.jump_forward",
        ),
        (
            key(KeyCode::Char(':'), KeyModifiers::NONE),
            "command_line.open",
        ),
        (key(KeyCode::Char('/'), KeyModifiers::NONE), "search.open"),
        (key(KeyCode::Char('n'), KeyModifiers::NONE), "search.next"),
        (
            key(KeyCode::Char('N'), KeyModifiers::NONE),
            "search.previous",
        ),
    ];

    let mut seen = HashSet::new();
    for (binding, expected) in bindings {
        let action = normal_action(binding, 0).expect("canonical binding resolves");
        let id = action
            .id()
            .expect("canonical binding has a command id")
            .as_str();
        assert_eq!(id, expected);
        assert!(seen.insert(id), "duplicate canonical binding for {id}");
    }
}

#[test]
fn canonical_hints_match_the_rebound_keys() {
    assert_eq!(default_key_hint("selection.split_lines"), Some("S"));
    assert_eq!(default_key_hint("selection.extend_up"), Some("Alt-C"));
    assert_eq!(default_key_hint("selection.extend_down"), Some("C"));
    assert_eq!(default_key_hint("editor.toggle_comment"), Some("Ctrl-/"));
    assert_eq!(default_key_hint("cursor.line_start"), Some("0"));
    assert_eq!(default_key_hint("cursor.line_end"), Some("$"));
    assert_eq!(default_key_hint("editor.join_lines"), Some("J"));
    assert_eq!(default_key_hint("editor.indent_lines"), Some(">"));
    assert_eq!(default_key_hint("editor.outdent_lines"), Some("<"));
}

#[test]
fn cursor_line_and_split_bindings_are_canonical() {
    assert_eq!(
        normal_action(key(KeyCode::Char('C'), KeyModifiers::ALT), 0),
        Some(NormalCommand::AddCursorLine(true))
    );
    assert_eq!(
        normal_action(key(KeyCode::Char('S'), KeyModifiers::NONE), 0),
        Some(NormalCommand::SplitSelection)
    );
    assert_eq!(
        normal_action(key(KeyCode::Char('s'), KeyModifiers::ALT), 0),
        None
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
fn alt_jump_bindings_are_canonical() {
    assert_eq!(
        normal_action(key(KeyCode::Char('o'), KeyModifiers::CONTROL), 0),
        None
    );
    assert_eq!(
        normal_action(key(KeyCode::Char('i'), KeyModifiers::CONTROL), 0),
        None
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
fn numbered_space_bindings_are_removed() {
    for character in ['1', '2', '3'] {
        assert_eq!(
            pending_action(
                ' ',
                key(KeyCode::Char(character), KeyModifiers::NONE),
                false,
            ),
            None
        );
    }
    let config = KeymapConfig::from_preset("ergo").expect("ergo preset");
    for character in ['1', '2', '3'] {
        assert_eq!(
            resolve_pending(
                ' ',
                key(KeyCode::Char(character), KeyModifiers::NONE),
                false,
                &config,
            ),
            None
        );
    }
}

#[test]
fn pending_g_resolves_navigation_commands() {
    assert_eq!(
        pending_action('g', key(KeyCode::Char('d'), KeyModifiers::NONE), false),
        Some(PendingCommand::Definition)
    );
    assert_eq!(
        pending_action('g', key(KeyCode::Char('D'), KeyModifiers::NONE), false),
        Some(PendingCommand::Declaration)
    );
    assert_eq!(
        pending_action('g', key(KeyCode::Char('y'), KeyModifiers::NONE), false),
        Some(PendingCommand::TypeDefinition)
    );
    assert_eq!(
        pending_action('g', key(KeyCode::Char('I'), KeyModifiers::NONE), false),
        Some(PendingCommand::Implementation)
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
        pending_action('[', key(KeyCode::Char('['), KeyModifiers::NONE), false),
        Some(PendingCommand::SectionPrevious)
    );
    assert_eq!(
        pending_action(']', key(KeyCode::Char(']'), KeyModifiers::NONE), false),
        Some(PendingCommand::SectionNext)
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
    for (command, id) in [
        (NormalCommand::JoinLines, command::JOIN_LINES),
        (NormalCommand::IndentLines, command::INDENT_LINES),
        (NormalCommand::OutdentLines, command::OUTDENT_LINES),
    ] {
        assert_eq!(command.id(), Some(id));
        assert!(is_registered(id, CommandContext::Normal));
    }
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
        Some(NormalCommand::AddCursorLine(true))
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
fn window_prefix_opens_both_split_orientations() {
    let config = KeymapConfig::default();
    assert_eq!(
        resolve_pending(
            'w',
            key(KeyCode::Char('v'), KeyModifiers::NONE),
            false,
            &config,
        ),
        Some(PendingCommand::SplitVertical)
    );
    assert_eq!(
        resolve_pending(
            'w',
            key(KeyCode::Char('s'), KeyModifiers::NONE),
            false,
            &config,
        ),
        Some(PendingCommand::SplitHorizontal)
    );
    assert_eq!(
        resolve_pending(
            'w',
            key(KeyCode::Char('j'), KeyModifiers::NONE),
            false,
            &config,
        ),
        Some(PendingCommand::FocusSplit(true))
    );
    assert_eq!(
        resolve_pending(
            'w',
            key(KeyCode::Char('k'), KeyModifiers::NONE),
            false,
            &config,
        ),
        Some(PendingCommand::FocusSplit(false))
    );
}
#[test]
fn helix_preset_is_not_advertised() {
    assert!(KeymapConfig::from_preset("helix").is_err());
    assert!(KeymapConfig::from_preset("anvil-helix").is_err());

    let config = KeymapConfig::default();
    for character in ['h', 'k', 'c', '?'] {
        assert_eq!(
            resolve_pending(
                ' ',
                key(KeyCode::Char(character), KeyModifiers::NONE),
                false,
                &config,
            ),
            None,
            "Space {character} must not be a hidden duplicate"
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

#[test]
fn space_w_opens_workspace_start_page() {
    assert_eq!(
        pending_action(' ', key(KeyCode::Char('w'), KeyModifiers::NONE), false),
        Some(PendingCommand::Workspaces)
    );
}

#[test]
fn space_p_opens_project_symbols() {
    assert_eq!(
        pending_action(' ', key(KeyCode::Char('P'), KeyModifiers::NONE), false),
        Some(PendingCommand::WorkspaceSymbols)
    );
}

#[test]
fn keymap_list_includes_pending_and_context_bindings() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model
        .config
        .keymap
        .contexts
        .entry(KeymapContext::FileTree)
        .or_default()
        .insert(
            KeyChord::from_key(key(KeyCode::Char('x'), KeyModifiers::NONE)),
            ContextAction::Normal(NormalCommand::Hover),
        );

    let text = model.keymap_list_text();
    assert!(text.contains("Space d"), "{text}");
    assert!(text.contains("Built-in keys by overlay"), "{text}");
    assert!(text.contains("settings"), "{text}");
    assert!(text.contains("completion"), "{text}");
    assert!(text.contains("lsp_list"), "{text}");
    assert!(text.contains("debug_panel"), "{text}");
    assert!(text.contains("insert"), "{text}");
    assert!(text.contains("confirm_close"), "{text}");
    assert!(text.contains("command_line"), "{text}");
    assert!(text.contains("close tree"), "{text}");
    assert!(text.contains("Configured context overrides"), "{text}");
    assert!(text.contains("file_tree"), "{text}");
    assert!(text.contains("lsp.hover"), "{text}");
}

#[test]
fn builtin_context_catalog_uses_known_contexts() {
    let mut seen = HashSet::new();
    for (overlay, route, key, action) in BUILTIN_CONTEXT_KEYS {
        assert!(!overlay.is_empty());
        assert!(KeymapContext::route_order().contains(route));
        assert!(!key.is_empty());
        assert!(!action.is_empty());
        assert!(
            seen.insert((*overlay, *key)),
            "duplicate catalog key {overlay} {key}"
        );
    }
}

#[test]
fn keymap_list_prefers_configured_bindings_over_default_hints() {
    let mut model = Model::new(EditorState::from_text(""), None);
    assert!(model.config.keymap.bind(
        KeyChord::from_key(key(KeyCode::Char('x'), KeyModifiers::NONE)),
        NormalCommand::Hover,
    ));

    let text = model.keymap_list_text();
    let x_rows = text
        .lines()
        .filter(|line| line.trim_start().starts_with("x "))
        .collect::<Vec<_>>();
    assert_eq!(x_rows.len(), 1, "{text}");
    assert!(x_rows[0].contains("lsp.hover"), "{text}");
    assert!(!x_rows[0].contains("editor.delete_forward"), "{text}");
}

#[test]
fn alt_shift_letter_preserves_explicit_shift() {
    let chord = KeyChord::from_key(key(
        KeyCode::Char('A'),
        KeyModifiers::ALT | KeyModifiers::SHIFT,
    ));
    assert_eq!(chord, KeyChord::parse("alt-shift-a").expect("alt-shift"));
    assert_eq!(chord.code, KeyCode::Char('A'));
}

#[test]
fn legacy_alt_uppercase_still_folds_without_shift() {
    let chord = KeyChord::from_key(key(KeyCode::Char('C'), KeyModifiers::ALT));
    assert_eq!(chord.code, KeyCode::Char('c'));
}

#[test]
fn alt_shift_macro_round_trip_preserves_case() {
    let original = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::ALT | KeyModifiers::SHIFT);
    let restored = macro_key(&macro_step(original)).expect("macro key");
    assert_eq!(restored.code, KeyCode::Char('A'));
    assert_eq!(restored.modifiers, KeyModifiers::ALT | KeyModifiers::SHIFT);
}

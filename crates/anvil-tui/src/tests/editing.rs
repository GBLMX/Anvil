use super::*;
#[test]
fn idle_events_do_not_request_a_redraw() {
    let mut model = Model::new(EditorState::from_text("a\nb"), None);
    model.dirty = false;
    handle_event(&mut model, Event::FocusGained);
    assert!(!model.dirty);
    handle_event(&mut model, key(KeyCode::Char('z')));
    assert!(!model.dirty);
    handle_event(&mut model, key(KeyCode::Char('j')));
    assert!(model.dirty);
}

#[test]
fn invalid_utf8_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("binary.bin");
    fs::write(&path, [0xff, 0xfe]).expect("write");
    assert!(Model::open(path).is_err());
}

#[test]
fn paste_only_inserts_in_edit_modes() {
    let mut model = Model::new(EditorState::from_text("a"), None);
    model.dirty = false;
    handle_event(&mut model, Event::Paste("x".to_owned()));
    assert_eq!(model.editor.text().slice(0..1), "a");
    assert!(!model.dirty);

    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, Event::Paste("b".to_owned()));
    assert_eq!(model.editor.text().slice(0..2), "ba");
}

#[test]
fn quit_guard_requires_force_or_write() {
    let mut model = Model::new(EditorState::from_text("a"), None);
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, key(KeyCode::Char('b')));
    handle_event(&mut model, key(KeyCode::Esc));

    for code in [KeyCode::Char(':'), KeyCode::Char('q'), KeyCode::Enter] {
        handle_event(&mut model, key(code));
    }
    assert!(!model.quit);
    assert!(model.status.contains("No write since last change"));

    handle_event(&mut model, key(KeyCode::Char(':')));
    handle_event(&mut model, key(KeyCode::Char('q')));
    handle_event(&mut model, key(KeyCode::Char('!')));
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(model.quit);
}

#[test]
fn undo_and_redo_are_reachable_from_normal_mode() {
    let mut model = Model::new(EditorState::from_text("a"), None);
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, key(KeyCode::Char('X')));
    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.editor.text().slice(0..2), "Xa");

    handle_event(&mut model, key(KeyCode::Char('u')));
    assert_eq!(model.editor.text().slice(0..1), "a");
    assert_eq!(model.cursor, 0);

    handle_event(&mut model, ctrl(KeyCode::Char('r')));
    assert_eq!(model.editor.text().slice(0..2), "Xa");
    assert_eq!(model.cursor, 1);
}

#[test]
fn delete_into_named_register() {
    let mut model = Model::new(EditorState::from_text("alpha beta"), None);
    for key_code in [
        KeyCode::Char('"'),
        KeyCode::Char('a'),
        KeyCode::Char('d'),
        KeyCode::Char('w'),
    ] {
        handle_event(&mut model, key(key_code));
    }
    assert_eq!(
        model.registers.get(&'a').map(String::as_str),
        Some("alpha ")
    );
    for key_code in [KeyCode::Char('"'), KeyCode::Char('a'), KeyCode::Char('p')] {
        handle_event(&mut model, key(key_code));
    }
    assert_eq!(model.editor.text().slice(0..10), "alpha beta");
}

#[test]
fn named_register_yank_and_put() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta"), None);
    for key_code in [
        KeyCode::Char('"'),
        KeyCode::Char('a'),
        KeyCode::Char('y'),
        KeyCode::Char('y'),
    ] {
        handle_event(&mut model, key(key_code));
    }
    assert_eq!(
        model.registers.get(&'a').map(String::as_str),
        Some("alpha\n")
    );
    model.cursor = 0;
    for key_code in [KeyCode::Char('"'), KeyCode::Char('a'), KeyCode::Char('p')] {
        handle_event(&mut model, key(key_code));
    }
    assert_eq!(model.editor.text().slice(0..12), "alpha\nalpha\n");
}

#[test]
fn macro_records_and_replays() {
    let mut model = Model::new(EditorState::from_text(""), None);
    for code in [
        KeyCode::Char('q'),
        KeyCode::Char('a'),
        KeyCode::Char('i'),
        KeyCode::Char('X'),
        KeyCode::Esc,
        KeyCode::Char('q'),
    ] {
        handle_event(&mut model, key(code));
    }
    assert_eq!(model.macros.get(&'a').map(Vec::len), Some(3));
    for code in [KeyCode::Char('@'), KeyCode::Char('a')] {
        handle_event(&mut model, key(code));
    }
    assert_eq!(model.editor.text().slice(0..2), "XX");
}

#[test]
fn change_inner_word_and_quote() {
    let mut model = Model::new(EditorState::from_text("alpha beta"), None);
    for key_code in [KeyCode::Char('c'), KeyCode::Char('i'), KeyCode::Char('w')] {
        handle_event(&mut model, key(key_code));
    }
    handle_event(&mut model, key(KeyCode::Char('X')));
    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.editor.text().slice(0..6), "X beta");

    let mut model = Model::new(EditorState::from_text("say \"hello\" now"), None);
    model.cursor = 6;
    for key_code in [KeyCode::Char('c'), KeyCode::Char('i'), KeyCode::Char('"')] {
        handle_event(&mut model, key(key_code));
    }
    handle_event(&mut model, key(KeyCode::Char('X')));
    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.editor.text().slice(0..11), "say \"X\" now");
}

#[test]
fn pending_commands_consume_the_count() {
    let mut model = Model::new(EditorState::from_text("one two"), None);
    for key_code in [KeyCode::Char('2'), KeyCode::Char('c'), KeyCode::Char('w')] {
        handle_event(&mut model, key(key_code));
    }
    assert_eq!(model.count, 0);

    let mut model = Model::new(EditorState::from_text("one"), None);
    handle_event(&mut model, key(KeyCode::Char('2')));
    handle_event(&mut model, ctrl(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Char('v')));
    assert_eq!(model.count, 0);
    assert!(model.split.is_some());

    let mut model = Model::new(EditorState::from_text("one"), None);
    handle_event(&mut model, key(KeyCode::Char('2')));
    handle_event(&mut model, key(KeyCode::Char('d')));
    handle_event(&mut model, key(KeyCode::Char('x')));
    assert_eq!(model.count, 0);
}

#[test]
fn model_uses_keymap_overrides() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    let chord = KeyChord::from_key(KeyPress::from_key(KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::NONE,
    )));
    assert!(model.keymap.bind(chord, NormalCommand::MoveEnd));
    handle_event(&mut model, key(KeyCode::Char('z')));
    assert_eq!(model.cursor, 3);
}

#[test]
fn context_keymap_runs_direct_pending_action() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    model.welcome = false;
    model.keymap = KeymapConfig::from_toml(
        r#"
[keymap.contexts.editor]
"ctrl-w" = "view.split_vertical"
"#,
    )
    .expect("valid contextual keymap");
    handle_event(&mut model, ctrl(KeyCode::Char('w')));
    assert!(model.split.is_some());
}

#[test]
fn model_uses_pending_keymap_overrides() {
    let mut model = Model::new(EditorState::from_text("one"), None);
    let chord = KeyChord::from_key(KeyPress::from_key(KeyEvent::new(
        KeyCode::Char('d'),
        KeyModifiers::NONE,
    )));
    assert!(
        model
            .keymap
            .bind_pending('d', chord, PendingCommand::YankLines)
    );
    handle_event(&mut model, key(KeyCode::Char('d')));
    handle_event(&mut model, key(KeyCode::Char('d')));
    assert_eq!(model.editor.text().slice(0..3), "one");
}

#[test]
fn model_loads_keymap_config_from_file() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("keymap.toml");
    std::fs::write(
        &path,
        r#"
[keymap.normal]
x = "cursor.line_end"
"#,
    )
    .expect("write keymap");
    let mut model = Model::new(EditorState::from_text("abc"), None);
    model.load_keymap_config_from(&path).expect("load keymap");
    handle_event(&mut model, key(KeyCode::Char('x')));
    assert_eq!(model.cursor, 3);
}

#[test]
fn counts_repeat_motions_and_operators() {
    let mut model = Model::new(EditorState::from_text("a b c d"), None);
    handle_event(&mut model, key(KeyCode::Char('3')));
    handle_event(&mut model, key(KeyCode::Char('w')));
    assert_eq!(model.cursor, 6);

    let mut model = Model::new(EditorState::from_text("a b c d"), None);
    handle_event(&mut model, key(KeyCode::Char('d')));
    handle_event(&mut model, key(KeyCode::Char('2')));
    handle_event(&mut model, key(KeyCode::Char('w')));
    assert_eq!(model.editor.text().slice(0..3), "c d");
}

#[test]
fn zero_moves_to_the_line_start_without_a_count() {
    let mut model = Model::new(EditorState::from_text("abcdef"), None);
    model.cursor = 4;
    handle_event(&mut model, key(KeyCode::Char('0')));
    assert_eq!(model.cursor, 0);
}

#[test]
fn word_motions_follow_vi_boundaries() {
    let mut model = Model::new(EditorState::from_text("alpha beta.gamma"), None);
    handle_event(&mut model, key(KeyCode::Char('w')));
    assert_eq!(model.cursor, 6);
    handle_event(&mut model, key(KeyCode::Char('w')));
    assert_eq!(model.cursor, 10);
    handle_event(&mut model, key(KeyCode::Char('b')));
    assert_eq!(model.cursor, 6);
    handle_event(&mut model, key(KeyCode::Char('e')));
    assert_eq!(model.cursor, 9);
}

#[test]
fn substitute_is_literal_and_line_scoped() {
    let mut model = Model::new(EditorState::from_text("foo foo\nfoo"), None);
    model.execute_command("s/foo/bar/");
    assert_eq!(model.editor.text().slice(0..11), "bar foo\nfoo");
    assert!(model.modified);

    let mut model = Model::new(EditorState::from_text("foo foo\nfoo"), None);
    model.execute_command("%s/foo/bar/g");
    assert_eq!(model.editor.text().slice(0..11), "bar bar\nbar");
}

#[test]
fn substitute_uses_non_overlapping_matches() {
    let mut model = Model::new(EditorState::from_text("aaa"), None);
    model.execute_command("%s/aa/X/g");
    assert_eq!(model.editor.text().slice(0..2), "Xa");
}

#[test]
fn substitute_updates_offsets_after_length_change() {
    let mut model = Model::new(EditorState::from_text("a a"), None);
    model.execute_command("%s/a/aa/g");
    assert_eq!(model.editor.text().slice(0..5), "aa aa");
}

#[test]
fn word_end_keeps_multibyte_boundary() {
    let mut model = Model::new(EditorState::from_text("café"), None);
    handle_event(&mut model, key(KeyCode::Char('e')));
    assert_eq!(model.cursor, 3);
    assert!(model.editor.text().is_char_boundary(model.cursor));
}

#[test]
fn dw_deletes_to_next_word_start_and_undoes() {
    let mut model = Model::new(EditorState::from_text("alpha beta"), None);
    handle_event(&mut model, key(KeyCode::Char('d')));
    handle_event(&mut model, key(KeyCode::Char('w')));
    assert_eq!(model.editor.text().slice(0..4), "beta");
    assert_eq!(model.cursor, 0);
    handle_event(&mut model, key(KeyCode::Char('u')));
    assert_eq!(model.editor.text().slice(0..10), "alpha beta");
}

#[test]
fn dd_deletes_current_line_and_keeps_line_start() {
    let mut model = Model::new(EditorState::from_text("a\nb\nc"), None);
    model.move_line(1);
    handle_event(&mut model, key(KeyCode::Char('d')));
    handle_event(&mut model, key(KeyCode::Char('d')));
    assert_eq!(model.editor.text().slice(0..3), "a\nc");
    assert_eq!(model.cursor, 2);
    assert!(model.modified);
}

#[test]
fn yy_and_p_use_the_unnamed_register() {
    let mut model = Model::new(EditorState::from_text("a\nb"), None);
    model.move_line(1);
    handle_event(&mut model, key(KeyCode::Char('y')));
    handle_event(&mut model, key(KeyCode::Char('y')));
    assert_eq!(model.editor.yank_text(), "b");
    assert_eq!(model.cursor, 2);

    model.move_end();
    handle_event(&mut model, key(KeyCode::Char('p')));
    assert_eq!(model.editor.text().slice(0..4), "a\nbb");
    assert_eq!(model.cursor, 4);
}

#[test]
fn x_deletes_forward() {
    let mut model = Model::new(EditorState::from_text("ab"), None);
    handle_event(&mut model, key(KeyCode::Char('x')));
    assert_eq!(model.editor.text().slice(0..1), "b");
    assert_eq!(model.cursor, 0);
    assert!(model.modified);
}

#[test]
fn wq_without_a_path_does_not_quit() {
    let mut model = Model::new(EditorState::from_text("a"), None);
    for code in [
        KeyCode::Char(':'),
        KeyCode::Char('w'),
        KeyCode::Char('q'),
        KeyCode::Enter,
    ] {
        handle_event(&mut model, key(code));
    }
    assert!(!model.quit);
    assert_eq!(model.status, "E: no file name");
}

#[test]
fn search_cycles_and_crosses_chunk_boundaries() {
    let mut model = Model::new(EditorState::from_text("foo bar foo"), None);
    handle_event(&mut model, key(KeyCode::Char('/')));
    type_text(&mut model, "foo");
    handle_event(&mut model, key(KeyCode::Enter));
    assert_eq!(model.cursor, 0);

    handle_event(&mut model, key(KeyCode::Char('n')));
    assert_eq!(model.cursor, 8);
    handle_event(&mut model, key(KeyCode::Char('N')));
    assert_eq!(model.cursor, 0);

    let mut chunked = Model::new(EditorState::from_text("hello "), None);
    chunked.cursor = 6;
    chunked.insert_text("world");
    handle_event(&mut chunked, key(KeyCode::Char('/')));
    type_text(&mut chunked, "o w");
    handle_event(&mut chunked, key(KeyCode::Enter));
    assert_eq!(chunked.cursor, 4);
}

#[test]
fn incremental_search_debounces_and_esc_restores_cursor() {
    let mut model = Model::new(EditorState::from_text("foo foo"), None);
    model.cursor = 5;
    handle_event(&mut model, key(KeyCode::Char('/')));
    type_text(&mut model, "foo");
    assert!(model.search.matches.is_empty());
    model.search.due = Some(std::time::Instant::now());
    model.tick_search();
    assert_eq!(model.search.matches.len(), 2);
    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.cursor, 5);
    assert!(model.search.matches.is_empty());
    assert!(matches!(model.ui.input_mode, InputMode::Normal));
}

#[test]
fn invalid_regex_keeps_previous_matches() {
    let mut model = Model::new(EditorState::from_text("foo foo"), None);
    model.commit_search("foo".to_owned());
    assert_eq!(model.search.matches.len(), 2);
    model.search.options.regex = true;
    model.commit_search("[a-".to_owned());
    assert!(model.search.error.is_some());
    assert_eq!(model.search.matches.len(), 2);
}

#[test]
fn replace_all_is_one_undo_and_handles_unicode() {
    let mut model = Model::new(EditorState::from_text(&"x ".repeat(100)), None);
    model.commit_search("x".to_owned());
    assert_eq!(model.search.matches.len(), 100);
    assert!(model.replace_all("y"));
    assert_eq!(model.editor.text().slice(0..3), "y y");
    model.undo();
    assert_eq!(model.editor.text().slice(0..3), "x x");

    let mut unicode = Model::new(EditorState::from_text("你好 你好"), None);
    unicode.commit_search("你好".to_owned());
    assert_eq!(unicode.search.matches.len(), 2);
    assert!(unicode.replace_all("hi"));
    assert_eq!(unicode.editor.text().slice(0..5), "hi hi");
}

#[test]
fn replacement_rejects_read_only_help() {
    let mut model = Model::new(EditorState::from_text(""), None);
    command(&mut model, "help");
    model.commit_search("Anvil".to_owned());
    let before = model.editor.text().slice(0..5);
    assert!(!model.replace_all("X"));
    assert_eq!(model.editor.text().slice(0..5), before);
}

#[test]
#[cfg(unix)]
fn open_resolves_symlink_to_canonical_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    fs::write(&target, "first\nsecond").expect("write");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    let mut model = Model::open(&link).expect("open");
    assert_eq!(model.path.as_deref(), Some(target.as_path()));
    model.diagnostics.insert(
        model.path.clone().expect("path"),
        vec![lsp::Diagnostic {
            range: lsp::Range {
                start: lsp::Position {
                    line: 1,
                    character: 0,
                },
                end: lsp::Position {
                    line: 1,
                    character: 3,
                },
            },
            severity: Some(1),
            message: "error".to_owned(),
            source: Some("test".to_owned()),
            extra: serde_json::Map::new(),
        }],
    );
    assert_eq!(model.diagnostic_lines(), vec![1]);
}

#[test]
fn w_path_saves_as_and_updates_the_current_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("saved.txt");
    let mut model = Model::new(EditorState::from_text("data"), None);
    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, &format!("w {}", path.display()));
    handle_event(&mut model, key(KeyCode::Enter));

    assert_eq!(fs::read_to_string(&path).expect("read"), "data");
    assert_eq!(model.path.as_ref(), Some(&path));
    assert!(!model.modified);
}

#[test]
fn insert_variants_append_and_open_lines() {
    let mut append = Model::new(EditorState::from_text("ac"), None);
    handle_event(&mut append, key(KeyCode::Char('a')));
    type_text(&mut append, "b");
    assert_eq!(append.editor.text().slice(0..3), "abc");

    let mut below = Model::new(EditorState::from_text("a"), None);
    handle_event(&mut below, key(KeyCode::Char('o')));
    type_text(&mut below, "b");
    assert_eq!(below.editor.text().slice(0..3), "a\nb");

    let mut above = Model::new(EditorState::from_text("a"), None);
    handle_event(&mut above, key(KeyCode::Char('O')));
    type_text(&mut above, "b");
    assert_eq!(above.editor.text().slice(0..3), "b\na");

    let mut end = Model::new(EditorState::from_text("a"), None);
    handle_event(&mut end, key(KeyCode::Char('A')));
    type_text(&mut end, "b");
    assert_eq!(end.editor.text().slice(0..2), "ab");
}

#[test]
fn grapheme_motion_and_deletion_keep_emoji_whole() {
    let mut model = Model::new(EditorState::from_text("a👨‍👩‍👧‍👦b"), None);
    model.cursor = 1;
    model.move_char(1);
    assert_eq!(model.cursor, "a👨‍👩‍👧‍👦".len());
    model.move_char(-1);
    assert_eq!(model.cursor, 1);
    model.delete_forward();
    assert_eq!(model.editor.text().slice(0..2), "ab");

    let mut backspace = Model::new(EditorState::from_text("a👨‍👩‍👧‍👦b"), None);
    backspace.cursor = "a👨‍👩‍👧‍👦".len();
    backspace.backspace();
    assert_eq!(backspace.editor.text().slice(0..2), "ab");
}

#[test]
fn help_is_read_only_and_returns_to_the_previous_buffer() {
    let original_path = PathBuf::from("file.txt");
    let mut model = Model::new(
        EditorState::from_text("original"),
        Some(original_path.clone()),
    );
    model.cursor = 3;
    model.scroll = 1;
    model.hscroll = 2;
    model.modified = true;

    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, "help");
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(model.help_back.is_some());
    assert_eq!(model.editor.text().slice(0..HELP_TEXT.len()), HELP_TEXT);
    assert_eq!(model.ui.input_mode, InputMode::Normal);

    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "Z");
    assert_eq!(model.ui.input_mode, InputMode::Normal);
    assert!(model.status.contains("read-only"));
    assert_eq!(model.editor.text().slice(0..HELP_TEXT.len()), HELP_TEXT);

    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, "q");
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(model.help_back.is_none());
    assert_eq!(model.editor.text().slice(0..8), "original");
    assert_eq!(model.path, Some(original_path));
    assert_eq!(model.cursor, 3);
    assert_eq!(model.scroll, 1);
    assert_eq!(model.hscroll, 2);
    assert!(model.modified);
}

#[test]
fn help_refuses_writes() {
    let mut model = Model::new(EditorState::from_text("data"), None);
    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, "help");
    handle_event(&mut model, key(KeyCode::Enter));

    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, "w");
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(model.help_back.is_some());
    assert!(model.status.contains("read-only"));
    assert_eq!(model.editor.text().slice(0..HELP_TEXT.len()), HELP_TEXT);
}

#[test]
fn visual_operations_refuse_writes_to_help_buffer() {
    let mut model = Model::new(EditorState::from_text("data"), None);
    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, "help");
    handle_event(&mut model, key(KeyCode::Enter));

    let cases: [&[KeyCode]; 3] = [
        &[
            KeyCode::Char('v'),
            KeyCode::Right,
            KeyCode::Char('s'),
            KeyCode::Char('('),
        ],
        &[KeyCode::Char('v'), KeyCode::Right, KeyCode::Char('d')],
        &[KeyCode::Char('v'), KeyCode::Right, KeyCode::Char('c')],
    ];
    for keys in cases {
        for &key_code in keys {
            handle_event(&mut model, key(key_code));
        }
        assert!(model.status.contains("read-only"));
        assert!(model.modal_layer.is_none());
        assert!(model.visual_anchor.is_none());
        assert_eq!(model.editor.text().slice(0..HELP_TEXT.len()), HELP_TEXT);
    }
}

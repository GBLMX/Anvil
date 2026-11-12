use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
#[test]
fn multi_buffer_switch_preserves_per_buffer_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "first").expect("write first");
    fs::write(&second, "second").expect("write second");

    let mut model = Model::open(&first).expect("open first");
    model.cursor = 2;
    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, &format!("e {}", second.display()));
    handle_event(&mut model, key(KeyCode::Enter));
    assert_eq!(model.path.as_ref(), Some(&second));
    assert_eq!(model.editor.text().slice(0..6), "second");

    model.cursor = 1;
    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, "bn");
    handle_event(&mut model, key(KeyCode::Enter));
    assert_eq!(model.path.as_ref(), Some(&first));
    assert_eq!(model.cursor, 2);

    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, "bn");
    handle_event(&mut model, key(KeyCode::Enter));
    assert_eq!(model.path.as_ref(), Some(&second));
    assert_eq!(model.cursor, 1);

    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, &format!("e {}", first.display()));
    handle_event(&mut model, key(KeyCode::Enter));
    assert_eq!(model.path.as_ref(), Some(&first));
    assert_eq!(model.buffers.entries.len(), 2);
}

#[test]
fn switching_buffers_preserves_visual_selection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "first").expect("write first");
    fs::write(&second, "second").expect("write second");
    let mut model = Model::open(&first).expect("open first");
    model.open_path(&second).expect("open second");
    command(&mut model, "bp");

    handle_event(&mut model, key(KeyCode::Char('v')));
    handle_event(&mut model, key(KeyCode::Right));
    assert!(model.edit.visual_anchor.is_some());

    command(&mut model, "bn");
    assert!(model.edit.visual_anchor.is_none());
    command(&mut model, "bp");
    assert_eq!(model.edit.visual_anchor, Some(0));
}

#[test]
fn multi_buffer_w_saves_the_active_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "first").expect("write first");
    fs::write(&second, "second").expect("write second");

    let mut model = Model::open(&first).expect("open first");
    model.open_path(&second).expect("open second");
    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "X");
    handle_event(&mut model, key(KeyCode::Esc));
    command(&mut model, "w");

    assert_eq!(fs::read_to_string(&second).expect("read second"), "Xsecond");
    assert_eq!(fs::read_to_string(&first).expect("read first"), "first");
    assert!(!model.modified);
}

#[test]
fn six_buffers_restore_cached_syntax_while_switching() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut model: Option<Model> = None;
    for index in 0..6 {
        let path = dir.path().join(format!("{index}.rs"));
        fs::write(&path, format!("fn function_{index}() {{}}\n")).expect("write");
        match model.as_mut() {
            Some(model) => model.open_path(&path).expect("open"),
            None => model = Some(Model::open(&path).expect("open first")),
        }
        let model = model.as_mut().expect("model");
        for _ in 0..200 {
            model.poll_syntax();
            if !model.syntax.borrow().highlights().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            !model.syntax.borrow().highlights().is_empty(),
            "syntax did not load for {index}.rs"
        );
    }

    let mut model = model.expect("model");
    assert_eq!(model.buffers.entries.len(), 6);
    for index in 0..6 {
        model.switch_to_buffer(index);
        assert!(
            !model.syntax.borrow().highlights().is_empty(),
            "buffer {index} lost cached syntax"
        );
    }
}

#[test]
fn open_buffer_spans_survive_global_cache_pressure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "fn first() {}\n").expect("write first");
    fs::write(&second, "fn second() {}\n").expect("write second");

    let mut model = Model::open(&first).expect("open first");
    wait_for_syntax(&mut model);
    model.open_path(&second).expect("open second");
    wait_for_syntax(&mut model);

    model.syntax.borrow_mut().fill_cache_for_test();
    model.switch_to_buffer(0);
    assert!(
        !model.syntax.borrow().highlights().is_empty(),
        "first buffer lost spans under cache pressure"
    );
    model.switch_to_buffer(1);
    assert!(
        !model.syntax.borrow().highlights().is_empty(),
        "second buffer lost spans under cache pressure"
    );
}

fn wait_for_syntax(model: &mut Model) {
    for _ in 0..200 {
        model.poll_syntax();
        if !model.syntax.borrow().highlights().is_empty() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("syntax did not load");
}
#[test]
fn load_buffer_syncs_the_current_buffer_before_switching() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "first").expect("write first");
    fs::write(&second, "second").expect("write second");

    let mut model = Model::open(&first).expect("open first");
    model.open_path(&second).expect("open second");
    model.switch_to_buffer(0);
    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "X");
    handle_event(&mut model, key(KeyCode::Esc));
    assert!(model.modified);

    model.switch_to_buffer(1);
    model.switch_to_buffer(0);
    assert_eq!(model.editor.text().slice(0..6), "Xfirst");
    assert!(model.modified);
}

#[test]
fn save_then_buffer_switch_round_trip_keeps_the_buffer_clean() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "first").expect("write first");
    fs::write(&second, "second").expect("write second");

    let mut model = Model::open(&first).expect("open first");
    model.open_path(&second).expect("open second");
    model.switch_to_buffer(0);
    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "X");
    handle_event(&mut model, key(KeyCode::Esc));
    command(&mut model, "w");
    assert!(!model.modified);

    model.switch_to_buffer(1);
    model.switch_to_buffer(0);
    assert!(!model.modified);
    assert_eq!(model.editor.text().slice(0..6), "Xfirst");
}

#[test]
fn cq_exits_with_status_one() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    command(&mut model, "cq");
    assert!(model.quit);
    assert_eq!(model.exit_code(), 1);
}

#[test]
fn editor_mode_keeps_save_failure_sticky_for_q_bang() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut model = Model::new(
        EditorState::from_text("dirty"),
        Some(directory.path().to_path_buf()),
    );
    model.set_editor_mode(true);
    model.modified = true;
    assert!(!model.save());
    assert_eq!(model.exit_code(), 1);

    model.force_quit();
    assert!(model.quit);
    assert_eq!(model.exit_code(), 1);
}

#[test]
fn stdin_read_only_buffer_rejects_mutation() {
    let mut model = Model::new(EditorState::from_text("log"), None);
    model.set_stdin_buffer(true);
    assert_eq!(model.buffer_name(0), "[stdin]");
    model.insert_text("x");
    assert_eq!(model.editor.text().slice(0..3), "log");
    assert!(model.status.text.contains("read-only"));
}
#[test]
fn read_only_file_mode_rejects_mutation_and_survives_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "first").expect("write first");
    fs::write(&second, "second").expect("write second");

    let mut model = Model::open(&first).expect("open first");
    model.set_read_only(true);
    model.insert_text("X");
    assert_eq!(model.editor.text().slice(0..5), "first");
    assert!(model.status.text.contains("read-only"));

    model.open_path(&second).expect("open second");
    model.insert_text("Y");
    assert_eq!(model.editor.text().slice(0..6), "second");
    assert!(model.status.text.contains("read-only"));
}

#[test]
fn mouse_tab_switch_preserves_the_active_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "first").expect("write first");
    fs::write(&second, "second").expect("write second");

    let mut model = Model::open(&first).expect("open first");
    model.open_path(&second).expect("open second");
    model.switch_to_buffer(0);
    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "X");
    handle_event(&mut model, key(KeyCode::Esc));

    let click = |column| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row: 1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    assert!(crate::mouse::handle_mouse(&mut model, click(13), 80, 24));
    assert_eq!(model.buffers.active, 1);
    assert!(crate::mouse::handle_mouse(&mut model, click(2), 80, 24));
    assert_eq!(model.buffers.active, 0);
    assert_eq!(model.editor.text().slice(0..6), "Xfirst");
    assert!(model.modified);
}

#[test]
fn closing_active_middle_buffer_keeps_the_following_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    let third = dir.path().join("third.txt");
    fs::write(&first, "first").expect("write first");
    fs::write(&second, "second").expect("write second");
    fs::write(&third, "third").expect("write third");

    let mut model = Model::open(&first).expect("open first");
    model.open_path(&second).expect("open second");
    model.open_path(&third).expect("open third");
    model.switch_to_buffer(1);
    model.close_buffer(1, false);

    assert_eq!(model.buffers.entries.len(), 2);
    assert_eq!(model.buffers.active, 1);
    assert_eq!(model.editor.text().slice(0..5), "third");
    assert_eq!(model.buffers.entries[1].editor.text().slice(0..5), "third");
}

#[test]
fn buffer_commands_require_a_second_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("only.txt");
    fs::write(&path, "only").expect("write");
    let mut model = Model::open(&path).expect("open");

    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, "bn");
    handle_event(&mut model, key(KeyCode::Enter));
    assert_eq!(model.path.as_ref(), Some(&path));
    assert_eq!(model.status.text, "E: only one buffer");
}

#[test]
fn new_and_close_buffer_lifecycle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("file.txt");
    fs::write(&path, "file").expect("write");
    let mut model = Model::open(&path).expect("open");

    command(&mut model, "new");
    assert_eq!(model.path, None);
    assert_eq!(model.buffers.entries.len(), 2);
    assert_eq!(model.buffers.active, 1);
    assert_eq!(model.editor.text().len_bytes(), 0);

    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "X");
    handle_event(&mut model, key(KeyCode::Esc));
    command(&mut model, "bd");
    assert!(matches!(
        model.ui.input_mode,
        InputMode::ConfirmClose { index: 1 }
    ));
    handle_event(&mut model, key(KeyCode::Char('c')));
    assert_eq!(model.path, None);

    command(&mut model, "bd");
    handle_event(&mut model, key(KeyCode::Char('n')));
    assert_eq!(model.path.as_ref(), Some(&path));
    assert_eq!(model.buffers.entries.len(), 1);

    command(&mut model, "bd");
    assert_eq!(model.path, None);
    assert!(model.buffers.entries.is_empty());
    assert_eq!(model.editor.text().len_bytes(), 0);
}

#[test]
fn closing_modified_buffer_can_save_or_discard() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("file.txt");
    fs::write(&path, "file").expect("write");
    let mut model = Model::open(&path).expect("open");
    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "X");
    handle_event(&mut model, key(KeyCode::Esc));
    command(&mut model, "bd");
    handle_event(&mut model, key(KeyCode::Char('y')));
    assert_eq!(fs::read_to_string(&path).expect("read"), "Xfile");
    assert_eq!(model.path, None);
    assert!(model.buffers.entries.is_empty());

    fs::write(&path, "file").expect("rewrite");
    let mut model = Model::open(&path).expect("open");
    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "X");
    handle_event(&mut model, key(KeyCode::Esc));
    command(&mut model, "bd");
    handle_event(&mut model, key(KeyCode::Char('n')));
    assert_eq!(fs::read_to_string(&path).expect("read"), "file");
    assert_eq!(model.path, None);
}

#[test]
fn failed_save_keeps_close_confirmation_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut model = Model::new(EditorState::from_text("dirty"), None);
    model.path = Some(dir.path().to_path_buf());
    model.modified = true;
    model.ui.input_mode = InputMode::ConfirmClose { index: 0 };
    model.queue_effect(Effect::SaveBuffer {
        index: 0,
        close: true,
    });
    model.drain_effects();

    assert!(matches!(
        model.ui.input_mode,
        InputMode::ConfirmClose { index: 0 }
    ));
    assert!(model.status.text.starts_with("E:"), "{}", model.status.text);
}

#[test]
fn close_indexed_buffer_keeps_active_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "first").expect("first");
    fs::write(&second, "second").expect("second");
    let mut model = Model::open(&first).expect("open");
    model.open_path(&second).expect("open second");

    command(&mut model, "bd 1");
    assert_eq!(model.path.as_ref(), Some(&second));
    assert_eq!(model.buffers.entries.len(), 1);
    assert_eq!(model.buffers.entries[0].path.as_ref(), Some(&second));
}

#[test]
fn closing_buffer_clears_matching_split() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("file.txt");
    fs::write(&path, "file").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.split_vertical();
    assert!(model.buffers.split.is_some());

    command(&mut model, "bd");
    assert!(model.buffers.split.is_none());
    assert!(!model.ui.focus.split);
    assert_eq!(model.path, None);
}

#[test]
fn closing_buffer_refuses_to_drop_dirty_split() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("file.txt");
    fs::write(&path, "file").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.split_vertical();
    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "X");
    handle_event(&mut model, key(KeyCode::Esc));
    model.focus_split(true);

    command(&mut model, "bd");
    assert!(
        model
            .buffers
            .split
            .as_ref()
            .is_some_and(|pane| pane.buffer.modified)
    );
    assert!(model.status.text.contains("split pane has unsaved changes"));
    assert_eq!(model.path.as_ref(), Some(&path));
}

#[test]
fn buffer_bar_shows_dirty_missing_and_read_only_states() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "first").expect("first");
    fs::write(&second, "second").expect("second");
    let mut model = Model::open(&first).expect("open first");
    model.open_path(&second).expect("open second");
    model.buffers.entries[0].modified = true;
    model.mark_missing(&second);

    let render = |model: &Model, width: u16, height: u16| {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| view(model, frame)).expect("draw");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    };
    let rendered = render(&model, 80, 12);
    assert!(rendered.contains("first.txt ●"), "{rendered}");
    assert!(rendered.contains("second.txt !"), "{rendered}");

    command(&mut model, "help");
    let rendered = render(&model, 80, 12);
    assert!(rendered.contains("[help] RO"), "{rendered}");
}

#[test]
fn buffer_bar_scrolls_to_many_buffers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut model: Option<Model> = None;
    for index in 0..10 {
        let path = dir.path().join(format!("f{index}.txt"));
        fs::write(&path, "x").expect("write");
        match model.as_mut() {
            Some(model) => model.open_path(&path).expect("open"),
            None => model = Some(Model::open(&path).expect("open first")),
        }
    }
    let model = model.expect("model");
    assert_eq!(model.buffers.entries.len(), 10);
    assert_eq!(model.buffers.active, 9);

    let backend = TestBackend::new(40, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("‹"), "{rendered}");
    assert!(rendered.contains("f9.txt"), "{rendered}");
}

#[test]
fn deleted_file_marks_the_buffer_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("gone.txt");
    fs::write(&path, "data").expect("write");
    let mut model = Model::open(&path).expect("open");
    fs::remove_file(&path).expect("remove");

    assert!(model.reload_external(&path).is_err());
    assert!(model.missing);
}

#[test]
fn external_clean_change_reloads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("file.txt");
    fs::write(&path, "old").expect("write");
    let mut model = Model::open(&path).expect("open");
    fs::write(&path, "disk").expect("external write");

    model.handle_external_change(path.clone());
    assert_eq!(model.editor.text().slice(0..4), "disk");
    assert!(!model.modified);
    assert!(model.status.text.contains("reloaded"));
}

#[test]
fn external_dirty_change_requires_confirmation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("file.txt");
    fs::write(&path, "old").expect("write");
    let mut model = Model::open(&path).expect("open");
    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "X");
    handle_event(&mut model, key(KeyCode::Esc));
    fs::write(&path, "disk").expect("external write");

    model.handle_external_change(path.clone());
    assert!(matches!(
        model.ui.input_mode,
        InputMode::ConfirmReload { .. }
    ));
    handle_event(&mut model, key(KeyCode::Char('n')));
    assert_eq!(model.editor.text().slice(0..4), "Xold");

    model.handle_external_change(path);
    handle_event(&mut model, key(KeyCode::Char('y')));
    assert_eq!(model.editor.text().slice(0..4), "disk");
    assert!(!model.modified);
}

#[test]
fn quit_guard_checks_inactive_buffers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "first").expect("write first");
    fs::write(&second, "second").expect("write second");

    let mut model = Model::open(&first).expect("open");
    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "X");
    handle_event(&mut model, key(KeyCode::Esc));
    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, &format!("e {}", second.display()));
    handle_event(&mut model, key(KeyCode::Enter));
    assert_eq!(model.path.as_ref(), Some(&second));

    for code in [KeyCode::Char(':'), KeyCode::Char('q'), KeyCode::Enter] {
        handle_event(&mut model, key(code));
    }
    assert!(!model.quit);
    assert!(model.status.text.contains("No write since last change"));

    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, "q!");
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(model.quit);
}

#[test]
fn gateway_required_refuses_file_operations_without_client() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("file.txt");
    let mut model = Model::new(EditorState::from_text("data"), None);
    model.workspace.require_gateway = true;
    model.workspace.path = directory.path().to_path_buf();
    model.path = Some(path);
    assert!(!model.save());
    assert!(model.status.text.contains("gateway unavailable"));

    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, "e other.txt");
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(model.status.text.contains("gateway unavailable"));
}

#[test]
fn gateway_refuses_paths_outside_workspace() {
    let directory = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.require_gateway = true;
    model.workspace.path = directory.path().to_path_buf();
    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(
        &mut model,
        &format!("e {}", outside.path().join("x").display()),
    );
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(
        model.status.text.contains("outside workspace"),
        "{}",
        model.status.text
    );
}

#[test]
fn dot_repeats_word_delete() {
    let mut model = Model::new(EditorState::from_text("one two three"), None);
    handle_event(&mut model, key(KeyCode::Char('d')));
    handle_event(&mut model, key(KeyCode::Char('w')));
    assert_eq!(model.editor.text().slice(0..9), "two three");
    handle_event(&mut model, key(KeyCode::Char('.')));
    assert_eq!(model.editor.text().slice(0..5), "three");
}

#[test]
fn dot_count_overrides_recorded_delete_count() {
    let mut model = Model::new(EditorState::from_text("one two three"), None);
    handle_event(&mut model, key(KeyCode::Char('2')));
    handle_event(&mut model, key(KeyCode::Char('d')));
    handle_event(&mut model, key(KeyCode::Char('w')));
    assert_eq!(model.editor.text().slice(0..5), "three");
    handle_event(&mut model, key(KeyCode::Char('3')));
    handle_event(&mut model, key(KeyCode::Char('.')));
    assert_eq!(model.editor.text().len_bytes(), 0);
}

#[test]
fn dot_repeats_append_line_end_insert() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    handle_event(&mut model, key(KeyCode::Char('A')));
    type_text(&mut model, "foo");
    handle_event(&mut model, key(KeyCode::Esc));
    handle_event(&mut model, key(KeyCode::Char('.')));
    assert_eq!(model.editor.text().slice(0..7), "xfoofoo");
}

#[test]
fn dot_repeats_inner_word_change() {
    let mut model = Model::new(EditorState::from_text("one two"), None);
    handle_event(&mut model, key(KeyCode::Char('c')));
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, key(KeyCode::Char('w')));
    type_text(&mut model, "X");
    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.editor.text().slice(0..5), "X two");
    handle_event(&mut model, key(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Char('.')));
    assert_eq!(model.editor.text().slice(0..3), "X X");
}

#[test]
fn failed_delete_preserves_previous_last_change() {
    let mut model = Model::new(EditorState::from_text("one"), None);
    handle_event(&mut model, key(KeyCode::Char('d')));
    handle_event(&mut model, key(KeyCode::Char('w')));
    let previous = model.edit.last_change.clone();
    assert_eq!(model.editor.text().len_bytes(), 0);
    handle_event(&mut model, key(KeyCode::Char('d')));
    handle_event(&mut model, key(KeyCode::Char('w')));
    assert_eq!(model.edit.last_change, previous);
}

#[test]
fn empty_insert_does_not_replace_last_change() {
    let mut model = Model::new(EditorState::from_text("one two three"), None);
    handle_event(&mut model, key(KeyCode::Char('d')));
    handle_event(&mut model, key(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, key(KeyCode::Esc));
    handle_event(&mut model, key(KeyCode::Char('.')));
    assert_eq!(model.editor.text().slice(0..5), "three");
}

#[test]
fn backspace_updates_insert_session_text() {
    let mut model = Model::new(EditorState::from_text(""), None);
    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "abc");
    handle_event(&mut model, key(KeyCode::Backspace));
    handle_event(&mut model, key(KeyCode::Esc));
    handle_event(&mut model, key(KeyCode::Char('.')));
    assert_eq!(model.editor.text().slice(0..4), "abab");
}

#[test]
fn backspace_at_document_start_and_line_boundaries_is_safe() {
    let mut empty = Model::new(EditorState::from_text(""), None);
    for _ in 0..2000 {
        empty.backspace();
    }
    assert_eq!(empty.editor.text().len_bytes(), 0);

    let mut lines = Model::new(EditorState::from_text("a\nb\nc"), None);
    for _ in 0..2 {
        let newline = lines
            .editor
            .text()
            .slice(0..lines.editor.text().len_bytes())
            .find('\n');
        let Some(newline) = newline else {
            break;
        };
        lines.cursor = newline + 1;
        lines.backspace();
    }
    assert_eq!(lines.editor.text().slice(0..3), "abc");
    for _ in 0..2000 {
        lines.backspace();
    }
    assert_eq!(lines.cursor, 0);
}

#[test]
fn bracketed_multiline_paste_preserves_lines_and_undoes_once() {
    let mut model = Model::new(EditorState::from_text(""), None);
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, Event::Paste("one\ntwo\n".to_owned()));
    assert_eq!(model.editor.text().slice(0..8), "one\ntwo\n");
    handle_event(&mut model, key(KeyCode::Esc));
    handle_event(&mut model, key(KeyCode::Char('u')));
    assert_eq!(model.editor.text().len_bytes(), 0);
}

#[test]
fn wide_characters_use_display_columns() {
    let mut model = Model::new(EditorState::from_text(""), None);
    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "你好");
    let position = model
        .editor
        .index()
        .position(model.editor.text(), model.cursor)
        .expect("position");
    assert_eq!(position.display_column, 4);
    model.backspace();
    assert_eq!(model.editor.text().slice(0..3), "你");
    let position = model
        .editor
        .index()
        .position(model.editor.text(), model.cursor)
        .expect("position");
    assert_eq!(position.display_column, 2);
}

#[test]
fn paste_normalizes_crlf_and_cr() {
    let mut model = Model::new(EditorState::from_text(""), None);
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, Event::Paste("a\r\nb\rc".to_owned()));
    assert_eq!(model.editor.text().slice(0..5), "a\nb\nc");
}

#[test]
fn command_line_paste_does_not_embed_newlines() {
    let mut model = Model::new(EditorState::from_text(""), None);
    handle_event(&mut model, key(KeyCode::Char(':')));
    handle_event(&mut model, Event::Paste("w\r\nq".to_owned()));
    match &model.ui.input_mode {
        InputMode::CommandLine { buffer } => assert_eq!(buffer.text(), "w q"),
        other => panic!("unexpected mode: {other:?}"),
    }
}

#[test]
fn moved_insert_session_does_not_poison_last_change() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    model.begin_insert(InsertOrigin::InsertHere);
    model.capture_insert("x");
    model.move_char(-1);
    model.finish_insert_session();
    assert!(model.edit.last_change.is_none());
}

#[test]
fn paste_drops_control_characters() {
    let mut model = Model::new(EditorState::from_text(""), None);
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, Event::Paste("a\u{7}b\u{1b}c".to_owned()));
    assert_eq!(model.editor.text().slice(0..3), "abc");
}

#[test]
fn delete_inner_and_around_word() {
    let mut inner = Model::new(EditorState::from_text("one two"), None);
    handle_event(&mut inner, key(KeyCode::Char('d')));
    handle_event(&mut inner, key(KeyCode::Char('i')));
    handle_event(&mut inner, key(KeyCode::Char('w')));
    assert_eq!(inner.editor.text().slice(0..4), " two");

    let mut around = Model::new(EditorState::from_text("one two"), None);
    handle_event(&mut around, key(KeyCode::Char('d')));
    handle_event(&mut around, key(KeyCode::Char('a')));
    handle_event(&mut around, key(KeyCode::Char('w')));
    assert_eq!(around.editor.text().slice(0..3), "two");
}

#[test]
fn change_inner_and_around_word() {
    let mut inner = Model::new(EditorState::from_text("one two"), None);
    handle_event(&mut inner, key(KeyCode::Char('c')));
    handle_event(&mut inner, key(KeyCode::Char('i')));
    handle_event(&mut inner, key(KeyCode::Char('w')));
    type_text(&mut inner, "X");
    handle_event(&mut inner, key(KeyCode::Esc));
    assert_eq!(inner.editor.text().slice(0..5), "X two");

    let mut around = Model::new(EditorState::from_text("one two"), None);
    handle_event(&mut around, key(KeyCode::Char('c')));
    handle_event(&mut around, key(KeyCode::Char('a')));
    handle_event(&mut around, key(KeyCode::Char('w')));
    type_text(&mut around, "X");
    handle_event(&mut around, key(KeyCode::Esc));
    assert_eq!(around.editor.text().slice(0..4), "Xtwo");
}

#[test]
fn delete_inner_quote_does_not_change() {
    let mut model = Model::new(EditorState::from_text("say \"hello\" now"), None);
    model.cursor = 6;
    for key_code in [KeyCode::Char('d'), KeyCode::Char('i'), KeyCode::Char('"')] {
        handle_event(&mut model, key(key_code));
    }
    assert_eq!(model.editor.text().slice(0..10), "say \"\" now");
}

#[test]
fn delete_around_quote_removes_delimiters() {
    let mut model = Model::new(EditorState::from_text("say \"hello\" now"), None);
    model.cursor = 6;
    for key_code in [KeyCode::Char('d'), KeyCode::Char('a'), KeyCode::Char('"')] {
        handle_event(&mut model, key(key_code));
    }
    assert_eq!(model.editor.text().slice(0..8), "say  now");
}

#[test]
fn change_around_quote_repeats_with_quotes() {
    let mut model = Model::new(EditorState::from_text("say \"hello\" now"), None);
    model.cursor = 6;
    for key_code in [KeyCode::Char('c'), KeyCode::Char('a'), KeyCode::Char('"')] {
        handle_event(&mut model, key(key_code));
    }
    type_text(&mut model, "X");
    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.editor.text().slice(0..9), "say X now");
}

#[test]
fn bracket_text_objects_support_delete_change_and_around() {
    let mut inner = Model::new(EditorState::from_text("call(foo, bar)"), None);
    inner.cursor = 6;
    for key_code in [KeyCode::Char('d'), KeyCode::Char('i'), KeyCode::Char('(')] {
        handle_event(&mut inner, key(key_code));
    }
    assert_eq!(inner.editor.text().slice(0..6), "call()");

    let mut around = Model::new(EditorState::from_text("call(foo, bar)"), None);
    around.cursor = 6;
    for key_code in [KeyCode::Char('d'), KeyCode::Char('a'), KeyCode::Char('(')] {
        handle_event(&mut around, key(key_code));
    }
    assert_eq!(around.editor.text().slice(0..4), "call");

    let mut change = Model::new(EditorState::from_text("call(foo, bar)"), None);
    change.cursor = 6;
    for key_code in [KeyCode::Char('c'), KeyCode::Char('i'), KeyCode::Char('(')] {
        handle_event(&mut change, key(key_code));
    }
    type_text(&mut change, "X");
    handle_event(&mut change, key(KeyCode::Esc));
    assert_eq!(change.editor.text().slice(0..7), "call(X)");
}

#[test]
fn paragraph_text_objects_delete_and_change() {
    let mut inner = Model::new(EditorState::from_text("a\nb\n\nc\nd"), None);
    inner.cursor = 1;
    for key_code in [KeyCode::Char('d'), KeyCode::Char('i'), KeyCode::Char('p')] {
        handle_event(&mut inner, key(key_code));
    }
    assert_eq!(inner.editor.text().slice(0..3), "\nc\n");

    let mut around = Model::new(EditorState::from_text("a\nb\n\nc\nd"), None);
    around.cursor = 1;
    for key_code in [KeyCode::Char('d'), KeyCode::Char('a'), KeyCode::Char('p')] {
        handle_event(&mut around, key(key_code));
    }
    assert_eq!(around.editor.text().slice(0..3), "c\nd");

    let mut change = Model::new(EditorState::from_text("a\nb\n\nc\nd"), None);
    change.cursor = 1;
    for key_code in [KeyCode::Char('c'), KeyCode::Char('i'), KeyCode::Char('p')] {
        handle_event(&mut change, key(key_code));
    }
    type_text(&mut change, "X");
    handle_event(&mut change, key(KeyCode::Esc));
    assert_eq!(change.editor.text().slice(0..3), "X\nc");
}

#[test]
fn tag_text_objects_delete_change_and_around() {
    let mut inner = Model::new(EditorState::from_text("<p>hi</p>"), None);
    inner.cursor = 4;
    for key_code in [KeyCode::Char('d'), KeyCode::Char('i'), KeyCode::Char('t')] {
        handle_event(&mut inner, key(key_code));
    }
    assert_eq!(inner.editor.text().len_bytes(), 7);

    let mut around = Model::new(EditorState::from_text("<p>hi</p>"), None);
    around.cursor = 4;
    for key_code in [KeyCode::Char('d'), KeyCode::Char('a'), KeyCode::Char('t')] {
        handle_event(&mut around, key(key_code));
    }
    assert_eq!(around.editor.text().len_bytes(), 0);

    let mut change = Model::new(EditorState::from_text("<p>hi</p>"), None);
    change.cursor = 4;
    for key_code in [KeyCode::Char('c'), KeyCode::Char('i'), KeyCode::Char('t')] {
        handle_event(&mut change, key(key_code));
    }
    type_text(&mut change, "X");
    handle_event(&mut change, key(KeyCode::Esc));
    assert_eq!(change.editor.text().slice(0..3), "<p>");
}

#[test]
fn word_text_objects_honor_counts() {
    let mut model = Model::new(EditorState::from_text("one two three"), None);
    model.cursor = 0;
    for key_code in [
        KeyCode::Char('2'),
        KeyCode::Char('d'),
        KeyCode::Char('i'),
        KeyCode::Char('w'),
    ] {
        handle_event(&mut model, key(key_code));
    }
    assert_eq!(model.editor.text().slice(0..6), " three");
}

#[test]
fn paragraph_text_objects_honor_counts() {
    let mut model = Model::new(EditorState::from_text("a\nb\n\nc\nd"), None);
    model.cursor = 0;
    for key_code in [
        KeyCode::Char('2'),
        KeyCode::Char('d'),
        KeyCode::Char('i'),
        KeyCode::Char('p'),
    ] {
        handle_event(&mut model, key(key_code));
    }
    assert_eq!(model.editor.text().len_bytes(), 0);
}

#[test]
fn visual_adapter_maps_character_and_line_selection() {
    let mut character = Model::new(EditorState::from_text("abc\ndef"), None);
    character.cursor = 0;
    handle_event(&mut character, key(KeyCode::Char('v')));
    handle_event(&mut character, key(KeyCode::Right));
    handle_event(&mut character, key(KeyCode::Char('d')));
    assert_eq!(character.editor.text().slice(0..4), "c\nde");

    let mut line = Model::new(EditorState::from_text("abc\ndef"), None);
    line.cursor = 0;
    handle_event(&mut line, key(KeyCode::Char('V')));
    handle_event(&mut line, key(KeyCode::Down));
    handle_event(&mut line, key(KeyCode::Char('d')));
    assert_eq!(line.editor.text().len_bytes(), 0);
}

#[test]
fn visual_surround_uses_operation_scoped_modal_layer() {
    let mut model = Model::new(EditorState::from_text("word"), None);
    model.welcome = false;
    model.cursor = 0;
    for key_code in [
        KeyCode::Char('v'),
        KeyCode::Right,
        KeyCode::Char('s'),
        KeyCode::Char('('),
    ] {
        handle_event(&mut model, key(key_code));
    }
    assert_eq!(model.editor.text().slice(0..6), "(wo)rd");
    assert!(model.edit.modal_layer.is_none());
    assert!(model.edit.visual_anchor.is_none());
}

#[test]
fn visual_delete_keeps_register_and_undo() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    model.cursor = 0;
    for key_code in [KeyCode::Char('v'), KeyCode::Right, KeyCode::Char('d')] {
        handle_event(&mut model, key(key_code));
    }
    assert_eq!(model.editor.text().slice(0..1), "c");
    handle_event(&mut model, key(KeyCode::Char('p')));
    assert_eq!(model.editor.text().slice(0..3), "abc");
}

#[test]
fn visual_delete_undoes_and_handles_reversal_and_empty_buffer() {
    let mut reversed = Model::new(EditorState::from_text("abc"), None);
    reversed.cursor = 2;
    for key_code in [KeyCode::Char('v'), KeyCode::Left, KeyCode::Char('d')] {
        handle_event(&mut reversed, key(key_code));
    }
    assert_eq!(reversed.editor.text().slice(0..1), "a");
    handle_event(&mut reversed, key(KeyCode::Char('u')));
    assert_eq!(reversed.editor.text().slice(0..3), "abc");

    let mut empty = Model::new(EditorState::from_text(""), None);
    for key_code in [KeyCode::Char('v'), KeyCode::Char('d')] {
        handle_event(&mut empty, key(key_code));
    }
    assert_eq!(empty.editor.text().len_bytes(), 0);
}

#[test]
fn unknown_object_failure_preserves_text() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    model.cursor = 0;
    for key_code in [KeyCode::Char('d'), KeyCode::Char('i'), KeyCode::Char('"')] {
        handle_event(&mut model, key(key_code));
    }
    assert_eq!(model.editor.text().slice(0..3), "abc");
    assert!(model.status.text.starts_with("E:"));
}

#[test]
fn visual_delete_applies_to_multiple_selections_in_one_undo() {
    let mut model = Model::new(EditorState::from_text("foo foo"), None);
    model.cursor = 0;
    model.select_word_occurrences();
    model.delete_visual();
    model.ui.input_mode = InputMode::Normal;
    assert_eq!(model.editor.text().slice(0..1), " ");
    handle_event(&mut model, key(KeyCode::Char('u')));
    assert_eq!(model.editor.text().len_bytes(), 7);
}

#[test]
fn focus_change_auto_save_writes_the_active_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "a").expect("first");
    fs::write(&second, "b").expect("second");
    let mut model = Model::open(&first).expect("open");
    model.open_path(&second).expect("second");
    model.switch_buffer(-1);
    model.settings.editor.auto_save = true;
    model.insert_text("X");
    assert!(model.modified);

    model.switch_buffer(1);

    assert_eq!(fs::read_to_string(&first).expect("first content"), "Xa");
    assert!(!model.modified);
}

#[test]
fn failed_auto_save_aborts_the_focus_change() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "a").expect("first");
    fs::write(&second, "b").expect("second");
    let mut model = Model::open(&first).expect("open");
    model.open_path(&second).expect("second");
    model.switch_buffer(-1);
    model.settings.editor.auto_save = true;
    model.insert_text("X");
    model.path = Some(dir.path().to_path_buf());

    model.switch_buffer(1);

    assert_eq!(model.buffers.active, 0);
    assert!(model.modified);
    assert!(model.status.text.contains("E:"));
}

#[test]
fn split_focus_change_auto_saves_the_split_pane() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.settings.editor.auto_save = true;

    model.split_vertical();
    model.focus_split(true);
    model.insert_text("X");
    assert!(model.modified);

    model.focus_split(false);

    assert!(!model.ui.focus.split);
    assert_eq!(fs::read_to_string(&path).expect("content"), "Xa");
    assert!(!model.modified);
}

#[test]
fn focus_editor_auto_saves_a_dirty_split_pane() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.settings.editor.auto_save = true;

    model.split_vertical();
    model.focus_split(true);
    model.insert_text("X");
    assert!(model.modified);

    model.focus_editor();

    assert!(!model.ui.focus.split);
    assert_eq!(fs::read_to_string(&path).expect("content"), "Xa");
    assert!(!model.modified);
}

#[test]
fn opening_another_file_auto_saves_the_active_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "a").expect("first");
    fs::write(&second, "b").expect("second");
    let mut model = Model::open(&first).expect("open");
    model.settings.editor.auto_save = true;
    model.insert_text("X");

    model.open_path(&second).expect("second");

    assert_eq!(fs::read_to_string(&first).expect("first content"), "Xa");
    assert_eq!(
        model.path.as_deref(),
        Some(second.canonicalize().expect("second").as_path())
    );
}

#[test]
fn opening_another_file_refuses_a_dirty_unnamed_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::new(EditorState::from_text("draft"), None);
    model.modified = true;

    assert!(model.open_path(&path).is_err());

    assert!(model.path.is_none());
    assert_eq!(model.editor.text().slice(0..5), "draft");
    assert!(model.status.text.contains("save or discard"));
}

#[test]
fn quitting_auto_saves_the_active_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.settings.editor.auto_save = true;
    model.insert_text("X");

    model.request_quit();

    assert!(model.quit);
    assert_eq!(fs::read_to_string(&path).expect("content"), "Xa");
}

#[test]
fn ex_open_auto_saves_before_switching() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "a").expect("first");
    fs::write(&second, "b").expect("second");
    let mut model = Model::open(&first).expect("open");
    model.settings.editor.auto_save = true;
    model.insert_text("X");

    model.open_file(second.to_str().expect("path"));

    assert_eq!(fs::read_to_string(&first).expect("first content"), "Xa");
    assert_eq!(
        model.path.as_deref(),
        Some(second.canonicalize().expect("second").as_path())
    );
}

#[test]
fn new_buffer_auto_saves_before_switching() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.settings.editor.auto_save = true;
    model.insert_text("X");

    model.new_buffer();

    assert!(model.path.is_none());
    assert_eq!(fs::read_to_string(&path).expect("content"), "Xa");
}

#[test]
fn closing_a_dirty_split_pane_auto_saves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.settings.editor.auto_save = true;
    model.split_vertical();
    model.focus_split(true);
    model.insert_text("X");

    model.close_split();

    assert!(model.buffers.split.is_none());
    assert_eq!(fs::read_to_string(&path).expect("content"), "Xa");
}

#[test]
fn closing_the_active_buffer_auto_saves_first() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.settings.editor.auto_save = true;
    model.insert_text("X");

    model.close_buffer(0, false);

    assert!(model.buffers.entries.is_empty());
    assert!(model.path.is_none());
    assert_eq!(fs::read_to_string(&path).expect("content"), "Xa");
}

#[test]
fn focus_lost_auto_saves_the_active_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.settings.editor.auto_save = true;
    model.insert_text("X");

    handle_event(&mut model, Event::FocusLost);

    assert_eq!(fs::read_to_string(&path).expect("content"), "Xa");
    assert!(!model.modified);
}

#[test]
fn focus_lost_auto_saves_while_insert_is_active() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.settings.editor.auto_save = true;

    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, key(KeyCode::Char('X')));
    assert!(matches!(model.ui.input_mode, InputMode::Insert));

    handle_event(&mut model, Event::FocusLost);

    assert_eq!(fs::read_to_string(&path).expect("content"), "Xa");
    assert!(!model.modified);
    assert!(matches!(model.ui.input_mode, InputMode::Insert));
}

#[test]
fn focus_lost_skips_auto_save_during_blocking_confirmation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.settings.editor.auto_save = true;
    model.insert_text("X");
    model.ui.input_mode = InputMode::ConfirmClose { index: 0 };

    handle_event(&mut model, Event::FocusLost);

    assert_eq!(fs::read_to_string(&path).expect("content"), "a");
    assert!(model.modified);
    assert!(matches!(
        model.ui.input_mode,
        InputMode::ConfirmClose { .. }
    ));
}

#[test]
fn focus_lost_auto_saves_while_search_is_active() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.settings.editor.auto_save = true;
    model.insert_text("X");
    model.ui.input_mode = InputMode::Search {
        query: InputBuffer::default(),
    };

    handle_event(&mut model, Event::FocusLost);

    assert_eq!(fs::read_to_string(&path).expect("content"), "Xa");
    assert!(!model.modified);
}

#[test]
fn switching_to_the_same_buffer_does_not_auto_save() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.settings.editor.auto_save = true;
    model.insert_text("X");

    model.switch_buffer(1);

    assert_eq!(fs::read_to_string(&path).expect("content"), "a");
    assert!(model.modified);
    assert!(model.status.text.contains("only one buffer"));
}

#[test]
fn ex_opening_the_current_file_does_not_auto_save() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.settings.editor.auto_save = true;
    model.insert_text("X");

    model.open_file(path.to_str().expect("path"));

    assert_eq!(fs::read_to_string(&path).expect("content"), "a");
    assert!(model.modified);
    assert!(model.status.text.contains("Already editing"));
}

#[test]
fn format_on_save_without_lsp_still_saves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "a").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.settings.editor.format_on_save = true;
    model.insert_text("x");
    assert!(model.save());
    assert_eq!(fs::read_to_string(&path).expect("content"), "xa");
    assert!(!model.language.lsp_requests.format_on_save_pending);
}

#[test]
fn toggles_breakpoint_gutter_marker() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "one\ntwo\n").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.cursor = model.offset_at_utf16(1, 0);
    model.editor.set_selection_bytes(model.cursor, model.cursor);
    model.toggle_breakpoint();
    assert!(model.breakpoint_marker(&path, 1));
    let gutter = model
        .visible_gutter(5)
        .into_iter()
        .flat_map(|line| line.spans.into_iter())
        .map(|span| span.content.into_owned())
        .collect::<String>();
    assert!(gutter.contains('●'), "{gutter}");
    model.toggle_breakpoint();
    assert!(!model.breakpoint_marker(&path, 1));
}

use super::*;
#[test]
fn renders_start_dashboard_for_an_empty_launch() {
    let mut model = Model::new(EditorState::from_text(""), None);
    assert!(model.welcome);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("ANVIL"));
    assert!(rendered.contains("Space f"));

    handle_event(&mut model, key(KeyCode::Char('i')));
    assert!(!model.welcome);
    assert!(matches!(model.ui.input_mode, InputMode::Insert));
}

#[test]
fn dashboard_survives_help_round_trip() {
    let mut model = Model::new(EditorState::from_text(""), None);
    handle_event(&mut model, key(KeyCode::Char('?')));
    assert!(model.help_back.is_some());
    assert!(!model.welcome);

    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, "q");
    handle_event(&mut model, key(KeyCode::Enter));

    assert!(model.help_back.is_none());
    assert!(model.welcome);
    assert_eq!(model.editor.text().slice(0..0), "");
}

#[test]
fn start_dashboard_does_not_replace_provided_text() {
    let model = Model::new(EditorState::from_text("alpha"), None);
    assert!(!model.welcome);
}

#[test]
fn renders_visible_lines() {
    let model = Model::new(EditorState::from_text("alpha\nbeta\ngamma"), None);
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let buffer = terminal.backend().buffer();
    let rendered = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("alpha"));
    assert!(rendered.contains("beta"));
    assert!(rendered.contains("gamma"));
    assert!(rendered.contains("1 alpha"), "{rendered}");
}

#[test]
fn busy_operation_renders_spinner() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.busy = Some("copying".to_owned());
    model.spinner = 0;
    let backend = TestBackend::new(80, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("copying"), "{rendered}");
    assert!(rendered.contains("⠋"), "{rendered}");
}

#[test]
fn indent_guides_do_not_overwrite_code() {
    let model = Model::new(EditorState::from_text("    let value = 42;"), None);
    let backend = TestBackend::new(40, 6);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("let value = 42;"), "{rendered}");
}

#[test]
fn tree_visibility_toggles_hidden_directories() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir(dir.path().join("src")).expect("src");
    fs::create_dir(dir.path().join(".github")).expect("github");
    fs::create_dir(dir.path().join(".hidden")).expect("hidden");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace = dir.path().to_path_buf();
    model.open_tree();
    assert!(
        model
            .tree
            .iter()
            .any(|entry| entry.path.ends_with(".github"))
    );
    assert!(
        !model
            .tree
            .iter()
            .any(|entry| entry.path.ends_with(".hidden"))
    );
    model.toggle_hidden();
    assert!(
        model
            .tree
            .iter()
            .any(|entry| entry.path.ends_with(".hidden"))
    );
}

#[test]
fn tree_creates_and_renames_files_and_directories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace = workspace.clone();
    model.open_tree();

    model.finish_tree_create(workspace.clone(), "new.txt".to_owned(), CreateKind::File);
    let file = workspace.join("new.txt");
    assert!(file.exists());
    assert_eq!(model.path.as_deref(), Some(file.as_path()));

    model.finish_tree_rename(file.clone(), "renamed.txt".to_owned());
    let renamed = workspace.join("renamed.txt");
    assert!(renamed.exists());
    assert!(!file.exists());
    assert_eq!(model.path.as_deref(), Some(renamed.as_path()));

    model.finish_tree_create(
        workspace.clone(),
        "components".to_owned(),
        CreateKind::Directory,
    );
    assert!(workspace.join("components").is_dir());
}

#[test]
fn tree_moves_copies_and_recursively_deletes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    fs::create_dir(workspace.join("src")).expect("src");
    fs::create_dir(workspace.join("dest")).expect("dest");
    let source = workspace.join("src/file.txt");
    fs::write(&source, "content").expect("source");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace = workspace.clone();
    model.open_tree();
    model.open_path(&source).expect("open");

    model.finish_tree_transfer(source.clone(), false, "dest".to_owned());
    let moved = workspace.join("dest/file.txt");
    assert!(!source.exists());
    assert!(moved.exists());
    assert_eq!(model.path.as_deref(), Some(moved.as_path()));

    model.finish_tree_transfer(moved.clone(), true, "src".to_owned());
    assert!(workspace.join("src/file.txt").exists());
    assert!(moved.exists());

    model.finish_tree_transfer(moved.clone(), true, "../".to_owned());
    assert!(model.status.contains("outside the workspace"));

    model.refresh_tree(Some(workspace.join("src")));
    model.begin_tree_transfer(true);
    assert!(model.status.contains("files only"));
    model.busy = Some("copying".to_owned());
    model.finish_tree_transfer(moved.clone(), true, "src".to_owned());
    assert!(model.status.contains("already in progress"));
    assert!(moved.exists());
    model.busy = None;

    model.finish_tree_delete(workspace.join("src"));
    assert!(!workspace.join("src").exists());
    assert!(moved.exists());
}

#[test]
fn tree_delete_requires_an_unopened_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    fs::write(workspace.join("open.txt"), "open").expect("open file");
    fs::write(workspace.join("closed.txt"), "closed").expect("closed file");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace = workspace.clone();
    model.open_tree();
    model.open_path(workspace.join("open.txt")).expect("open");

    model.refresh_tree(Some(workspace.join("open.txt")));
    model.begin_tree_delete();
    assert!(model.status.contains("close the open buffer"));

    model.refresh_tree(Some(workspace.join("closed.txt")));
    model.finish_tree_delete(workspace.join("closed.txt"));
    assert!(!workspace.join("closed.txt").exists());
}

#[test]
fn tree_renders_file_and_folder_icons() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("main.rs"), "").expect("file");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace = dir.path().to_path_buf();
    model.welcome = false;
    model.open_tree();

    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    let expected = if model.ascii_icons {
        icon_theme::ascii_glyph(icon_theme::IconKind::Rust)
    } else {
        icon_theme::nerd_glyph(icon_theme::IconKind::Rust)
    };
    assert!(rendered.contains(expected), "{rendered}");
    assert!(rendered.contains("main.rs"), "{rendered}");
}

#[test]
fn tree_renders_ascii_icon_fallback() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("main.rs"), "").expect("file");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace = dir.path().to_path_buf();
    model.ascii_icons = true;
    model.welcome = false;
    model.open_tree();

    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("[rs]"), "{rendered}");
    assert!(rendered.contains("main.rs"), "{rendered}");
}

#[test]
fn tree_selection_scrolls_into_view() {
    let dir = tempfile::tempdir().expect("tempdir");
    for index in 0..20 {
        fs::write(dir.path().join(format!("f{index:02}.txt")), "").expect("file");
    }
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace = dir.path().to_path_buf();
    model.welcome = false;
    model.open_tree();
    model.tree_selected = Some(19);

    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("f19.txt"), "{rendered}");
}

#[test]
fn fuzzy_selection_scrolls_into_view() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.welcome = false;
    model.fuzzy = Some(FuzzyState {
        generation: 0,
        query: String::new(),
        entries: (0..20).map(|index| format!("f{index:02}.txt")).collect(),
        filtered: (0..20).collect(),
        selected: 19,
    });

    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("f19.txt"), "{rendered}");
}
#[test]
fn fuzzy_preview_and_icon_render_on_wide_terminals() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.welcome = false;
    model.fuzzy = Some(FuzzyState {
        generation: 0,
        query: String::new(),
        entries: vec!["src/main.rs".to_owned()],
        filtered: vec![0],
        selected: 0,
    });
    model.preview = PreviewState {
        path: Some("src/main.rs".to_owned()),
        lines: vec!["fn main() {}".to_owned()],
        ..PreviewState::default()
    };

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("src/main.rs"), "{rendered}");
    assert!(rendered.contains("Preview"), "{rendered}");
    assert!(rendered.contains("fn main() {}"), "{rendered}");
}

#[test]
fn fuzzy_preview_stacks_below_list_on_narrow_terminals() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.welcome = false;
    model.fuzzy = Some(FuzzyState {
        generation: 0,
        query: String::new(),
        entries: vec!["src/main.rs".to_owned()],
        filtered: vec![0],
        selected: 0,
    });
    model.preview = PreviewState {
        path: Some("src/main.rs".to_owned()),
        lines: vec!["fn main() {}".to_owned()],
        ..PreviewState::default()
    };

    let backend = TestBackend::new(60, 20);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("src/main.rs"), "{rendered}");
    assert!(rendered.contains("Preview"), "{rendered}");
    assert!(rendered.contains("fn main() {}"), "{rendered}");
}

#[test]
fn notification_progress_and_problems_render_without_focus_change() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    model.welcome = false;
    model.notify(
        anvil_session::ui::NotificationSeverity::Info,
        "test",
        "hello",
    );
    let progress = model.ui.progress.start("copy", "working", true);
    model
        .diagnostics
        .insert(PathBuf::from("diag.rs"), vec![diagnostic(0, 0, "problem")]);
    model.rebuild_problems();
    model.ui.problems.visible = true;
    let focus = model.ui.focus;

    let render = |model: &Model| {
        let backend = TestBackend::new(60, 16);
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
    let rendered = render(&model);
    assert!(rendered.contains("Notifications"), "{rendered}");
    assert!(rendered.contains("Progress"), "{rendered}");
    assert!(rendered.contains("Problems"), "{rendered}");
    assert_eq!(model.ui.focus, focus);

    model.ui.progress.finish(progress);
    let rendered = render(&model);
    assert!(!rendered.contains("Progress"), "{rendered}");
}

#[test]
fn status_bar_renders_path_diagnostics_and_position() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    model.welcome = false;
    model.workspace = PathBuf::from("/ws");
    model.path = Some(PathBuf::from(
        "/ws/src/company/project/module/very/long/path/Button.tsx",
    ));

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("..."), "{rendered}");
    assert!(rendered.contains("Button.tsx"), "{rendered}");
    assert!(rendered.contains("diag 0"), "{rendered}");
    assert!(rendered.contains("Ln 1, Col 1"), "{rendered}");
}

#[test]
fn read_only_mode_renders_in_the_title() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    model.welcome = false;
    model.set_read_only(true);
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("[RO]"), "{rendered}");
}

#[test]
fn pipe_locations_render_a_bounded_panel() {
    let mut model = Model::new(EditorState::from_text("log"), None);
    model.welcome = false;
    model.set_pipe_locations("src/main.rs:2:3: error");
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("Locations · 1"), "{rendered}");
    assert!(rendered.contains("src/main.rs:2:3 error"), "{rendered}");
}
#[test]
fn command_line_renders_physical_cursor_at_the_edit_position() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    model.welcome = false;
    handle_event(&mut model, key(KeyCode::Char(':')));
    handle_event(&mut model, key(KeyCode::Char('a')));
    handle_event(&mut model, key(KeyCode::Char('b')));
    handle_event(&mut model, key(KeyCode::Left));

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let cursor = terminal.backend().cursor_position();
    assert_eq!((cursor.x, cursor.y), (8, 19));
}

#[test]
fn search_renders_physical_cursor_at_the_query_position() {
    let mut model = Model::new(EditorState::from_text("alpha beta"), None);
    model.welcome = false;
    handle_event(&mut model, key(KeyCode::Char('/')));
    handle_event(&mut model, key(KeyCode::Char('a')));
    handle_event(&mut model, key(KeyCode::Char('b')));
    handle_event(&mut model, key(KeyCode::Left));

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let cursor = terminal.backend().cursor_position();
    assert_eq!((cursor.x, cursor.y), (8, 19));
}

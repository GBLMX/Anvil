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
    assert!(model.buffers.help_back.is_some());
    assert!(!model.welcome);

    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, "q");
    handle_event(&mut model, key(KeyCode::Enter));

    assert!(model.buffers.help_back.is_none());
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
    model.workspace.path = dir.path().to_path_buf();
    model.open_tree();
    assert!(
        model
            .tree
            .entries
            .iter()
            .any(|entry| entry.path.ends_with(".github"))
    );
    assert!(
        !model
            .tree
            .entries
            .iter()
            .any(|entry| entry.path.ends_with(".hidden"))
    );
    model.toggle_hidden();
    assert!(
        model
            .tree
            .entries
            .iter()
            .any(|entry| entry.path.ends_with(".hidden"))
    );
}

#[test]
fn tree_creates_and_renames_files_and_directories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace.clone();
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
    model.workspace.path = workspace.clone();
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
    model.workspace.path = workspace.clone();
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
    model.workspace.path = dir.path().to_path_buf();
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
fn tree_shows_modified_and_diagnostic_markers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "fn main() {}\n").expect("file");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = dir.path().to_path_buf();
    model.welcome = false;
    model.open_tree();
    model.open_path(&path).expect("open");
    model.insert_text("x");
    model
        .language
        .diagnostics
        .by_path
        .insert(path.clone(), vec![diagnostic(0, 0, "broken")]);
    model.git.status.insert(path.clone(), 'M');

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
    assert!(rendered.contains("main.rs * M"), "{rendered}");
    assert!(rendered.contains("main.rs * M !"), "{rendered}");
}

#[test]
fn tree_renders_ascii_icon_fallback() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("main.rs"), "").expect("file");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = dir.path().to_path_buf();
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
    model.workspace.path = dir.path().to_path_buf();
    model.welcome = false;
    model.open_tree();
    model.tree.selected = Some(19);

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
    model.overlays.fuzzy = Some(FuzzyState {
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
    model.overlays.fuzzy = Some(FuzzyState {
        generation: 0,
        query: String::new(),
        entries: vec!["src/main.rs".to_owned()],
        filtered: vec![0],
        selected: 0,
    });
    model.overlays.preview = PreviewState {
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
    model.overlays.fuzzy = Some(FuzzyState {
        generation: 0,
        query: String::new(),
        entries: vec!["src/main.rs".to_owned()],
        filtered: vec![0],
        selected: 0,
    });
    model.overlays.preview = PreviewState {
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
        .language
        .diagnostics
        .by_path
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
    model.workspace.path = PathBuf::from("/ws");
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

#[test]
fn tree_rolls_up_child_git_and_diagnostics_to_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let directory = dir.path().join("src");
    let file = directory.join("main.rs");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.welcome = false;
    model.tree.visible = true;
    model.tree.entries = vec![
        TreeEntry {
            path: directory.clone(),
            depth: 0,
            is_dir: true,
            ino: 0,
            expanded: true,
        },
        TreeEntry {
            path: file.clone(),
            depth: 1,
            is_dir: false,
            ino: 0,
            expanded: false,
        },
    ];
    model.tree.selected = Some(0);
    model.git.status.insert(file.clone(), 'M');
    model
        .language
        .diagnostics
        .by_path
        .insert(file, vec![diagnostic(0, 0, "broken")]);

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
    assert!(rendered.contains("src M !"), "{rendered}");
}

#[test]
fn debug_panel_renders_threads_stack_variables_and_watches() {
    use crate::debug_panel::{
        DebugFrame, DebugPanelKind, DebugPanelState, DebugThread, DebugVariable, DebugWatch,
    };
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    let mut panel = DebugPanelState {
        threads: vec![DebugThread {
            id: 1,
            name: "main".to_owned(),
        }],
        frames: vec![DebugFrame {
            id: 10,
            name: "run".to_owned(),
            path: Some(PathBuf::from("src/main.rs")),
            line: 7,
            column: 3,
        }],
        variables: vec![DebugVariable {
            name: "x".to_owned(),
            value: "42".to_owned(),
            variables_reference: 0,
        }],
        watches: vec![DebugWatch {
            expression: "count".to_owned(),
            value: "3".to_owned(),
        }],
        ..DebugPanelState::default()
    };
    for (kind, expected) in [
        (DebugPanelKind::Threads, "Debug · Threads"),
        (DebugPanelKind::Stack, "Debug · Stack"),
        (DebugPanelKind::Variables, "Debug · Variables"),
        (DebugPanelKind::Watch, "Debug · Watch"),
    ] {
        panel.kind = kind;
        model.overlays.debug_panel = Some(panel.clone());
        let backend = TestBackend::new(80, 16);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| view(&model, frame)).expect("draw");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains(expected), "{kind:?}: {rendered}");
        assert!(rendered.contains("r refresh"), "{kind:?}: {rendered}");
        assert!(
            rendered.contains("main")
                || rendered.contains("run")
                || rendered.contains("x = 42")
                || rendered.contains("count = 3"),
            "{kind:?}: {rendered}"
        );
    }
}

#[test]
fn debug_panel_keys_cycle_and_close() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.execute_command("debug");
    assert!(model.overlays.debug_panel.is_some());
    handle_event(&mut model, key(KeyCode::Tab));
    assert_eq!(
        model.overlays.debug_panel.as_ref().map(|panel| panel.kind),
        Some(crate::debug_panel::DebugPanelKind::Stack)
    );
    handle_event(&mut model, key(KeyCode::Esc));
    assert!(model.overlays.debug_panel.is_none());
}

#[test]
fn debug_watch_command_adds_an_expression() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.execute_command("debug");
    model.execute_command("debug-watch count");
    let panel = model.overlays.debug_panel.as_ref().expect("debug panel");
    assert_eq!(panel.kind, crate::debug_panel::DebugPanelKind::Watch);
    assert_eq!(panel.watches[0].expression, "count");
}

#[test]
fn breadcrumb_row_routes_path_clicks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    fs::write(workspace.join("main.rs"), "alpha").expect("source");
    let path = workspace.join("main.rs");
    let mut model = Model::new(EditorState::from_text("alpha"), Some(path.clone()));
    model.workspace.path = workspace;
    assert_eq!(
        crate::mouse::hit_test(&model, 1, 1, 80, 24),
        crate::mouse::Target::Breadcrumb(crate::breadcrumbs::BreadcrumbTarget::Path)
    );
    let click = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    assert!(crate::mouse::handle_mouse(&mut model, click, 80, 24));
    assert!(model.tree.visible);
    assert_eq!(
        model
            .tree
            .entries
            .get(model.tree.selected.unwrap_or(0))
            .map(|entry| &entry.path),
        Some(&path)
    );
}

#[test]
fn recent_files_and_workspaces_are_bounded_and_deduplicated() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.recent.persist = false;
    model.record_recent_workspace(Path::new("/ws/one"));
    model.record_recent_workspace(Path::new("/ws/two"));
    model.record_recent_workspace(Path::new("/ws/one"));
    assert_eq!(
        model.recent.workspaces,
        vec![PathBuf::from("/ws/one"), PathBuf::from("/ws/two")]
    );
    let workspace = tempfile::tempdir().expect("workspace");
    model.workspace.path = workspace.path().to_path_buf();
    model.workspace.roots =
        crate::workspace_roots::WorkspaceRoots::new(&model.workspace.path).expect("roots");
    for index in 0..25 {
        model.record_recent_file(&model.workspace.path.join(format!("{index}.rs")));
    }
    model.record_recent_file(Path::new("/outside/ignored.rs"));
    assert_eq!(model.recent.files.len(), 20);
    assert_eq!(model.recent.files[0], PathBuf::from("/outside/ignored.rs"));
    model.open_recent_files();
    let entries = &model.overlays.fuzzy.as_ref().expect("fuzzy").entries;
    assert_eq!(entries.len(), 19);
    assert!(entries.iter().all(|entry| !entry.starts_with('/')));
}

#[test]
fn reduced_motion_uses_a_static_busy_glyph() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.settings.ui.reduced_motion = true;
    model.busy = Some("indexing".to_owned());
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
    assert!(rendered.contains("· indexing"), "{rendered}");
    assert!(!rendered.contains('⠋'), "{rendered}");
}

#[test]
fn welcome_shows_recent_files_and_r_opens_them() {
    let workspace = tempfile::tempdir().expect("workspace");
    fs::write(workspace.path().join("main.rs"), "fn main() {}").expect("file");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace.path().to_path_buf();
    model.workspace.roots =
        crate::workspace_roots::WorkspaceRoots::new(&model.workspace.path).expect("roots");
    model.record_recent_file(&workspace.path().join("main.rs"));
    let backend = TestBackend::new(80, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("recent"), "{rendered}");
    assert!(rendered.contains("main.rs"), "{rendered}");
    assert!(rendered.contains("recent files"), "{rendered}");

    handle_event(&mut model, key(KeyCode::Char('r')));
    assert!(!model.welcome);
    assert!(model.overlays.fuzzy.is_some());
}

#[test]
fn welcome_recent_rows_are_clickable() {
    let workspace = tempfile::tempdir().expect("workspace");
    let path = workspace.path().join("main.rs");
    fs::write(&path, "fn main() {}").expect("file");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace.path().to_path_buf();
    model.workspace.roots =
        crate::workspace_roots::WorkspaceRoots::new(&model.workspace.path).expect("roots");
    model.record_recent_file(&path);
    assert_eq!(
        crate::mouse::hit_test(&model, 35, 5, 80, 30),
        crate::mouse::Target::WelcomeRecent(0)
    );
    let click = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 35,
        row: 5,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    assert!(crate::mouse::handle_mouse(&mut model, click, 80, 30));
    assert!(!model.welcome);
    assert_eq!(model.path.as_deref(), Some(path.as_path()));
}

#[test]
fn settings_page_renders_tabs_sections_values_and_description() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.open_settings();
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("Editor"), "{rendered}");
    assert!(rendered.contains("Interface"), "{rendered}");
    assert!(rendered.contains("Language"), "{rendered}");
    assert!(rendered.contains("Sections"), "{rendered}");
    assert!(rendered.contains("scroll_margin"), "{rendered}");
    assert!(
        rendered.contains("Lines kept visible around the cursor"),
        "{rendered}"
    );
}

#[test]
fn command_palette_renders_title_detail_and_footer() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.open_command_palette();
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("Commands ·"), "{rendered}");
    assert!(rendered.contains("Enter run"), "{rendered}");
    assert!(rendered.contains("normal"), "{rendered}");
}

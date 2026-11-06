use super::*;
use crate::file_worker::PreviewData;
use crate::input::KeyPress;
use crate::keymap::{KeyChord, KeymapConfig, NormalCommand, PendingCommand};
use anvil_session::ui::CreateKind;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
}

fn alt(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::ALT))
}

fn repeat(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new_with_kind(
        code,
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    ))
}

fn release(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new_with_kind(
        code,
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ))
}

fn type_text(model: &mut Model, text: &str) {
    for character in text.chars() {
        handle_event(model, key(KeyCode::Char(character)));
    }
}

fn command(model: &mut Model, text: &str) {
    handle_event(model, key(KeyCode::Char(':')));
    type_text(model, text);
    handle_event(model, key(KeyCode::Enter));
}

fn diagnostic(line: u32, character: u32, message: &str) -> crate::lsp::Diagnostic {
    crate::lsp::Diagnostic {
        range: crate::lsp::Range {
            start: crate::lsp::Position { line, character },
            end: crate::lsp::Position {
                line,
                character: character + 1,
            },
        },
        severity: Some(1),
        message: message.to_owned(),
        source: Some("test".to_owned()),
        extra: serde_json::Map::new(),
    }
}

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
    assert!(rendered.contains("R src/main.rs"), "{rendered}");
    assert!(rendered.contains("Preview"), "{rendered}");
    assert!(rendered.contains("fn main() {}"), "{rendered}");
}

#[test]
fn fuzzy_preview_stacks_below_list_on_narrow_terminals() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.welcome = false;
    model.fuzzy = Some(FuzzyState {
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
fn space_leader_focuses_editor_and_tree() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.welcome = false;
    model.open_tree();
    assert!(model.ui.focus.tree);
    for key_code in [KeyCode::Char(' '), KeyCode::Char('1')] {
        handle_event(&mut model, key(key_code));
    }
    assert!(!model.ui.focus.tree);
    assert!(model.tree_visible);
    for key_code in [KeyCode::Char(' '), KeyCode::Char('2')] {
        handle_event(&mut model, key(key_code));
    }
    assert!(model.tree_visible);
    assert!(model.ui.focus.tree);
}

#[test]
fn space_leader_expires_without_consuming_next_key() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta"), None);
    handle_event(&mut model, key(KeyCode::Char(' ')));
    assert_eq!(model.pending, Some(' '));
    model.leader_deadline = Some(std::time::Instant::now());
    model.expire_leader();
    assert_eq!(model.pending, None);
    handle_event(&mut model, key(KeyCode::Char('j')));
    assert_eq!(model.cursor, 6);
}

#[test]
fn popup_consumes_j_before_editor_motion() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta"), None);
    model.welcome = false;
    model.document_symbols = Some(SymbolState {
        items: vec![
            SymbolItem {
                name: "alpha".to_owned(),
                detail: None,
                line: 0,
                character: 0,
                depth: 0,
            },
            SymbolItem {
                name: "beta".to_owned(),
                detail: None,
                line: 1,
                character: 0,
                depth: 0,
            },
        ],
        selected: 0,
    });
    handle_event(&mut model, key(KeyCode::Char('j')));
    assert_eq!(model.cursor, 0);
    assert_eq!(
        model
            .document_symbols
            .as_ref()
            .map(|symbols| symbols.selected),
        Some(1)
    );
}

#[test]
fn pending_space_renders_keymap_continuations() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.welcome = false;
    model.begin_pending(' ');
    let backend = TestBackend::new(100, 20);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("picker.fuzzy"), "{rendered}");
    assert!(rendered.contains("view.focus_editor"), "{rendered}");
}

#[test]
fn pending_operator_which_key_shows_text_object_extenders() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.welcome = false;
    model.begin_pending('d');
    handle_event(&mut model, key(KeyCode::Char('i')));
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("w "), "{rendered}");
    assert!(rendered.contains("delete inner word"), "{rendered}");
}

#[test]
fn keymap_context_prefers_popup_then_command_line_then_tree() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.welcome = false;
    model.ui.focus.tree = true;
    assert_eq!(
        model.active_keymap_contexts(),
        vec![KeymapContext::Global, KeymapContext::FileTree]
    );

    model.fuzzy = Some(FuzzyState {
        query: String::new(),
        entries: vec!["a".to_owned()],
        filtered: vec![0],
        selected: 0,
    });
    assert_eq!(
        model.active_keymap_contexts(),
        vec![
            KeymapContext::Global,
            KeymapContext::FileTree,
            KeymapContext::Popup
        ]
    );

    model.fuzzy = None;
    model.ui.input_mode = InputMode::CommandLine {
        buffer: String::new(),
    };
    assert_eq!(
        model.active_keymap_contexts(),
        vec![
            KeymapContext::Global,
            KeymapContext::FileTree,
            KeymapContext::CommandLine
        ]
    );

    model.ui.input_mode = InputMode::Normal;
    model.visual_anchor = Some(0);
    assert_eq!(
        model.active_keymap_contexts(),
        vec![
            KeymapContext::Global,
            KeymapContext::FileTree,
            KeymapContext::Modal
        ]
    );
}

#[test]
fn file_tree_context_bindings_dispatch_before_hardcoded_keys() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.welcome = false;
    model.open_tree();
    assert!(model.ui.focus.tree);
    model.keymap = crate::keymap::KeymapConfig::from_toml(
        r#"
[keymap.contexts.file_tree]
x = "view.focus_editor"
"#,
    )
    .expect("context keymap");
    handle_event(&mut model, key(KeyCode::Char('x')));
    assert!(!model.ui.focus.tree);
}

#[test]
fn popup_context_bindings_dispatch_before_popup_keys() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.welcome = false;
    model.open_tree();
    model.document_symbols = Some(SymbolState {
        items: vec![SymbolItem {
            name: "alpha".to_owned(),
            detail: None,
            line: 0,
            character: 0,
            depth: 0,
        }],
        selected: 0,
    });
    model.keymap = crate::keymap::KeymapConfig::from_toml(
        r#"
[keymap.contexts.popup]
x = "view.focus_editor"
"#,
    )
    .expect("context keymap");
    handle_event(&mut model, key(KeyCode::Char('x')));
    assert!(!model.ui.focus.tree);
    assert!(model.document_symbols.is_some());
}

#[test]
fn global_context_does_not_shadow_open_popup() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta"), None);
    model.welcome = false;
    model.document_symbols = Some(SymbolState {
        items: vec![
            SymbolItem {
                name: "alpha".to_owned(),
                detail: None,
                line: 0,
                character: 0,
                depth: 0,
            },
            SymbolItem {
                name: "beta".to_owned(),
                detail: None,
                line: 1,
                character: 0,
                depth: 0,
            },
        ],
        selected: 0,
    });
    model.keymap = crate::keymap::KeymapConfig::from_toml(
        r#"
[keymap.contexts.global]
j = "editor.undo"
"#,
    )
    .expect("global keymap");
    handle_event(&mut model, key(KeyCode::Char('j')));
    assert_eq!(model.cursor, 0);
    assert_eq!(
        model
            .document_symbols
            .as_ref()
            .map(|symbols| symbols.selected),
        Some(1)
    );
}

#[test]
fn command_line_context_is_explicit_without_global_fallback() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.welcome = false;
    model.open_tree();
    model.keymap = crate::keymap::KeymapConfig::from_toml(
        r#"
[keymap.contexts.global]
a = "editor.undo"

[keymap.contexts.command_line]
"ctrl-x" = "view.focus_editor"
"#,
    )
    .expect("context keymap");
    model.ui.input_mode = InputMode::CommandLine {
        buffer: String::new(),
    };
    handle_event(&mut model, ctrl(KeyCode::Char('x')));
    assert!(!model.ui.focus.tree);
    handle_event(&mut model, key(KeyCode::Char('a')));
    assert_eq!(
        match &model.ui.input_mode {
            InputMode::CommandLine { buffer } => buffer.as_str(),
            _ => "",
        },
        "a"
    );
}

#[test]
fn command_palette_filters_and_executes_registry_commands() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.welcome = false;
    model.open_tree();
    assert!(model.ui.focus.tree);
    for key_code in [KeyCode::Char(' '), KeyCode::Char('p')] {
        handle_event(&mut model, key(key_code));
    }
    assert!(model.command_palette.is_some());
    for character in "focus_editor".chars() {
        handle_event(&mut model, key(KeyCode::Char(character)));
    }
    let backend = TestBackend::new(120, 20);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("View Focus Editor"), "{rendered}");
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(!model.ui.focus.tree);
    assert!(model.command_palette.is_none());
    model.open_command_palette();
    assert_eq!(
        model.selected_command_id().as_deref(),
        Some("view.focus_editor")
    );
}

#[test]

fn search_and_rename_contexts_consume_explicit_bindings() {
    let keymap = crate::keymap::KeymapConfig::from_toml(
        r#"
[keymap.contexts.search]
"ctrl-x" = "view.focus_editor"

[keymap.contexts.rename]
"ctrl-y" = "view.focus_editor"
"#,
    )
    .expect("context keymap");

    let mut search = Model::new(EditorState::from_text("alpha"), None);
    search.welcome = false;
    search.open_tree();
    search.keymap = keymap;
    search.ui.input_mode = InputMode::Search {
        query: String::new(),
    };
    handle_event(&mut search, ctrl(KeyCode::Char('x')));
    assert!(!search.ui.focus.tree);
    assert!(matches!(search.ui.input_mode, InputMode::Search { .. }));

    let mut rename = Model::new(EditorState::from_text("alpha"), None);
    rename.welcome = false;
    rename.open_tree();
    rename.keymap = crate::keymap::KeymapConfig::from_toml(
        r#"
[keymap.contexts.rename]
"ctrl-y" = "view.focus_editor"
"#,
    )
    .expect("rename keymap");
    rename.ui.input_mode = InputMode::Rename {
        name: "alpha".to_owned(),
    };
    handle_event(&mut rename, ctrl(KeyCode::Char('y')));
    assert!(!rename.ui.focus.tree);
    assert!(matches!(rename.ui.input_mode, InputMode::Rename { .. }));
}

#[test]
fn tree_expands_directories() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir(dir.path().join("src")).expect("dir");
    fs::write(dir.path().join("src/main.rs"), "fn main() {}").expect("file");
    fs::write(dir.path().join("Cargo.toml"), "").expect("manifest");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace = dir.path().to_path_buf();
    model.open_tree();
    assert!(model.tree_visible);
    let index = model
        .tree
        .iter()
        .position(|entry| entry.path.ends_with("src"))
        .expect("src");
    model.tree_selected = Some(index);
    model.tree_toggle();
    assert!(
        model
            .tree
            .iter()
            .any(|entry| entry.path.ends_with("src/main.rs"))
    );
    model.tree_toggle();
    assert!(
        !model
            .tree
            .iter()
            .any(|entry| entry.path.ends_with("src/main.rs"))
    );
}

#[test]
fn tree_refresh_preserves_selection_and_expansion() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir(dir.path().join("src")).expect("dir");
    fs::write(dir.path().join("src/main.rs"), "fn main() {}").expect("file");
    fs::write(dir.path().join("root.txt"), "").expect("file");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace = dir.path().to_path_buf();
    model.open_tree();
    let src = model
        .tree
        .iter()
        .position(|entry| entry.path.ends_with("src"))
        .expect("src");
    model.tree_selected = Some(src);
    model.tree_toggle();
    let main = model
        .tree
        .iter()
        .position(|entry| entry.path.ends_with("src/main.rs"))
        .expect("main");
    model.tree_selected = Some(main);
    fs::write(dir.path().join("src/added.rs"), "").expect("new file");

    model.refresh_tree(None);
    assert!(
        model
            .tree
            .iter()
            .any(|entry| entry.path.ends_with("src/added.rs"))
    );
    assert!(
        model
            .tree
            .iter()
            .any(|entry| entry.path.ends_with("src") && entry.expanded)
    );
    assert_eq!(
        model.tree[model.tree_selected.expect("selection")].path,
        dir.path().join("src/main.rs")
    );

    let renamed = dir.path().join("src/renamed.rs");
    fs::rename(dir.path().join("src/main.rs"), &renamed).expect("rename");
    model.refresh_tree(None);
    assert!(model.tree.iter().any(|entry| entry.path == renamed));
    assert_eq!(
        model.tree[model.tree_selected.expect("rename fallback")].path,
        dir.path().join("src")
    );

    fs::remove_file(renamed).expect("remove");
    model.refresh_tree(None);
    assert_eq!(
        model.tree[model.tree_selected.expect("delete fallback")].path,
        dir.path().join("src")
    );
}

#[test]
fn fuzzy_filters_workspace_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir(dir.path().join("src")).expect("dir");
    fs::write(dir.path().join("src/main.rs"), "fn main() {}").expect("file");
    fs::write(dir.path().join("src/lib.rs"), "").expect("file");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace = dir.path().to_path_buf();
    model.start_fuzzy();
    let fuzzy = model.fuzzy.as_mut().expect("fuzzy");
    fuzzy.query = "main".to_owned();
    model.fuzzy_refresh();
    assert_eq!(model.fuzzy.as_ref().expect("fuzzy").filtered.len(), 1);
}
#[test]
fn stale_preview_results_are_discarded() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.preview_generation = 2;
    model.preview = PreviewState {
        path: Some("src/main.rs".to_owned()),
        loading: true,
        ..PreviewState::default()
    };

    model.handle_preview(
        "src/main.rs".to_owned(),
        1,
        Ok(PreviewData {
            lines: vec!["stale".to_owned()],
            truncated: false,
        }),
    );
    assert!(model.preview.loading);
    assert!(model.preview.lines.is_empty());

    model.handle_preview(
        "src/main.rs".to_owned(),
        2,
        Ok(PreviewData {
            lines: vec!["fresh".to_owned()],
            truncated: true,
        }),
    );
    assert!(!model.preview.loading);
    assert_eq!(model.preview.lines, ["fresh"]);
    assert!(model.preview.truncated);
}

#[test]
fn preview_failure_keeps_the_list_usable() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.preview_generation = 1;
    model.preview = PreviewState {
        path: Some("src/main.rs".to_owned()),
        loading: true,
        ..PreviewState::default()
    };
    model.handle_preview(
        "src/main.rs".to_owned(),
        1,
        Err("gateway failed".to_owned()),
    );
    assert!(!model.preview.loading);
    assert_eq!(model.preview.message.as_deref(), Some("gateway failed"));
    assert!(model.status.contains("E: preview: gateway failed"));
}

#[test]
fn preview_without_worker_reports_gateway_unavailable() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.fuzzy = Some(FuzzyState {
        query: String::new(),
        entries: vec!["src/main.rs".to_owned()],
        filtered: vec![0],
        selected: 0,
    });
    model.request_preview();
    assert!(!model.preview.loading);
    assert_eq!(
        model.preview.message.as_deref(),
        Some("preview unavailable: gateway")
    );
    assert!(model.status.contains("E: preview: gateway unavailable"));
}

#[test]
fn lsp_settings_cover_builtin_languages() {
    for (path, language) in [
        ("main.rs", "rust"),
        ("main.go", "go"),
        ("main.py", "python"),
        ("main.ts", "typescript"),
        ("main.tsx", "typescriptreact"),
        ("main.js", "javascript"),
        ("main.jsx", "javascriptreact"),
        ("main.c", "c"),
        ("main.cpp", "cpp"),
        ("main.h", "c"),
        ("main.java", "java"),
        ("main.lua", "lua"),
        ("main.json", "json"),
        ("main.jsonc", "jsonc"),
        ("main.yaml", "yaml"),
        ("main.yml", "yaml"),
        ("main.sh", "shellscript"),
        ("README.md", "markdown"),
        ("Cargo.toml", "toml"),
        ("index.html", "html"),
        ("style.css", "css"),
        ("style.scss", "scss"),
        ("main.zig", "zig"),
    ] {
        assert_eq!(
            lsp_settings(Path::new(path))
                .unwrap_or_else(|| panic!("missing settings for {path}"))
                .language_id,
            language,
            "{path}"
        );
    }
    assert_eq!(
        lsp_settings(Path::new("main.ts")).expect("typescript").args,
        vec!["--stdio"]
    );
    assert_eq!(
        lsp_settings(Path::new("main.sh")).expect("shell").args,
        vec!["start"]
    );
    assert_eq!(
        lsp_settings(Path::new("README.md")).expect("markdown").args,
        vec!["server"]
    );
    assert_eq!(
        lsp_settings(Path::new("Cargo.toml")).expect("toml").args,
        vec!["lsp", "stdio"]
    );
    assert!(lsp_settings(Path::new("README.txt")).is_none());
}

#[test]
fn renders_two_split_panes() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta\ngamma"), None);
    model.split_vertical();
    let backend = TestBackend::new(60, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let buffer = terminal.backend().buffer();
    let rendered = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.matches("alpha").count() >= 2, "{rendered}");
}

#[test]
fn split_panes_hold_different_buffers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let left = dir.path().join("left.txt");
    let right = dir.path().join("right.txt");
    std::fs::write(&left, "left text").expect("write left");
    std::fs::write(&right, "right text").expect("write right");
    let mut model = Model::new(EditorState::from_text("scratch"), None);
    model.open_path(&left).expect("open left");
    model.split_vertical();
    model.focus_split(true);
    model.open_path(&right).expect("open right");
    assert_eq!(
        model
            .editor
            .text()
            .slice(0..model.editor.text().len_bytes()),
        "right text"
    );
    model.focus_split(false);
    assert_eq!(
        model
            .editor
            .text()
            .slice(0..model.editor.text().len_bytes()),
        "left text"
    );
    model.focus_split(true);
    assert_eq!(
        model
            .editor
            .text()
            .slice(0..model.editor.text().len_bytes()),
        "right text"
    );
}

#[test]
fn split_focus_restores_buffer_state_and_active_tab() {
    let dir = tempfile::tempdir().expect("tempdir");
    let left = dir.path().join("left.txt");
    let right = dir.path().join("right.txt");
    fs::write(&left, "left").expect("left");
    fs::write(&right, "right").expect("right");
    let mut model = Model::open(&left).expect("open left");
    model.split_vertical();
    model.focus_split(true);
    model.open_path(&right).expect("open right");
    model.mark_missing(&right);
    handle_event(&mut model, key(KeyCode::Char('v')));
    handle_event(&mut model, key(KeyCode::Right));
    assert_eq!(model.active, 1);

    model.focus_split(false);
    assert_eq!(model.active, 0);
    assert!(model.visual_anchor.is_none());
    assert!(!model.missing);

    model.focus_split(true);
    assert_eq!(model.active, 1);
    assert_eq!(model.visual_anchor, Some(0));
    assert!(model.missing);
}

#[test]
fn split_focus_resets_stale_search_matches() {
    let mut model = Model::new(EditorState::from_text("foo"), None);
    model.commit_search("foo".to_owned());
    assert_eq!(model.search.matches.len(), 1);
    model.split_vertical();
    model.focus_split(true);
    assert!(model.search.matches.is_empty());
}
#[test]
fn closing_focused_split_restores_left_pane() {
    let mut model = Model::new(EditorState::from_text("left\nright"), None);
    model.split_vertical();
    model.focus_split(true);
    model.cursor = 5;
    model.close_split();
    assert!(model.split.is_none());
    assert!(!model.ui.focus.split);
    assert_eq!(model.cursor, 0);
    assert_eq!(model.editor.text().slice(0..4), "left");
}

#[test]
fn ctrl_w_v_opens_split_and_ctrl_w_h_l_switches_focus() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta\ngamma"), None);
    handle_event(&mut model, ctrl(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Char('v')));
    assert!(model.split.is_some());
    handle_event(&mut model, ctrl(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Char('l')));
    assert!(model.ui.focus.split);
    handle_event(&mut model, ctrl(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Char('h')));
    assert!(!model.ui.focus.split);
}

#[test]
fn split_panes_keep_independent_cursors() {
    let mut model = Model::new(EditorState::from_text("one\ntwo\nthree"), None);
    model.cursor = 1;
    model.split_vertical();
    assert!(model.split.is_some());
    assert!(!model.ui.focus.split);
    model.focus_split(true);
    assert!(model.ui.focus.split);
    assert_eq!(model.cursor, 1);
    model.cursor = 5;
    model.scroll = 2;
    model.hscroll = 3;
    model.focus_split(false);
    assert!(!model.ui.focus.split);
    assert_eq!(model.cursor, 1);
    assert_eq!(model.scroll, 0);
    assert_eq!(model.hscroll, 0);
    model.focus_split(true);
    assert_eq!(model.cursor, 5);
    assert_eq!(model.scroll, 2);
    assert_eq!(model.hscroll, 3);
}

#[test]
fn select_word_occurrences_replaces_all_as_one_undo() {
    let original = "foo\n".repeat(100);
    let mut model = Model::new(
        EditorState::from_text(&original),
        Some(PathBuf::from("multi.txt")),
    );
    model.select_word_occurrences();
    assert_eq!(model.editor.selection_bytes().len(), 100);
    model.insert_text("bar");
    assert_eq!(model.editor.text().slice(0..400), "bar\n".repeat(100));
    assert_eq!(model.editor.undo_depth(), 1);
    model.undo();
    assert_eq!(model.editor.text().slice(0..400), original);
    assert_eq!(model.editor.selection_bytes().len(), 100);
}

#[test]
fn split_selection_lines_creates_one_caret_per_line() {
    let mut model = Model::new(EditorState::from_text("one\ntwo\nthree"), None);
    assert!(model.editor.set_selection_bytes(0, 7));
    model.split_selection_lines();
    assert_eq!(model.editor.selection_bytes().len(), 2);
}

#[test]
fn add_cursor_after_motion_uses_the_moved_caret() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta\ngamma"), None);
    handle_event(&mut model, key(KeyCode::Char('j')));
    handle_event(&mut model, key(KeyCode::Char('C')));
    assert_eq!(model.editor.selection_bytes(), vec![(6, 6), (11, 11)]);

    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "X");
    let len = model.editor.text().len_bytes();
    assert_eq!(model.editor.text().slice(0..len), "alpha\nXbeta\nXgamma");
}

#[test]
fn escape_merges_cursors_only_in_normal_mode() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta\ngamma"), None);
    handle_event(&mut model, key(KeyCode::Char('j')));
    handle_event(&mut model, key(KeyCode::Char('C')));
    assert_eq!(model.editor.selection_bytes().len(), 2);

    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.editor.selection_bytes(), vec![(6, 6)]);

    handle_event(&mut model, key(KeyCode::Char('C')));
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.editor.selection_bytes().len(), 2);
    assert!(matches!(model.ui.input_mode, InputMode::Normal));

    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.editor.selection_bytes(), vec![(6, 6)]);
    assert_eq!(model.cursor, 6);
    let len = model.editor.text().len_bytes();
    assert_eq!(model.editor.text().slice(0..len), "alpha\nbeta\ngamma");
}

#[test]
fn alt_c_adds_a_cursor_above_for_both_keycodes() {
    for code in [KeyCode::Char('c'), KeyCode::Char('C')] {
        let mut model = Model::new(EditorState::from_text("alpha\nbeta\ngamma"), None);
        handle_event(&mut model, key(KeyCode::Char('j')));
        handle_event(&mut model, alt(code));
        assert_eq!(model.editor.selection_bytes(), vec![(0, 0), (6, 6)]);
    }
}

#[test]
fn alt_s_is_case_insensitive() {
    for code in [KeyCode::Char('s'), KeyCode::Char('S')] {
        let mut model = Model::new(EditorState::from_text("one\ntwo\nthree"), None);
        assert!(model.editor.set_selection_bytes(0, 7));
        handle_event(&mut model, alt(code));
        assert_eq!(model.editor.selection_bytes().len(), 2);
    }
}

#[test]
fn pending_commands_ignore_alt_modifiers() {
    let mut model = Model::new(EditorState::from_text("one two"), None);
    handle_event(&mut model, key(KeyCode::Char('d')));
    handle_event(&mut model, alt(KeyCode::Char('w')));
    assert_eq!(model.editor.text().slice(0..7), "one two");
    assert!(model.pending.is_none());
}

#[test]
fn insert_ignores_alt_modified_characters() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, alt(KeyCode::Char('j')));
    assert_eq!(model.editor.text().slice(0..3), "abc");
    assert!(matches!(model.ui.input_mode, InputMode::Insert));
}

#[test]
fn normal_mode_ignores_alt_commands() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    handle_event(&mut model, alt(KeyCode::Char('i')));
    handle_event(&mut model, alt(KeyCode::Char('d')));
    assert!(matches!(model.ui.input_mode, InputMode::Normal));
    assert!(model.pending.is_none());
}

#[test]
fn repeated_key_events_still_dispatch() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    handle_event(&mut model, repeat(KeyCode::Char('l')));
    assert_eq!(model.cursor, 1);
}

#[test]
fn released_key_events_are_ignored() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    handle_event(&mut model, release(KeyCode::Char('l')));
    assert_eq!(model.cursor, 0);
}

#[test]
fn normal_tab_does_not_insert_text() {
    let mut model = Model::new(EditorState::from_text("a"), None);
    handle_event(&mut model, key(KeyCode::Tab));
    handle_event(&mut model, key(KeyCode::BackTab));
    assert_eq!(model.editor.text().slice(0..1), "a");
    assert!(!model.modified);
}

#[test]
fn insert_tab_adds_four_spaces() {
    let mut model = Model::new(EditorState::from_text("a"), None);
    handle_event(&mut model, key(KeyCode::Char('a')));
    handle_event(&mut model, key(KeyCode::Tab));
    assert_eq!(model.editor.text().slice(0..5), "a    ");
}

#[test]
fn rendered_tabs_use_display_columns() {
    let model = Model::new(
        EditorState::from_text("a\tb"),
        Some(PathBuf::from("tabs.rs")),
    );
    let backend = TestBackend::new(20, 5);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(content.contains("a   b"), "{content:?}");
    assert!(!content.contains('\t'));
}

#[test]
fn insert_mode_modifies_piece_table() {
    let mut model = Model::new(EditorState::from_text("ac"), None);
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, key(KeyCode::Char('b')));
    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.editor.text().slice(0..3), "bac");
    assert!(model.modified);
    assert_eq!(model.ui.input_mode, InputMode::Normal);
}

#[test]
fn command_line_saves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("file.txt");
    fs::write(&path, "old").expect("write");
    let mut model = Model::open(&path).expect("open");
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, key(KeyCode::Char('n')));
    handle_event(&mut model, key(KeyCode::Esc));
    handle_event(&mut model, key(KeyCode::Char(':')));
    handle_event(&mut model, key(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(!model.modified);
    assert_eq!(fs::read_to_string(path).expect("read"), "nold");
}

#[test]
fn cursor_and_viewport_are_independent() {
    let mut model = Model::new(EditorState::from_text("a\nb\nc\nd"), None);
    model.move_line(3);
    model.ensure_cursor_visible(2, 80);
    assert_eq!(model.scroll, 2);
    model.move_line(-1);
    model.ensure_cursor_visible(2, 80);
    assert_eq!(model.scroll, 2);
}

#[test]
fn horizontal_scroll_follows_the_cursor() {
    let mut model = Model::new(EditorState::from_text("0123456789"), None);
    model.cursor = 8;
    model.ensure_cursor_visible(1, 4);
    assert_eq!(model.hscroll, 5);
    model.cursor = 0;
    model.ensure_cursor_visible(1, 4);
    assert_eq!(model.hscroll, 0);
}

#[test]
fn horizontal_scroll_uses_display_cells() {
    let mut model = Model::new(EditorState::from_text("中中中中中"), None);
    model.cursor = model.editor.text().len_bytes();
    model.ensure_cursor_visible(1, 3);
    assert_eq!(model.hscroll, 8);
}

#[test]
fn syntax_keyword_is_rendered() {
    let mut model = Model::new(
        EditorState::from_text("fn main() {}"),
        Some(PathBuf::from("sample.rs")),
    );
    for _ in 0..200 {
        model.poll_syntax();
        if !model.syntax.borrow().highlights().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        !model.syntax.borrow().highlights().is_empty(),
        "no syntax highlights produced"
    );
    let backend = TestBackend::new(80, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let colored = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .any(|cell| cell.fg == theme::SYNTAX_KEYWORD);
    assert!(colored, "keyword span was not colored");
    model.dirty = false;
}

#[test]
fn search_preserves_syntax_and_adds_background() {
    let mut model = Model::new(
        EditorState::from_text("fn main() {} fn"),
        Some(PathBuf::from("sample.rs")),
    );
    for _ in 0..200 {
        model.poll_syntax();
        if !model.syntax.borrow().highlights().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        !model.syntax.borrow().highlights().is_empty(),
        "no syntax highlights produced"
    );
    handle_event(&mut model, key(KeyCode::Char('/')));
    type_text(&mut model, "fn");
    handle_event(&mut model, key(KeyCode::Enter));
    let lines = model.visible_lines(8, 80);
    let keyword = lines[0]
        .spans
        .iter()
        .find(|span| span.content.as_ref() == "fn")
        .expect("keyword span");
    assert_eq!(keyword.style.fg, Some(theme::SYNTAX_KEYWORD));
    assert_eq!(keyword.style.bg, Some(theme::WARNING));
    let backgrounds = lines[0]
        .spans
        .iter()
        .filter(|span| span.content.as_ref() == "fn")
        .filter_map(|span| span.style.bg)
        .collect::<Vec<_>>();
    assert!(backgrounds.contains(&theme::WARNING));
    assert!(backgrounds.contains(&theme::SELECTION));
    let backend = TestBackend::new(80, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal.backend().buffer().content().iter().any(|cell| {
        cell.symbol() == "f" && cell.fg == theme::SYNTAX_KEYWORD && cell.bg == theme::WARNING
    });
    assert!(rendered, "search overlay replaced the syntax foreground");
}

#[test]
fn snippets_drop_placeholders() {
    assert_eq!(
        lsp_ui::expand_snippet("fn ${1:name}(${2:args}) $0"),
        "fn name(args) "
    );
}

#[test]
fn utf16_offset_maps_through_emoji() {
    let model = Model::new(EditorState::from_text("a😀b"), None);
    assert_eq!(model.offset_at_utf16(0, 1), 1);
    assert_eq!(model.offset_at_utf16(0, 3), 5);
}

#[test]
fn utf16_columns_map_through_emoji() {
    let text = "a😀b";
    assert_eq!(utf16_to_byte(text, 1), 1);
    assert_eq!(utf16_to_byte(text, 3), 5);
    assert_eq!(utf16_to_byte(text, 4), 6);
}

#[test]
fn navigation_preserves_modified_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "first").expect("first");
    fs::write(&second, "second").expect("second");
    let mut model = Model::open(&first).expect("open");
    model.insert_text("X");
    assert!(model.modified);
    model.open_path(&second).expect("second open");
    assert_eq!(
        model.path.as_deref(),
        Some(second.canonicalize().expect("canonical").as_path())
    );
    assert_eq!(model.editor.text().slice(0..6), "second");
    assert_eq!(model.buffers.len(), 2);
    assert!(model.buffers[0].modified);
    assert_eq!(model.buffers[0].editor.text().slice(0..6), "Xfirst");
    model.switch_buffer(-1);
    assert_eq!(model.editor.text().slice(0..6), "Xfirst");
    assert!(model.modified);
}

#[test]
fn definition_response_moves_cursor() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "fn foo() {}\nfn main() { foo(); }\n").expect("write");
    let mut model = Model::open(&path).expect("open");
    let uri = lsp_worker::file_uri(&path);
    model.apply_definition_response(&serde_json::json!([{
        "targetUri": uri,
        "targetSelectionRange": {
            "start": {"line": 1, "character": 3},
            "end": {"line": 1, "character": 7}
        }
    }]));
    assert_eq!(
        model.path.as_deref(),
        Some(path.canonicalize().expect("canonical").as_path())
    );
    assert_eq!(model.cursor, model.offset_at_utf16(1, 3));
}

#[test]
fn cancelling_completion_drops_pending_response() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    model.completion = Some(CompletionState {
        id: 9,
        items: vec![CompletionItem {
            label: "item".to_owned(),
            insert: "item".to_owned(),
            detail: String::new(),
            replace: None,
        }],
        filtered: vec![0],
        query: String::new(),
        selected: 0,
    });
    model.pending_lsp.insert(9, LspRequestKind::Completion);
    model.cancel_completion();
    assert!(model.completion.is_none());
    assert!(!model.pending_lsp.contains_key(&9));
    model.handle_lsp_response(9, serde_json::json!({"items": []}), None);
    assert!(model.completion.is_none());
    model.pending_lsp.insert(10, LspRequestKind::Completion);
    model.cancel_completion();
    assert!(!model.pending_lsp.contains_key(&10));
    model.handle_lsp_response(10, serde_json::json!({"items": []}), None);
    assert!(model.completion.is_none());
}

#[test]
fn rename_response_applies_workspace_edit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("rename.rs");
    fs::write(&file, "foo").expect("file");
    let mut model = Model::open(&file).expect("open");
    let uri = lsp_worker::file_uri(&file);
    model.pending_lsp.insert(7, LspRequestKind::Rename);
    model.handle_lsp_response(
        7,
        serde_json::json!({
            "changes": {
                uri: [{
                    "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}},
                    "newText": "bar"
                }]
            }
        }),
        None,
    );
    assert_eq!(model.editor.text().slice(0..3), "bar");
    assert!(model.modified);
}

#[test]
fn code_action_response_applies_workspace_edit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("action.rs");
    fs::write(&file, "bad").expect("file");
    let mut model = Model::open(&file).expect("open");
    let uri = lsp_worker::file_uri(&file);
    model.pending_lsp.insert(8, LspRequestKind::CodeAction);
    model.handle_lsp_response(
        8,
        serde_json::json!([{
            "title": "Fix it",
            "edit": {
                "changes": {
                    uri: [{
                        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}},
                        "newText": "good"
                    }]
                }
            }
        }]),
        None,
    );
    assert!(model.code_actions.is_some());
    model.apply_code_action();
    assert_eq!(model.editor.text().slice(0..4), "good");
    assert!(model.code_actions.is_none());
}

#[test]
fn cancelling_code_action_drops_pending_response() {
    let mut model = Model::new(EditorState::from_text("text"), Some(PathBuf::from("a.rs")));
    model.pending_lsp.insert(10, LspRequestKind::CodeAction);
    model.cancel_code_actions();
    assert!(!model.pending_lsp.contains_key(&10));
    model.handle_lsp_response(10, serde_json::json!([{"title": "late"}]), None);
    assert!(model.code_actions.is_none());
}

#[test]
fn gopls_code_action_response_applies_organize_imports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.go");
    fs::write(&file, "package main\n\nimport \"fmt\"\n\nfunc main() {}\n").expect("file");
    let mut model = Model::open(&file).expect("open");
    let uri = lsp_worker::file_uri(&file);
    model.pending_lsp.insert(9, LspRequestKind::CodeAction);
    model.handle_lsp_response(
        9,
        serde_json::json!([
            {
                "title": "Organize Imports",
                "kind": "source.organizeImports",
                "edit": {
                    "documentChanges": [{
                        "textDocument": {"version": 2, "uri": uri},
                        "edits": [{
                            "range": {"start": {"line": 1, "character": 0}, "end": {"line": 3, "character": 0}},
                            "newText": ""
                        }]
                    }]
                }
            },
            {
                "title": "Browse documentation",
                "kind": "source.doc",
                "command": {"title": "Browse documentation", "command": "gopls.doc"}
            }
        ]),
        None,
    );
    let actions = model.code_actions.as_ref().expect("actions");
    assert_eq!(actions.items.len(), 2);
    assert!(actions.items[0].edit.is_some());
    model.apply_code_action();
    assert!(
        !model
            .editor
            .text()
            .slice(0..model.editor.text().len_bytes())
            .contains("fmt")
    );
    assert!(model.modified);
}

#[test]
fn document_symbols_populate_and_jump_to_selection() {
    let file = PathBuf::from("symbols.rs");
    let mut model = Model::new(
        EditorState::from_text("fn alpha() {\n    fn inner() {}\n}\nfn beta() {}\n"),
        Some(file),
    );
    model.pending_lsp.insert(11, LspRequestKind::DocumentSymbol);
    model.handle_lsp_response(
        11,
        serde_json::json!([
            {
                "name": "alpha",
                "kind": 12,
                "detail": "fn()",
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 2, "character": 1}
                },
                "selectionRange": {
                    "start": {"line": 0, "character": 3},
                    "end": {"line": 0, "character": 8}
                },
                "children": [{
                    "name": "inner",
                    "kind": 12,
                    "range": {
                        "start": {"line": 1, "character": 4},
                        "end": {"line": 1, "character": 17}
                    },
                    "selectionRange": {
                        "start": {"line": 1, "character": 7},
                        "end": {"line": 1, "character": 12}
                    }
                }]
            },
            {
                "name": "beta",
                "kind": 12,
                "location": {
                    "uri": "file:///workspace/symbols.rs",
                    "range": {
                        "start": {"line": 3, "character": 3},
                        "end": {"line": 3, "character": 7}
                    }
                }
            }
        ]),
        None,
    );
    let symbols = model.document_symbols.as_ref().expect("symbols");
    assert_eq!(symbols.items.len(), 3);
    assert_eq!(symbols.items[1].depth, 1);
    assert_eq!(symbols.items[2].line, 3);
    model.apply_document_symbol();
    assert_eq!(model.cursor, 3);
    assert!(model.status.contains("alpha"));
}

#[test]
fn document_symbol_depth_is_bounded() {
    let mut value = serde_json::json!({
        "name": "leaf",
        "kind": 12,
        "selectionRange": {
            "start": {"line": 0, "character": 0},
            "end": {"line": 0, "character": 1}
        }
    });
    for _ in 0..100 {
        value = serde_json::json!({
            "name": "node",
            "kind": 12,
            "selectionRange": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 1}
            },
            "children": [value]
        });
    }
    let mut model = Model::new(
        EditorState::from_text("text"),
        Some(PathBuf::from("deep.rs")),
    );
    model.pending_lsp.insert(14, LspRequestKind::DocumentSymbol);
    model.handle_lsp_response(14, serde_json::json!([value]), None);
    let symbols = model.document_symbols.as_ref().expect("symbols");
    assert!(symbols.items.len() <= 65);
}

#[test]
fn formatting_response_applies_edits_without_writing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("format.rs");
    fs::write(&file, "fn main(){}").expect("file");
    let mut model = Model::open(&file).expect("open");
    model.pending_lsp.insert(12, LspRequestKind::Formatting);
    model.handle_lsp_response(
        12,
        serde_json::json!([{
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 0}
            },
            "newText": "//"
        }]),
        None,
    );
    assert_eq!(model.editor.text().slice(0..2), "//");
    assert_eq!(fs::read_to_string(&file).expect("disk"), "fn main(){}");
    assert!(model.save_all());
    assert_eq!(fs::read_to_string(&file).expect("saved"), "//fn main(){}");
}

#[test]
fn formatting_response_for_inactive_path_is_ignored() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "fn first(){}").expect("first");
    let mut model = Model::open(&first).expect("open");
    let first_path = model.path.clone().expect("first path");
    model.pending_edit_path.insert(13, first_path);
    model.pending_lsp.insert(13, LspRequestKind::Formatting);
    model
        .save_as(second.to_str().expect("second path"))
        .then_some(())
        .expect("save as");
    model.handle_lsp_response(
        13,
        serde_json::json!([{
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 0}
            },
            "newText": "//"
        }]),
        None,
    );
    assert_eq!(model.editor.text().slice(0..9), "fn first(");
    assert!(model.status.contains("inactive"));
}

#[test]
fn workspace_symbols_response_filters_and_jumps() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("symbols.rs");
    fs::write(&file, "fn alpha() {}\n").expect("file");
    let mut model = Model::open(&file).expect("open");
    let uri = lsp_worker::file_uri(&file);
    model
        .pending_lsp
        .insert(18, LspRequestKind::WorkspaceSymbol);
    model.handle_lsp_response(
        18,
        serde_json::json!([
            {
                "name": "alpha",
                "containerName": "symbols",
                "location": {
                    "uri": uri,
                    "range": {
                        "start": {"line": 0, "character": 3},
                        "end": {"line": 0, "character": 8}
                    }
                }
            },
            {
                "name": "beta",
                "containerName": "other",
                "location": {
                    "uri": uri,
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 2}
                    }
                }
            }
        ]),
        None,
    );
    let symbols = model.workspace_symbols.as_ref().expect("symbols");
    assert_eq!(symbols.items.len(), 2);
    model.filter_workspace_symbols();
    assert_eq!(
        model
            .workspace_symbols
            .as_ref()
            .expect("symbols")
            .filtered
            .len(),
        2
    );
    model.apply_workspace_symbol();
    assert_eq!(model.cursor, 3);
    assert!(model.status.contains("alpha"));
}

#[test]
fn signature_help_response_populates_active_signature() {
    let mut model = Model::new(
        EditorState::from_text("fn main() {}"),
        Some(PathBuf::from("sig.rs")),
    );
    model.pending_lsp.insert(17, LspRequestKind::SignatureHelp);
    model.handle_lsp_response(
        17,
        serde_json::json!({
            "signatures": [
                {"label": "fn foo(a: i32, b: &str)", "documentation": "Example"}
            ],
            "activeSignature": 0,
            "activeParameter": 1
        }),
        None,
    );
    let signature = model.signature_help.as_ref().expect("signature");
    assert_eq!(signature.active_parameter, 1);
    assert_eq!(signature.signatures.len(), 1);
    assert_eq!(signature.signatures[0].label, "fn foo(a: i32, b: &str)");
}

#[test]
fn stale_formatting_response_is_ignored() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("stale.rs");
    fs::write(&file, "fn main(){}").expect("file");
    let mut model = Model::open(&file).expect("open");
    model.pending_lsp.insert(15, LspRequestKind::Formatting);
    model
        .pending_edit_revision
        .insert(15, model.editor.revision());
    model.editor.insert(0, "x");
    model.handle_lsp_response(
        15,
        serde_json::json!([{
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 0}
            },
            "newText": "//"
        }]),
        None,
    );
    assert_eq!(model.editor.text().slice(0..3), "xfn");
    assert!(model.status.contains("stale"));
}

#[test]
fn rename_response_for_inactive_path_is_ignored() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "fn first(){}").expect("first");
    let mut model = Model::open(&first).expect("open");
    let first_path = model.path.clone().expect("first path");
    let uri = lsp_worker::file_uri(&first_path);
    model.pending_edit_path.insert(16, first_path);
    model
        .pending_edit_revision
        .insert(16, model.editor.revision());
    model.pending_lsp.insert(16, LspRequestKind::Rename);
    assert!(model.save_as(second.to_str().expect("second path")));
    model.handle_lsp_response(
        16,
        serde_json::json!({
            "changes": {
                uri: [{
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 2}
                    },
                    "newText": "xx"
                }]
            }
        }),
        None,
    );
    assert_eq!(model.editor.text().slice(0..9), "fn first(");
    assert!(model.status.contains("inactive"));
}

#[test]
fn diagnostic_jump_cycles_between_lines() {
    let mut model = Model::new(
        EditorState::from_text("one\ntwo\nthree"),
        Some(PathBuf::from("diag.rs")),
    );
    model.diagnostics.insert(
        PathBuf::from("diag.rs"),
        vec![
            lsp::Diagnostic {
                range: lsp::Range {
                    start: lsp::Position {
                        line: 0,
                        character: 0,
                    },
                    end: lsp::Position {
                        line: 0,
                        character: 1,
                    },
                },
                severity: Some(1),
                message: "first".to_owned(),
                source: Some("test".to_owned()),
                extra: serde_json::Map::new(),
            },
            lsp::Diagnostic {
                range: lsp::Range {
                    start: lsp::Position {
                        line: 2,
                        character: 0,
                    },
                    end: lsp::Position {
                        line: 2,
                        character: 1,
                    },
                },
                severity: Some(2),
                message: "second".to_owned(),
                source: Some("test".to_owned()),
                extra: serde_json::Map::new(),
            },
            lsp::Diagnostic {
                range: lsp::Range {
                    start: lsp::Position {
                        line: 0,
                        character: 2,
                    },
                    end: lsp::Position {
                        line: 0,
                        character: 3,
                    },
                },
                severity: None,
                message: "related".to_owned(),
                source: Some("test".to_owned()),
                extra: serde_json::Map::new(),
            },
        ],
    );
    model.jump_diagnostic(true);
    assert_eq!(model.cursor, model.offset_at_utf16(2, 0));
    assert!(
        model
            .ui
            .diagnostic_peek
            .as_ref()
            .is_some_and(|peek| peek.diagnostics[0].message.contains("second"))
    );
    model.jump_diagnostic(false);
    assert_eq!(model.cursor, 0);
    assert!(
        model
            .ui
            .diagnostic_peek
            .as_ref()
            .is_some_and(|peek| peek.diagnostics[0].message.contains("first"))
    );
    model.jump_diagnostic(false);
    assert_eq!(model.cursor, model.offset_at_utf16(2, 0));
}

#[test]
fn diagnostic_peek_merges_same_line_and_restores_focus() {
    let mut model = Model::new(
        EditorState::from_text("one\ntwo"),
        Some(PathBuf::from("diag.rs")),
    );
    let mut first = diagnostic(0, 0, "first");
    first.extra.insert(
        "relatedInformation".to_owned(),
        serde_json::json!([{
            "message": "see also",
            "location": {
                "uri": "file:///tmp/related.rs",
                "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 1}}
            }
        }]),
    );
    model.diagnostics.insert(
        PathBuf::from("diag.rs"),
        vec![first, diagnostic(0, 2, "second")],
    );
    model.ui.focus.split = true;
    model.jump_diagnostic(true);
    let peek = model.ui.diagnostic_peek.as_ref().expect("peek");
    assert_eq!(peek.diagnostics.len(), 2);
    assert!(peek.diagnostics[0].related[0].contains("related.rs"));
    let backend = TestBackend::new(30, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("Diagnostics"), "{rendered}");

    assert!(model.ui.focus.split);

    handle_event(&mut model, key(KeyCode::Esc));
    assert!(model.ui.diagnostic_peek.is_none());
    assert!(model.ui.focus.split);
}

#[test]
fn diagnostic_peek_clears_on_revision_change() {
    let mut model = Model::new(
        EditorState::from_text("one"),
        Some(PathBuf::from("diag.rs")),
    );
    model
        .diagnostics
        .insert(PathBuf::from("diag.rs"), vec![diagnostic(0, 0, "first")]);
    model.jump_diagnostic(true);
    assert!(model.ui.diagnostic_peek.is_some());
    model.editor.insert(0, "x");
    model.tick_status();
    assert!(model.ui.diagnostic_peek.is_none());
}

#[test]
fn diagnostic_peek_avoids_cursor_and_scrolls_long_messages() {
    let area = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: 30,
        height: 8,
    };
    let rect = crate::render::diagnostic_peek_rect(area, 3, 20, 6).expect("peek rect");
    assert!(rect.height < 6);
    assert!(rect.y > 3 || rect.y + rect.height <= 3);

    let mut model = Model::new(
        EditorState::from_text("one"),
        Some(PathBuf::from("diag.rs")),
    );
    let message = (0..20)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    model
        .diagnostics
        .insert(PathBuf::from("diag.rs"), vec![diagnostic(0, 0, &message)]);
    model.viewport_height = 10;
    model.jump_diagnostic(true);
    model.scroll_diagnostic_peek(5);
    assert!(model.ui.diagnostic_peek.as_ref().expect("peek").scroll > 0);
}

#[test]
fn problems_panel_jumps_to_selected_problem() {
    let mut model = Model::new(
        EditorState::from_text("one\ntwo"),
        Some(PathBuf::from("diag.rs")),
    );
    model
        .diagnostics
        .insert(PathBuf::from("diag.rs"), vec![diagnostic(1, 1, "problem")]);
    model.rebuild_problems();
    model.ui.problems.visible = true;
    model.ui.focus.tree = true;
    model.jump_problem();
    assert_eq!(model.cursor, model.offset_at_utf16(1, 1));
    assert!(!model.ui.focus.tree);
    assert!(!model.ui.problems.visible);
}

#[test]
fn diagnostics_underline_matching_line() {
    let mut model = Model::new(
        EditorState::from_text("first\nsecond"),
        Some(PathBuf::from("diag.rs")),
    );
    model.diagnostics.insert(
        PathBuf::from("diag.rs"),
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
    let backend = TestBackend::new(80, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let red = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .any(|cell| cell.fg == theme::ERROR);
    assert!(red, "diagnostic line was not rendered red");
}

#[test]
fn status_column_uses_grapheme_index() {
    let mut model = Model::new(EditorState::from_text("a👨‍👩‍👧‍👦b"), None);
    model.cursor = "a👨‍👩‍👧‍👦".len();
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
    assert!(rendered.contains("Col 3"));
    assert!(!rendered.contains("Col 26"));
}

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
    assert_eq!(model.buffers.len(), 2);
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
    assert!(model.visual_anchor.is_some());

    command(&mut model, "bn");
    assert!(model.visual_anchor.is_none());
    command(&mut model, "bp");
    assert_eq!(model.visual_anchor, Some(0));
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
    assert_eq!(model.status, "E: only one buffer");
}

#[test]
fn new_and_close_buffer_lifecycle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("file.txt");
    fs::write(&path, "file").expect("write");
    let mut model = Model::open(&path).expect("open");

    command(&mut model, "new");
    assert_eq!(model.path, None);
    assert_eq!(model.buffers.len(), 2);
    assert_eq!(model.active, 1);
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
    assert_eq!(model.buffers.len(), 1);

    command(&mut model, "bd");
    assert_eq!(model.path, None);
    assert!(model.buffers.is_empty());
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
    assert!(model.buffers.is_empty());

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
    assert!(model.status.starts_with("E:"), "{}", model.status);
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
    assert_eq!(model.buffers.len(), 1);
    assert_eq!(model.buffers[0].path.as_ref(), Some(&second));
}

#[test]
fn closing_buffer_clears_matching_split() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("file.txt");
    fs::write(&path, "file").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.split_vertical();
    assert!(model.split.is_some());

    command(&mut model, "bd");
    assert!(model.split.is_none());
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
            .split
            .as_ref()
            .is_some_and(|pane| pane.buffer.modified)
    );
    assert!(model.status.contains("split pane has unsaved changes"));
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
    model.buffers[0].modified = true;
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
    assert_eq!(model.buffers.len(), 10);
    assert_eq!(model.active, 9);

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
    assert!(model.status.contains("reloaded"));
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
    assert!(model.status.contains("No write since last change"));

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
    model.require_gateway = true;
    model.workspace = directory.path().to_path_buf();
    model.path = Some(path);
    assert!(!model.save());
    assert!(model.status.contains("gateway unavailable"));

    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(&mut model, "e other.txt");
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(model.status.contains("gateway unavailable"));
}

#[test]
fn gateway_refuses_paths_outside_workspace() {
    let directory = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.require_gateway = true;
    model.workspace = directory.path().to_path_buf();
    handle_event(&mut model, key(KeyCode::Char(':')));
    type_text(
        &mut model,
        &format!("e {}", outside.path().join("x").display()),
    );
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(
        model.status.contains("outside workspace"),
        "{}",
        model.status
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
    let previous = model.last_change.clone();
    assert_eq!(model.editor.text().len_bytes(), 0);
    handle_event(&mut model, key(KeyCode::Char('d')));
    handle_event(&mut model, key(KeyCode::Char('w')));
    assert_eq!(model.last_change, previous);
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
        InputMode::CommandLine { buffer } => assert_eq!(buffer, "w q"),
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
    assert!(model.last_change.is_none());
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
    assert!(model.modal_layer.is_none());
    assert!(model.visual_anchor.is_none());
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
    assert!(model.status.starts_with("E:"));
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
fn markdown_fenced_code_renders_a_full_width_background() {
    let mut model = Model::new(EditorState::from_text("```rust\nfn main() {}\n```\n"), None);
    model.path = Some(PathBuf::from("x.md"));
    for _ in 0..100 {
        model.poll_syntax();
        if model
            .syntax
            .borrow()
            .highlights()
            .iter()
            .any(|highlight| highlight.class == syntax::SyntaxClass::CodeBlock)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let buffer = terminal.backend().buffer();
    let width = buffer.area.width as usize;
    let cells = buffer.content();
    let keyword = cells
        .iter()
        .position(|cell| cell.symbol() == "f")
        .expect("code text");
    assert_eq!(cells[keyword].fg, theme::SYNTAX_KEYWORD);
    assert_eq!(cells[keyword].bg, theme::CODE_BLOCK_BG);
    let row = keyword / width;
    assert!(
        cells[row * width + width - 2].bg == theme::CODE_BLOCK_BG,
        "code background does not reach the right edge"
    );
}

#[test]
fn fenced_code_background_reaches_right_edge_when_hscrolled() {
    let mut model = Model::new(EditorState::from_text("```rust\nfn main() {}\n```\n"), None);
    model.path = Some(PathBuf::from("x.md"));
    model.hscroll = 1;
    for _ in 0..100 {
        model.poll_syntax();
        if model
            .syntax
            .borrow()
            .highlights()
            .iter()
            .any(|highlight| highlight.class == syntax::SyntaxClass::CodeBlock)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let mut lines = model.visible_lines(8, 80);
    crate::render::apply_code_block_background(&model, &mut lines, 20);
    assert_eq!(lines[1].width(), 21);
}

#[test]
fn cursorline_overlays_fenced_code_background() {
    let mut model = Model::new(EditorState::from_text("```rust\nfn main() {}\n```\n"), None);
    model.path = Some(PathBuf::from("x.md"));
    model.cursor = "```rust\n".len();
    for _ in 0..100 {
        model.poll_syntax();
        if model
            .syntax
            .borrow()
            .highlights()
            .iter()
            .any(|highlight| highlight.class == syntax::SyntaxClass::CodeBlock)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let buffer = terminal.backend().buffer();
    let keyword = buffer
        .content()
        .iter()
        .position(|cell| cell.symbol() == "f")
        .expect("code text");
    assert_eq!(buffer.content()[keyword].bg, theme::SURFACE_ALT);
}

#[test]
fn inline_code_gets_a_background_span() {
    let mut model = Model::new(EditorState::from_text("before `code` after"), None);
    model.path = Some(PathBuf::from("x.md"));
    for _ in 0..100 {
        model.poll_syntax();
        if !model.syntax.borrow().highlights().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let buffer = terminal.backend().buffer();
    let code = buffer
        .content()
        .iter()
        .position(|cell| cell.symbol() == "c")
        .expect("inline code");
    assert_eq!(buffer.content()[code].bg, theme::CODE_BLOCK_BG);
}

#[test]
fn search_background_overlays_fenced_code_background() {
    let mut model = Model::new(EditorState::from_text("```rust\nfn main() {}\n```\n"), None);
    model.path = Some(PathBuf::from("x.md"));
    model.cursor = 0;
    model.commit_search("fn".to_owned());
    model.cursor = 10;
    for _ in 0..100 {
        model.poll_syntax();
        if model
            .syntax
            .borrow()
            .highlights()
            .iter()
            .any(|highlight| highlight.class == syntax::SyntaxClass::CodeBlock)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let buffer = terminal.backend().buffer();
    let keyword = buffer
        .content()
        .iter()
        .position(|cell| cell.symbol() == "f")
        .expect("code text");
    assert_eq!(buffer.content()[keyword].bg, theme::SELECTION);
}

#[test]
fn mouse_hit_test_boundaries_and_cjk_snap() {
    let mut model = Model::new(EditorState::from_text("你好"), None);
    model.viewport_width = 20;
    model.viewport_height = 8;
    let _ = mouse_hit_boundary(&model, 0, 0, 20, 8);
    let _ = mouse_hit_boundary(&model, 19, 7, 20, 8);
    let _ = mouse_hit_boundary(&model, 20, 8, 20, 8);

    let press = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 7,
        row: 1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(press));
    assert_eq!(model.cursor, 3);
}

#[test]
fn dragging_outside_then_keyboard_resets_pointer() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    model.viewport_width = 20;
    model.viewport_height = 8;
    let down = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 5,
        row: 1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    let drag = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
        column: 100,
        row: 100,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(down));
    handle_event(&mut model, Event::Mouse(drag));
    assert!(matches!(
        model.pointer,
        crate::mouse::PointerState::Dragging(_)
    ));
    handle_event(&mut model, key(KeyCode::Char('j')));
    assert_eq!(model.pointer, crate::mouse::PointerState::Idle);
}

#[test]
fn clicking_outside_peek_restores_focus() {
    let mut model = Model::new(
        EditorState::from_text("one"),
        Some(PathBuf::from("diag.rs")),
    );
    model.viewport_width = 40;
    model.viewport_height = 12;
    model
        .diagnostics
        .insert(PathBuf::from("diag.rs"), vec![diagnostic(0, 0, "problem")]);
    model.ui.focus.split = true;
    model.jump_diagnostic(true);
    let click = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(click));
    assert!(model.ui.diagnostic_peek.is_none());
    assert!(model.ui.focus.split);
}

#[test]
fn dragging_split_handle_updates_ratio() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    model.viewport_width = 40;
    model.viewport_height = 12;
    model.split_vertical();
    let down = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 20,
        row: 3,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    let drag = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
        column: 30,
        row: 3,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(down));
    handle_event(&mut model, Event::Mouse(drag));
    assert!(model.split_ratio > 50);
}

fn mouse_hit_boundary(
    model: &Model,
    col: u16,
    row: u16,
    width: u16,
    height: u16,
) -> crate::mouse::Target {
    crate::mouse::hit_test(model, col, row, width, height)
}

#[test]
fn tree_visible_drag_keeps_editor_origin() {
    let mut model = Model::new(EditorState::from_text("abc\ndef"), None);
    model.viewport_width = 80;
    model.viewport_height = 12;
    model.tree_visible = true;
    let press = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 20,
        row: 2,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    let drag = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
        column: 20,
        row: 2,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(press));
    let cursor = model.cursor;
    handle_event(&mut model, Event::Mouse(drag));
    assert_eq!(model.cursor, cursor);
}

#[test]
fn tree_visible_split_resize_uses_editor_width() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    model.viewport_width = 80;
    model.viewport_height = 12;
    model.tree_visible = true;
    model.split_vertical();
    let press = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 49,
        row: 3,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    let drag = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
        column: 54,
        row: 3,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(press));
    handle_event(&mut model, Event::Mouse(drag));
    assert_eq!(model.split_ratio, 58);
}

#[test]
fn clicking_outside_command_palette_closes_it() {
    let mut model = Model::new(EditorState::from_text("one"), None);
    model.open_command_palette();
    let click = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(click));
    assert!(model.command_palette.is_none());
}

#[test]
fn clicking_outside_fuzzy_closes_it() {
    let mut model = Model::new(EditorState::from_text("one"), None);
    model.fuzzy = Some(FuzzyState {
        query: String::new(),
        entries: vec!["src/main.rs".to_owned()],
        filtered: vec![0],
        selected: 0,
    });
    let click = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(click));
    assert!(model.fuzzy.is_none());
}

#[test]
#[ignore = "manual long-line render boundary"]
fn long_line_render_stays_bounded() {
    let megabytes = std::env::var("ANVIL_LONG_LINE_MB")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);
    let text = "x".repeat(megabytes * 1024 * 1024);
    let model = Model::new(EditorState::from_text(&text), None);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
    let start = std::time::Instant::now();
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let elapsed = start.elapsed();
    println!("long-line-{megabytes}MB-render-ms={}", elapsed.as_millis());
    assert!(
        elapsed < std::time::Duration::from_millis(10),
        "long-line render took {elapsed:?}"
    );
}

#[test]
fn hit_test_extreme_dimensions_do_not_overflow() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    model.tree_visible = true;
    model.split_vertical();
    for (width, height) in [(0, 0), (1, 1), (2, 2), (u16::MAX, u16::MAX)] {
        let _ = mouse_hit_boundary(&model, 0, 0, width, height);
        let _ = mouse_hit_boundary(
            &model,
            width.saturating_sub(1),
            height.saturating_sub(1),
            width,
            height,
        );
    }
}

#[test]
fn dense_substitution_is_rejected() {
    let text = "a".repeat(1024 * 1024);
    let mut model = Model::new(EditorState::from_text(&text), None);
    model.execute_command("%s/a/b/g");
    assert!(model.status.contains("too many matches"));
    assert_eq!(model.editor.text().len_bytes(), text.len());
}

#[test]
fn line_substitution_ignores_dense_prefix() {
    let text = "a\n".repeat(10_050);
    let mut model = Model::new(EditorState::from_text(&text), None);
    model.cursor = text.len() - 2;
    model.execute_command("s/a/b/");
    assert!(model.status.contains("1 replacements"));
    assert_eq!(model.editor.text().slice(text.len() - 2..text.len()), "b\n");
}

#[test]
#[ignore = "manual 100MB cursor-at-end boundary"]
fn long_line_cursor_at_end_render_stays_bounded() {
    let megabytes = std::env::var("ANVIL_LONG_LINE_MB")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(100);
    let text = "x".repeat(megabytes * 1024 * 1024);
    let mut model = Model::new(EditorState::from_text(&text), None);
    model.cursor = text.len();
    model.hscroll = text.len().saturating_sub(100);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
    let start = std::time::Instant::now();
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let elapsed = start.elapsed();
    println!(
        "long-line-{megabytes}MB-cursor-end-render-ms={}",
        elapsed.as_millis()
    );
    assert!(
        elapsed < std::time::Duration::from_millis(10),
        "cursor-at-end render took {elapsed:?}"
    );
}

#[test]
fn diagnostics_follow_horizontal_window() {
    let mut model = Model::new(
        EditorState::from_text("abcdefghij"),
        Some(PathBuf::from("diag.rs")),
    );
    model.hscroll = 5;
    model.diagnostics.insert(
        PathBuf::from("diag.rs"),
        vec![lsp::Diagnostic {
            range: lsp::Range {
                start: lsp::Position {
                    line: 0,
                    character: 6,
                },
                end: lsp::Position {
                    line: 0,
                    character: 8,
                },
            },
            severity: Some(1),
            message: "error".to_owned(),
            source: Some("test".to_owned()),
            extra: serde_json::Map::new(),
        }],
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 8)).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let cells = terminal.backend().buffer().content();
    let underlined = |symbol: &str| {
        cells.iter().any(|cell| {
            cell.symbol() == symbol && cell.modifier.contains(ratatui::style::Modifier::UNDERLINED)
        })
    };
    assert!(!underlined("f"));
    assert!(underlined("g"));
}

#[test]
fn inlay_hints_follow_horizontal_window() {
    let mut model = Model::new(EditorState::from_text("abcdefghij"), None);
    model.hscroll = 5;
    model.inlay_hints = vec![
        InlayHint {
            line: 0,
            character: 7,
            label: ":visible".to_owned(),
        },
        InlayHint {
            line: 0,
            character: 1,
            label: ":hidden".to_owned(),
        },
    ];
    let mut terminal = Terminal::new(TestBackend::new(80, 8)).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains(":visible"));
    assert!(!rendered.contains(":hidden"));
}

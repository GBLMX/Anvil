use super::*;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
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
    assert!(matches!(model.input_mode, InputMode::Insert));
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
    assert!(lsp_settings(Path::new("README.md")).is_none());
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
fn closing_focused_split_restores_left_pane() {
    let mut model = Model::new(EditorState::from_text("left\nright"), None);
    model.split_vertical();
    model.focus_split(true);
    model.cursor = 5;
    model.close_split();
    assert!(model.split.is_none());
    assert!(!model.split_active);
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
    assert!(model.split_active);
    handle_event(&mut model, ctrl(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Char('h')));
    assert!(!model.split_active);
}

#[test]
fn split_panes_keep_independent_cursors() {
    let mut model = Model::new(EditorState::from_text("one\ntwo\nthree"), None);
    model.cursor = 1;
    model.split_vertical();
    assert!(model.split.is_some());
    assert!(!model.split_active);
    model.focus_split(true);
    assert!(model.split_active);
    assert_eq!(model.cursor, 1);
    model.cursor = 5;
    model.scroll = 2;
    model.hscroll = 3;
    model.focus_split(false);
    assert!(!model.split_active);
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
    assert_eq!(model.input_mode, InputMode::Normal);
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
    let lines = model.visible_lines(8);
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
            .diagnostic_popup
            .as_deref()
            .is_some_and(|text| text.contains("second"))
    );
    model.jump_diagnostic(false);
    assert_eq!(model.cursor, 0);
    assert!(
        model
            .diagnostic_popup
            .as_deref()
            .is_some_and(|text| text.contains("first"))
    );
    model.jump_diagnostic(false);
    assert_eq!(model.cursor, model.offset_at_utf16(2, 0));
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
    assert_eq!(model.input_mode, InputMode::Normal);

    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "Z");
    assert_eq!(model.input_mode, InputMode::Normal);
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
        model.input_mode,
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
    assert!(!model.split_active);
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
    assert!(matches!(model.input_mode, InputMode::ConfirmReload { .. }));
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

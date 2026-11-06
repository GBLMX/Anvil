use super::*;
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

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
        buffer: InputBuffer::default(),
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
        buffer: InputBuffer::default(),
    };
    handle_event(&mut model, ctrl(KeyCode::Char('x')));
    assert!(!model.ui.focus.tree);
    handle_event(&mut model, key(KeyCode::Char('a')));
    assert_eq!(
        match &model.ui.input_mode {
            InputMode::CommandLine { buffer } => buffer.text(),
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
        model.selected_command().map(|(id, _)| id).as_deref(),
        Some("view.focus_editor")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn command_palette_includes_plugin_commands() {
    use anvil_plugin_host::manifest::{PluginCommand, PluginManifest};

    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model
        .plugins
        .push(crate::plugin_discovery::PluginDescriptor {
            root: std::path::PathBuf::from("/tmp/plugin"),
            manifest: PluginManifest {
                id: "demo".to_owned(),
                version: "1.0.0".to_owned(),
                entry: "main.luau".to_owned(),
                api: "1.0".to_owned(),
                activation_events: vec!["onCommand:demo.trim".to_owned()],
                commands: vec![PluginCommand {
                    id: "demo.trim".to_owned(),
                    title: "Trim".to_owned(),
                }],
            },
            enabled: true,
            digest: None,
            source: None,
            previous_digest: None,
        });
    model.open_command_palette();
    let item = model
        .command_palette
        .as_ref()
        .expect("palette")
        .table
        .items()
        .iter()
        .find(|item| item.id == "demo.trim")
        .expect("plugin command");
    assert_eq!(item.plugin_id.as_deref(), Some("demo"));
    model.welcome = false;
    for character in "demo.trim".chars() {
        handle_event(&mut model, key(KeyCode::Char(character)));
    }
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(
        model.status.contains("gateway unavailable"),
        "{}",
        model.status
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
        query: InputBuffer::default(),
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
fn theme_command_switches_and_lists_builtins() {
    assert_eq!(theme::name(), "anvil-dark");
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.execute_command("theme one-dark");
    assert_eq!(theme::name(), "one-dark");
    assert!(model.status.contains("one-dark"));
    model.execute_command("theme missing");
    assert_eq!(theme::name(), "one-dark");
    assert!(model.status.starts_with("E:"));
    model.execute_command("theme");
    assert!(model.status.contains("solarized-light"));
    model.execute_command("theme anvil-dark");
    assert_eq!(theme::name(), "anvil-dark");
}

#[test]
fn command_line_tab_completes_and_enter_executes() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.welcome = false;
    handle_event(&mut model, key(KeyCode::Char(':')));
    for character in "the".chars() {
        handle_event(&mut model, key(KeyCode::Char(character)));
    }
    model.dirty = false;
    handle_event(&mut model, key(KeyCode::Tab));
    assert!(model.command_completion.is_some());
    assert!(model.dirty);
    handle_event(&mut model, key(KeyCode::Tab));
    assert!(matches!(
        &model.ui.input_mode,
        InputMode::CommandLine { buffer } if buffer.text() == "theme "
    ));
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(matches!(model.ui.input_mode, InputMode::Normal));
    assert!(model.status.contains("available:"));
}

#[test]
fn command_line_tab_opens_theme_argument_list() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.welcome = false;
    handle_event(&mut model, key(KeyCode::Char(':')));
    for character in "theme ".chars() {
        handle_event(&mut model, key(KeyCode::Char(character)));
    }
    handle_event(&mut model, key(KeyCode::Tab));
    assert!(model.command_completion.is_some());
    assert!(matches!(
        &model.ui.input_mode,
        InputMode::CommandLine { buffer } if buffer.text() == "theme "
    ));
}

#[test]
fn command_line_supports_mid_text_editing() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.welcome = false;
    handle_event(&mut model, key(KeyCode::Char(':')));
    for character in "ab".chars() {
        handle_event(&mut model, key(KeyCode::Char(character)));
    }
    handle_event(&mut model, key(KeyCode::Left));
    handle_event(&mut model, key(KeyCode::Char('X')));
    assert!(matches!(
        &model.ui.input_mode,
        InputMode::CommandLine { buffer } if buffer.text() == "aXb"
    ));
    handle_event(&mut model, key(KeyCode::Home));
    handle_event(&mut model, key(KeyCode::Delete));
    assert!(matches!(
        &model.ui.input_mode,
        InputMode::CommandLine { buffer } if buffer.text() == "Xb"
    ));
    handle_event(&mut model, key(KeyCode::End));
    handle_event(&mut model, key(KeyCode::Backspace));
    assert!(matches!(
        &model.ui.input_mode,
        InputMode::CommandLine { buffer } if buffer.text() == "X"
    ));
}

#[test]
fn search_mode_supports_mid_text_editing() {
    let mut model = Model::new(EditorState::from_text("alpha beta"), None);
    model.welcome = false;
    handle_event(&mut model, key(KeyCode::Char('/')));
    for character in "ab".chars() {
        handle_event(&mut model, key(KeyCode::Char(character)));
    }
    handle_event(&mut model, key(KeyCode::Left));
    handle_event(&mut model, key(KeyCode::Char('X')));
    assert!(matches!(
        &model.ui.input_mode,
        InputMode::Search { query } if query.text() == "aXb"
    ));
}

#[test]
fn command_line_completes_theme_argument() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.welcome = false;
    handle_event(&mut model, key(KeyCode::Char(':')));
    for character in "theme so".chars() {
        handle_event(&mut model, key(KeyCode::Char(character)));
    }
    handle_event(&mut model, key(KeyCode::Tab));
    handle_event(&mut model, key(KeyCode::Tab));
    assert!(matches!(
        &model.ui.input_mode,
        InputMode::CommandLine { buffer } if buffer.text() == "theme solarized-light "
    ));
    handle_event(&mut model, key(KeyCode::Enter));
    assert_eq!(theme::name(), "solarized-light");
    model.execute_command("theme anvil-dark");
}

#[test]
fn command_palette_previews_and_applies_themes() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.open_command_palette();
    *model
        .command_palette
        .as_mut()
        .expect("palette")
        .table
        .query_mut() = "solarized".to_owned();
    model.refresh_command_palette();
    assert_eq!(theme::name(), "solarized-light");
    model.close_command_palette();
    assert_eq!(theme::name(), "anvil-dark");
    model.open_command_palette();
    *model
        .command_palette
        .as_mut()
        .expect("palette")
        .table
        .query_mut() = "solarized".to_owned();
    model.refresh_command_palette();
    handle_event(&mut model, key(KeyCode::Enter));
    assert_eq!(theme::name(), "solarized-light");
    assert!(model.command_palette.is_none());
    model.execute_command("theme anvil-dark");
}

#[test]
fn theme_auto_and_save_persist_choice() {
    let directory = std::env::temp_dir().join(format!("anvil-theme-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temp dir");
    let path = directory.join("settings.toml");
    unsafe { std::env::set_var("ANVIL_SETTINGS", &path) };
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.persist_theme = true;
    model.execute_command("theme nord");
    assert!(model.status.contains("saved"));
    assert!(
        std::fs::read_to_string(&path)
            .expect("settings")
            .contains("name = \"nord\"")
    );
    model.execute_command("theme-auto nord solarized-light");
    assert!(model.status.contains("auto"));
    let text = std::fs::read_to_string(&path).expect("settings");
    assert!(text.contains("mode = \"auto\""));
    assert!(text.contains("dark = \"nord\""));
    model.persist_theme = false;
    model.execute_command("theme one-dark");
    model.execute_command("theme-save");
    assert!(model.status.contains("theme saved"));
    assert!(
        std::fs::read_to_string(&path)
            .expect("settings")
            .contains("name = \"one-dark\"")
    );
    unsafe { std::env::remove_var("ANVIL_SETTINGS") };
    std::fs::remove_dir_all(directory).expect("cleanup");
    model.execute_command("theme anvil-dark");
}

#[test]
fn typescript_uses_configured_tsserver() {
    let directory = std::env::temp_dir().join(format!("anvil-ts-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temp dir");
    let tsserver = directory.join("tsserver.js");
    std::fs::write(&tsserver, "").expect("tsserver");
    unsafe { std::env::set_var("ANVIL_TSSERVER", &tsserver) };
    let settings = lsp_settings(Path::new("main.ts")).expect("typescript");
    unsafe { std::env::remove_var("ANVIL_TSSERVER") };
    let project = directory.join("project");
    let project_tsserver = project.join("node_modules/typescript/lib/tsserver.js");
    std::fs::create_dir_all(project_tsserver.parent().expect("tsserver parent"))
        .expect("project tsserver dir");
    std::fs::write(&project_tsserver, "").expect("project tsserver");
    let source = project.join("src/main.ts");
    std::fs::create_dir_all(source.parent().expect("source parent")).expect("source dir");
    unsafe { std::env::set_var("ANVIL_LSP_TYPESCRIPT", "typescript-language-server") };
    let project_settings = lsp_settings(&source).expect("typescript");
    unsafe { std::env::remove_var("ANVIL_LSP_TYPESCRIPT") };
    let project_options = project_settings
        .initialization_options
        .expect("project tsserver options");
    assert_eq!(
        project_options["tsserver"]["path"],
        project_tsserver.to_string_lossy().as_ref()
    );
    let options = settings.initialization_options.expect("tsserver options");
    assert_eq!(
        options["tsserver"]["path"],
        tsserver.to_string_lossy().as_ref()
    );
    std::fs::remove_dir_all(directory).expect("cleanup");
}

#[cfg(target_os = "linux")]
#[test]
fn managed_state_errors_are_visible_and_fail_closed() {
    let directory = tempfile::tempdir().expect("tempdir");
    let local_plugins = directory.path().join("local");
    std::fs::create_dir(&local_plugins).expect("local plugins");
    // The test process is single-writer for this variable; discovery tests use
    // explicit paths and do not read the ambient plugin root.
    unsafe { std::env::set_var("ANVIL_PLUGIN_DIR", &local_plugins) };
    let socket = directory.path().join("gateway.sock");
    let listener = anvil_ipc::Listener::bind(&socket).expect("bind");
    let server = std::thread::spawn(move || {
        let mut connection = listener.accept().expect("accept");
        let frame = connection.recv().expect("hello").expect("hello frame");
        let request: serde_json::Value = serde_json::from_slice(&frame).expect("hello json");
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request["id"],
            "result": {"session_id": "test"}
        });
        connection
            .send(&serde_json::to_vec(&response).expect("hello response"))
            .expect("send hello");
        let frame = connection.recv().expect("request").expect("request frame");
        let request: serde_json::Value = serde_json::from_slice(&frame).expect("request json");
        assert_eq!(request["method"], "pkg.installed");
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request["id"],
            "error": {"code": -32009, "message": "corrupt state"}
        });
        connection
            .send(&serde_json::to_vec(&response).expect("error response"))
            .expect("send error");
    });
    let client = anvil_ipc::GatewayClient::connect(&socket, &[7; 32]).expect("client");
    let mut model = Model::with_gateway(client, directory.path().to_path_buf());
    let error = model.load_plugins().expect_err("managed state error");
    assert!(error.contains("corrupt state"), "{error}");
    assert!(model.plugins.is_empty());
    server.join().expect("server");
}

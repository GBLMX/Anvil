use super::*;

#[test]
fn ctrl_w_focuses_tree_and_editor_without_a_split() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.open_tree();
    assert!(model.ui.focus.tree);

    handle_event(&mut model, ctrl(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Char('l')));
    assert!(!model.ui.focus.tree);
    assert!(model.workspace.tree.visible);

    handle_event(&mut model, ctrl(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Char('h')));
    assert!(model.ui.focus.tree);
}

#[test]
fn space_leader_expires_without_consuming_next_key() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta"), None);
    handle_event(&mut model, key(KeyCode::Char(' ')));
    assert_eq!(model.input.pending, Some(' '));
    model.input.leader_deadline = Some(std::time::Instant::now());
    model.expire_leader();
    assert_eq!(model.input.pending, None);
    handle_event(&mut model, key(KeyCode::Char('j')));
    assert_eq!(model.session.cursor, 6);
}

#[test]
fn repeated_space_keeps_leader_pending() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta"), None);
    model.start_page.welcome = false;
    handle_event(&mut model, key(KeyCode::Char(' ')));
    handle_event(&mut model, key(KeyCode::Char(' ')));
    assert_eq!(model.input.pending, Some(' '));
    assert!(model.input.leader_deadline.is_some());
}

#[test]
fn space_leader_allows_slow_key_sequences() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    handle_event(&mut model, key(KeyCode::Char(' ')));
    let deadline = model.input.leader_deadline.expect("leader deadline");
    let remaining = deadline.duration_since(std::time::Instant::now());
    assert!(remaining >= std::time::Duration::from_millis(1_000));
    assert!(remaining <= std::time::Duration::from_millis(1_500));
}

#[test]
fn popup_consumes_j_before_editor_motion() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta"), None);
    model.start_page.welcome = false;
    model.overlays.lsp_results.document_symbols = Some(SymbolState {
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
    assert_eq!(model.session.cursor, 0);
    assert_eq!(
        model
            .overlays
            .lsp_results
            .document_symbols
            .as_ref()
            .map(|symbols| symbols.selected),
        Some(1)
    );
}

#[test]
fn lsp_list_unknown_key_cancels_and_passes_through() {
    let mut model = Model::new(EditorState::from_text("alpha\n"), None);
    model.start_page.welcome = false;
    model.overlays.lsp_results.document_symbols = Some(SymbolState {
        items: vec![SymbolItem {
            name: "alpha".to_owned(),
            detail: None,
            line: 0,
            character: 0,
            depth: 0,
        }],
        selected: 0,
    });

    handle_event(&mut model, key(KeyCode::Char('i')));
    assert!(model.overlays.lsp_results.document_symbols.is_none());
    assert!(matches!(model.ui.input_mode, InputMode::Insert));
}

#[test]
fn pending_space_renders_keymap_continuations() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
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
    assert!(rendered.contains("[file]"), "{rendered}");
}

#[test]
fn pending_operator_which_key_shows_text_object_extenders() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
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
    model.start_page.welcome = false;
    model.ui.focus.tree = true;
    assert_eq!(
        model.active_keymap_contexts(),
        vec![KeymapContext::Global, KeymapContext::FileTree]
    );

    model.overlays.fuzzy = Some(FuzzyState {
        generation: 0,
        query: String::new(),
        entries: vec!["a".to_owned()],
        filtered: vec![0],
        selected: 0,
    });
    model.push_overlay(OverlayKind::Fuzzy);
    assert_eq!(
        model.active_keymap_contexts(),
        vec![
            KeymapContext::Global,
            KeymapContext::FileTree,
            KeymapContext::Popup
        ]
    );

    model.close_fuzzy();
    model.open_buffer_picker();
    assert_eq!(
        model.active_keymap_contexts(),
        vec![
            KeymapContext::Global,
            KeymapContext::FileTree,
            KeymapContext::Popup
        ]
    );
    model.close_buffer_picker();
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
    model.edit.visual_anchor = Some(0);
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
fn hover_unknown_key_dismisses_and_passes_through() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.overlays.lsp_results.hover = Some("docs".to_owned());
    handle_event(&mut model, key(KeyCode::Char('i')));
    assert!(model.overlays.lsp_results.hover.is_none());
    assert!(matches!(model.ui.input_mode, InputMode::Insert));
}

#[test]
fn settings_keys_use_the_action_table() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    model.start_page.welcome = false;
    model.open_settings();
    handle_event(&mut model, key(KeyCode::Char('j')));
    handle_event(&mut model, key(KeyCode::Char('l')));
    assert!(model.config.settings.editor.auto_save);
    handle_event(&mut model, key(KeyCode::Esc));
    assert!(model.overlays.settings_overlay.is_none());
}

#[test]
fn overlay_stack_is_lifo_and_restores_focus() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.ui.focus.tree = true;

    model.open_settings();
    assert_eq!(model.overlays.stack.len(), 1);

    model.ui.focus.tree = false;
    model.open_command_palette();
    assert_eq!(model.overlays.stack.len(), 2);
    assert_eq!(
        model.overlays.stack.last().map(|entry| entry.kind),
        Some(OverlayKind::CommandPalette)
    );

    model.ui.focus.tree = true;
    handle_event(&mut model, key(KeyCode::Char('j')));
    assert_eq!(
        model
            .overlays
            .command_palette
            .as_ref()
            .expect("palette")
            .table
            .query(),
        "j"
    );
    assert_eq!(
        model
            .overlays
            .settings_overlay
            .as_ref()
            .expect("settings")
            .selected,
        0
    );

    handle_event(&mut model, key(KeyCode::Esc));
    assert!(model.overlays.command_palette.is_none());
    assert!(model.overlays.settings_overlay.is_some());
    assert!(!model.ui.focus.tree);
    assert_eq!(model.overlays.stack.len(), 1);

    handle_event(&mut model, key(KeyCode::Esc));
    assert!(model.overlays.settings_overlay.is_none());
    assert!(model.ui.focus.tree);
    assert!(model.overlays.stack.is_empty());
}

#[test]
fn theme_browser_uses_the_overlay_stack() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.open_theme_browser();
    assert_eq!(model.top_overlay(), Some(OverlayKind::ThemeBrowser));

    handle_event(&mut model, key(KeyCode::Esc));
    assert!(model.overlays.theme_browser.is_none());
    assert!(model.overlays.stack.is_empty());
}

#[test]
fn problems_panel_uses_the_overlay_stack() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.ui.focus.tree = true;

    model.toggle_problems();
    assert!(model.ui.problems.visible);
    assert_eq!(model.top_overlay(), Some(OverlayKind::Problems));

    handle_event(&mut model, key(KeyCode::Esc));
    assert!(!model.ui.problems.visible);
    assert!(model.overlays.stack.is_empty());
    assert!(model.ui.focus.tree);
}

#[test]
fn space_d_opens_the_diagnostics_picker() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;

    handle_event(&mut model, key(KeyCode::Char(' ')));
    handle_event(&mut model, key(KeyCode::Char('d')));
    assert!(model.ui.problems.visible);
    assert_eq!(model.top_overlay(), Some(OverlayKind::Problems));

    model.close_problems();
    model.execute_command("diagnostics");
    assert!(model.ui.problems.visible);
    assert_eq!(model.top_overlay(), Some(OverlayKind::Problems));
}

#[test]
fn space_g_opens_the_changed_files_picker() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.workspace.path = PathBuf::from("/ws");
    model
        .workspace
        .git
        .status
        .insert(PathBuf::from("/ws/src/main.rs"), 'M');

    handle_event(&mut model, key(KeyCode::Char(' ')));
    handle_event(&mut model, key(KeyCode::Char('g')));
    assert_eq!(model.top_overlay(), Some(OverlayKind::BufferPicker));
    let picker = model.overlays.buffer_picker.as_ref().expect("picker");
    assert!(matches!(
        &picker.mode,
        crate::state::BufferPickerMode::ChangedFiles { .. }
    ));
    assert_eq!(picker.entries.len(), 1);
    assert_eq!(picker.entries[0].display, "M src/main.rs");

    model.close_buffer_picker();
    model.execute_command("changed-files");
    assert_eq!(model.top_overlay(), Some(OverlayKind::BufferPicker));
}

#[test]
fn changed_files_picker_opens_the_selected_path() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("src/main.rs");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(&path, "hello").expect("write");
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.file.modified = false;
    model.workspace.path = directory.path().to_path_buf();
    model.workspace.git.status.insert(path.clone(), 'M');

    model.open_changed_files_picker();
    model.apply_buffer_picker();

    assert_eq!(model.session.path.as_deref(), Some(path.as_path()));
    assert_eq!(model.session.editor.text().slice(0..5), "hello");
}

#[test]
fn space_j_opens_jump_picker_and_selects_older_jump() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.session.path = Some(PathBuf::from("main.rs"));
    model.session.jump_list.record(crate::jump::JumpEntry {
        path: PathBuf::from("main.rs"),
        cursor: 1,
    });
    model.session.jump_list.record(crate::jump::JumpEntry {
        path: PathBuf::from("main.rs"),
        cursor: 2,
    });

    handle_event(&mut model, key(KeyCode::Char(' ')));
    handle_event(&mut model, key(KeyCode::Char('j')));
    let picker = model.overlays.buffer_picker.as_ref().expect("picker");
    assert!(matches!(
        &picker.mode,
        crate::state::BufferPickerMode::Jumps { .. }
    ));
    assert_eq!(picker.entries.len(), 2);
    assert!(picker.entries[0].display.starts_with('←'));

    model
        .overlays
        .buffer_picker
        .as_mut()
        .expect("picker")
        .selected = 1;
    model.apply_buffer_picker();
    assert_eq!(model.session.cursor, 1);
    assert_eq!(model.session.jump_list.back_len(), 0);
    assert_eq!(model.session.jump_list.forward_len(), 2);

    model.jump_forward();
    assert_eq!(model.session.cursor, 2);
    model.jump_forward();
    assert_eq!(model.session.cursor, 0);
}

#[test]
fn forward_jump_target_preserves_intermediate_history() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.session.path = Some(PathBuf::from("main.rs"));
    model
        .session
        .jump_list
        .push_forward(crate::jump::JumpEntry {
            path: PathBuf::from("main.rs"),
            cursor: 2,
        });
    model
        .session
        .jump_list
        .push_forward(crate::jump::JumpEntry {
            path: PathBuf::from("main.rs"),
            cursor: 1,
        });

    model.jump_history_to(true, 1);
    assert_eq!(model.session.cursor, 2);
    assert_eq!(model.session.jump_list.back_len(), 2);
    assert_eq!(model.session.jump_list.forward_len(), 0);

    model.jump_back();
    assert_eq!(model.session.cursor, 1);
    model.jump_back();
    assert_eq!(model.session.cursor, 0);
}

#[test]
fn space_shift_h_opens_search_history() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.commit_search("alpha".to_owned());

    handle_event(&mut model, key(KeyCode::Char(' ')));
    handle_event(&mut model, key(KeyCode::Char('H')));
    let picker = model.overlays.buffer_picker.as_ref().expect("picker");
    assert!(matches!(
        &picker.mode,
        crate::state::BufferPickerMode::SearchHistory { .. }
    ));
    assert_eq!(picker.entries[0].display, "alpha");
}

#[test]
fn command_history_and_keymap_shortcuts_open_the_expected_surfaces() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.overlays.command_palette_recents = vec!["editor.undo".to_owned()];

    handle_event(&mut model, key(KeyCode::Char(' ')));
    handle_event(&mut model, key(KeyCode::Char(':')));
    let palette = model.overlays.command_palette.as_ref().expect("history");
    assert_eq!(palette.mode, crate::state::CommandPaletteMode::History);
    model.close_command_palette();

    handle_event(&mut model, key(KeyCode::Char(' ')));
    handle_event(&mut model, key(KeyCode::Char('K')));
    assert_eq!(model.pager.title.as_deref(), Some("keymaps"));

    model.close_help();
    model.execute_command("keybinds");
    let palette = model.overlays.command_palette.as_ref().expect("keybinds");
    assert_eq!(palette.mode, crate::state::CommandPaletteMode::Keybinds);
}

#[test]
fn space_shift_l_opens_language_readiness() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;

    handle_event(&mut model, key(KeyCode::Char(' ')));
    handle_event(&mut model, key(KeyCode::Char('L')));
    assert_eq!(model.pager.title.as_deref(), Some("languages"));
    assert!(model.status.text.starts_with("Languages:"));
}

#[test]
fn problems_panel_filters_with_query_and_clear() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.toggle_problems();
    model.ui.problems.replace(vec![
        Problem {
            path: PathBuf::from("a.rs"),
            line: 0,
            character: 0,
            severity: Some(1),
            source: Some("rust-analyzer".to_owned()),
            code: None,
            message: "unused import".to_owned(),
        },
        Problem {
            path: PathBuf::from("b.rs"),
            line: 0,
            character: 0,
            severity: Some(2),
            source: Some("clippy".to_owned()),
            code: None,
            message: "needless borrow".to_owned(),
        },
    ]);

    for character in "unused".chars() {
        handle_event(&mut model, key(KeyCode::Char(character)));
    }
    assert_eq!(model.ui.problems.query, "unused");

    handle_event(&mut model, ctrl(KeyCode::Char('a')));
    assert_eq!(model.ui.problems.query, "unused");
    assert_eq!(model.ui.problems.visible_len(), 1);
    assert_eq!(
        model
            .ui
            .problems
            .selected()
            .map(|problem| problem.path.clone()),
        Some(PathBuf::from("a.rs"))
    );

    handle_event(&mut model, ctrl(KeyCode::Char('u')));
    assert!(model.ui.problems.query.is_empty());
    assert_eq!(model.ui.problems.visible_len(), 2);

    model.ui.problems.set_query("unused".to_owned());
    model.close_problems();
    model.toggle_problems();
    assert!(model.ui.problems.query.is_empty());
}

#[test]
fn popup_context_bindings_consume_buffer_picker_keys() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.open_buffer_picker();
    model.config.keymap = crate::keymap::KeymapConfig::from_toml(
        r#"
[keymap.contexts.popup]
x = "view.focus_editor"
"#,
    )
    .expect("context keymap");

    handle_event(&mut model, key(KeyCode::Char('x')));
    assert!(
        model
            .overlays
            .buffer_picker
            .as_ref()
            .expect("picker")
            .query
            .is_empty()
    );
}

#[test]
fn modal_context_bindings_apply_to_surround_layer() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.edit.visual_anchor = Some(0);
    model.edit.modal_layer = Some(ModalLayer::Surround);
    model.config.keymap = crate::keymap::KeymapConfig::from_toml(
        r#"
[keymap.contexts.modal]
x = "view.focus_editor"
"#,
    )
    .expect("context keymap");

    handle_event(&mut model, key(KeyCode::Char('x')));
    assert_eq!(model.edit.modal_layer, Some(ModalLayer::Surround));
}

#[test]
fn file_tree_context_bindings_dispatch_before_hardcoded_keys() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.open_tree();
    assert!(model.ui.focus.tree);
    model.config.keymap = crate::keymap::KeymapConfig::from_toml(
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
    model.start_page.welcome = false;
    model.open_tree();
    model.overlays.lsp_results.document_symbols = Some(SymbolState {
        items: vec![SymbolItem {
            name: "alpha".to_owned(),
            detail: None,
            line: 0,
            character: 0,
            depth: 0,
        }],
        selected: 0,
    });
    model.push_overlay(OverlayKind::DocumentSymbols);
    model.config.keymap = crate::keymap::KeymapConfig::from_toml(
        r#"
[keymap.contexts.popup]
x = "view.focus_editor"
"#,
    )
    .expect("context keymap");
    handle_event(&mut model, key(KeyCode::Char('x')));
    assert!(!model.ui.focus.tree);
    assert!(model.overlays.lsp_results.document_symbols.is_some());
}

#[test]
fn global_context_does_not_shadow_open_popup() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta"), None);
    model.start_page.welcome = false;
    model.overlays.lsp_results.document_symbols = Some(SymbolState {
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
    model.config.keymap = crate::keymap::KeymapConfig::from_toml(
        r#"
[keymap.contexts.global]
j = "editor.undo"
"#,
    )
    .expect("global keymap");
    handle_event(&mut model, key(KeyCode::Char('j')));
    assert_eq!(model.session.cursor, 0);
    assert_eq!(
        model
            .overlays
            .lsp_results
            .document_symbols
            .as_ref()
            .map(|symbols| symbols.selected),
        Some(1)
    );
}

#[test]
fn command_line_context_is_explicit_without_global_fallback() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    model.open_tree();
    model.config.keymap = crate::keymap::KeymapConfig::from_toml(
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
    model.start_page.welcome = false;
    model.open_tree();
    assert!(model.ui.focus.tree);
    for key_code in [KeyCode::Char(' '), KeyCode::Char('p')] {
        handle_event(&mut model, key(key_code));
    }
    assert!(model.overlays.command_palette.is_some());
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
    assert!(model.overlays.command_palette.is_none());
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
        .runtime
        .plugins
        .entries
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
    model.runtime.plugins.dynamic_commands.insert(
        "demo.dynamic".to_owned(),
        crate::state::DynamicPluginCommand {
            plugin_id: "demo".to_owned(),
            title: "Dynamic".to_owned(),
        },
    );
    model.open_command_palette();
    let item = model
        .overlays
        .command_palette
        .as_ref()
        .expect("palette")
        .table
        .items()
        .iter()
        .find(|item| item.id == "demo.trim")
        .expect("plugin command");
    assert_eq!(item.plugin_id.as_deref(), Some("demo"));
    let dynamic = model
        .overlays
        .command_palette
        .as_ref()
        .expect("palette")
        .table
        .items()
        .iter()
        .find(|item| item.id == "demo.dynamic")
        .expect("dynamic plugin command");
    assert_eq!(dynamic.plugin_id.as_deref(), Some("demo"));
    model.start_page.welcome = false;
    for character in "demo.trim".chars() {
        handle_event(&mut model, key(KeyCode::Char(character)));
    }
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(
        model.status.text.contains("gateway unavailable"),
        "{}",
        model.status.text
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
    search.start_page.welcome = false;
    search.open_tree();
    search.config.keymap = keymap;
    search.ui.input_mode = InputMode::Search {
        query: InputBuffer::default(),
    };
    handle_event(&mut search, ctrl(KeyCode::Char('x')));
    assert!(!search.ui.focus.tree);
    assert!(matches!(search.ui.input_mode, InputMode::Search { .. }));

    let mut rename = Model::new(EditorState::from_text("alpha"), None);
    rename.start_page.welcome = false;
    rename.open_tree();
    rename.config.keymap = crate::keymap::KeymapConfig::from_toml(
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
    model.workspace.path = dir.path().to_path_buf();
    model.open_tree();
    assert!(model.workspace.tree.visible);
    let index = model
        .workspace
        .tree
        .entries
        .iter()
        .position(|entry| entry.path.ends_with("src"))
        .expect("src");
    model.workspace.tree.selected = Some(index);
    model.tree_toggle();
    assert!(
        model
            .workspace
            .tree
            .entries
            .iter()
            .any(|entry| entry.path.ends_with("src/main.rs"))
    );
    model.tree_toggle();
    assert!(
        !model
            .workspace
            .tree
            .entries
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
    model.workspace.path = dir.path().to_path_buf();
    model.open_tree();
    let src = model
        .workspace
        .tree
        .entries
        .iter()
        .position(|entry| entry.path.ends_with("src"))
        .expect("src");
    model.workspace.tree.selected = Some(src);
    model.tree_toggle();
    let main = model
        .workspace
        .tree
        .entries
        .iter()
        .position(|entry| entry.path.ends_with("src/main.rs"))
        .expect("main");
    model.workspace.tree.selected = Some(main);
    fs::write(dir.path().join("src/added.rs"), "").expect("new file");

    model.refresh_tree(None);
    assert!(
        model
            .workspace
            .tree
            .entries
            .iter()
            .any(|entry| entry.path.ends_with("src/added.rs"))
    );
    assert!(
        model
            .workspace
            .tree
            .entries
            .iter()
            .any(|entry| entry.path.ends_with("src") && entry.expanded)
    );
    assert_eq!(
        model.workspace.tree.entries[model.workspace.tree.selected.expect("selection")].path,
        dir.path().join("src/main.rs")
    );

    let renamed = dir.path().join("src/renamed.rs");
    fs::rename(dir.path().join("src/main.rs"), &renamed).expect("rename");
    model.refresh_tree(None);
    assert!(
        model
            .workspace
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == renamed)
    );
    assert_eq!(
        model.workspace.tree.entries[model.workspace.tree.selected.expect("rename fallback")].path,
        dir.path().join("src")
    );

    fs::remove_file(renamed).expect("remove");
    model.refresh_tree(None);
    assert_eq!(
        model.workspace.tree.entries[model.workspace.tree.selected.expect("delete fallback")].path,
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
    model.workspace.path = dir.path().to_path_buf();
    model.start_fuzzy();
    assert_eq!(model.top_overlay(), Some(OverlayKind::Fuzzy));
    let fuzzy = model.overlays.fuzzy.as_mut().expect("fuzzy");
    fuzzy.query = "main".to_owned();
    model.fuzzy_refresh();
    assert_eq!(
        model.overlays.fuzzy.as_ref().expect("fuzzy").filtered.len(),
        1
    );
    model.close_fuzzy();
    assert!(model.overlays.stack.is_empty());
}
#[test]
fn stale_preview_results_are_discarded() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.overlays.preview_generation = 2;
    model.overlays.preview = PreviewState {
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
    assert!(model.overlays.preview.loading);
    assert!(model.overlays.preview.lines.is_empty());

    model.handle_preview(
        "src/main.rs".to_owned(),
        2,
        Ok(PreviewData {
            lines: vec!["fresh".to_owned()],
            truncated: true,
        }),
    );
    assert!(!model.overlays.preview.loading);
    assert_eq!(model.overlays.preview.lines, ["fresh"]);
    assert!(model.overlays.preview.truncated);
}

#[test]
fn preview_failure_keeps_the_list_usable() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.overlays.preview_generation = 1;
    model.overlays.preview = PreviewState {
        path: Some("src/main.rs".to_owned()),
        loading: true,
        ..PreviewState::default()
    };
    model.handle_preview(
        "src/main.rs".to_owned(),
        1,
        Err("gateway failed".to_owned()),
    );
    assert!(!model.overlays.preview.loading);
    assert_eq!(
        model.overlays.preview.message.as_deref(),
        Some("gateway failed")
    );
    assert!(model.status.text.contains("E: preview: gateway failed"));
}

#[test]
fn preview_without_worker_reports_gateway_unavailable() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.overlays.fuzzy = Some(FuzzyState {
        generation: 0,
        query: String::new(),
        entries: vec!["src/main.rs".to_owned()],
        filtered: vec![0],
        selected: 0,
    });
    model.request_preview();
    assert!(!model.overlays.preview.loading);
    assert_eq!(
        model.overlays.preview.message.as_deref(),
        Some("preview unavailable: gateway")
    );
    assert!(
        model
            .status
            .text
            .contains("E: preview: gateway unavailable")
    );
}

#[test]
fn theme_command_switches_and_lists_builtins() {
    assert_eq!(theme::name(), "anvil-dark");
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.execute_command("theme one-dark");
    assert_eq!(theme::name(), "one-dark");
    assert!(model.status.text.contains("one-dark"));
    model.execute_command("theme missing");
    assert_eq!(theme::name(), "one-dark");
    assert!(model.status.text.starts_with("E:"));
    model.execute_command("theme");
    assert!(model.status.text.contains("solarized-light"));
    model.execute_command("theme anvil-dark");
    assert_eq!(theme::name(), "anvil-dark");
}

#[test]
fn command_line_tab_completes_and_enter_executes() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    handle_event(&mut model, key(KeyCode::Char(':')));
    for character in "the".chars() {
        handle_event(&mut model, key(KeyCode::Char(character)));
    }
    model.file.dirty = false;
    handle_event(&mut model, key(KeyCode::Tab));
    assert!(model.overlays.command_completion.is_some());
    assert!(model.file.dirty);
    handle_event(&mut model, key(KeyCode::Tab));
    assert!(matches!(
        &model.ui.input_mode,
        InputMode::CommandLine { buffer } if buffer.text() == "theme "
    ));
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(matches!(model.ui.input_mode, InputMode::Normal));
    assert!(model.status.text.contains("available:"));
}

#[test]
fn command_line_tab_opens_theme_argument_list() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
    handle_event(&mut model, key(KeyCode::Char(':')));
    for character in "theme ".chars() {
        handle_event(&mut model, key(KeyCode::Char(character)));
    }
    handle_event(&mut model, key(KeyCode::Tab));
    assert!(model.overlays.command_completion.is_some());
    assert!(matches!(
        &model.ui.input_mode,
        InputMode::CommandLine { buffer } if buffer.text() == "theme "
    ));
}

#[test]
fn command_line_supports_mid_text_editing() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.start_page.welcome = false;
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
    model.start_page.welcome = false;
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
    model.start_page.welcome = false;
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
        .overlays
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
        .overlays
        .command_palette
        .as_mut()
        .expect("palette")
        .table
        .query_mut() = "solarized".to_owned();
    model.refresh_command_palette();
    handle_event(&mut model, key(KeyCode::Enter));
    assert_eq!(theme::name(), "solarized-light");
    assert!(model.overlays.command_palette.is_none());
    model.execute_command("theme anvil-dark");
}

#[test]
fn theme_auto_and_save_persist_choice() {
    let _guard = crate::settings::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = std::env::temp_dir().join(format!("anvil-theme-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temp dir");
    let path = directory.join("settings.toml");
    unsafe { std::env::set_var("ANVIL_SETTINGS", &path) };
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.appearance.persist_theme = true;
    model.execute_command("theme nord");
    assert!(model.status.text.contains("saved"));
    assert!(
        std::fs::read_to_string(&path)
            .expect("settings")
            .contains("name = \"nord\"")
    );
    model.execute_command("theme-auto nord solarized-light");
    assert!(model.status.text.contains("auto"));
    let text = std::fs::read_to_string(&path).expect("settings");
    assert!(text.contains("mode = \"auto\""));
    assert!(text.contains("dark = \"nord\""));
    model.appearance.persist_theme = false;
    model.execute_command("theme one-dark");
    model.execute_command("theme-save");
    assert!(model.status.text.contains("theme saved"));
    assert!(
        std::fs::read_to_string(&path)
            .expect("settings")
            .contains("name = \"one-dark\"")
    );
    unsafe { std::env::remove_var("ANVIL_SETTINGS") };
    std::fs::remove_dir_all(directory).expect("cleanup");
    model.execute_command("theme anvil-dark");
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
    assert!(model.runtime.plugins.entries.is_empty());
    server.join().expect("server");
}

#[test]
fn fuzzy_merges_secondary_root_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("second");
    fs::create_dir_all(&root).expect("second root");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = dir.path().to_path_buf();
    model.workspace.roots =
        crate::workspace_roots::WorkspaceRoots::new(dir.path()).expect("primary root");
    model.workspace.roots.add(&root, true).expect("second root");
    model.overlays.fuzzy = Some(FuzzyState {
        generation: 4,
        query: String::new(),
        entries: vec!["src/main.rs".to_owned()],
        filtered: vec![0],
        selected: 0,
    });

    model.handle_fuzzy_files(Some(root.clone()), 4, Ok(vec!["src/lib.rs".to_owned()]));

    let entries = &model.overlays.fuzzy.as_ref().expect("fuzzy").entries;
    assert!(entries.iter().any(|entry| entry == "src/main.rs"));
    assert!(
        entries
            .iter()
            .any(|entry| entry == &root.join("src/lib.rs").to_string_lossy())
    );
}

#[test]
fn stale_secondary_root_files_are_discarded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("second");
    fs::create_dir_all(&root).expect("second root");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = dir.path().to_path_buf();
    model.workspace.roots =
        crate::workspace_roots::WorkspaceRoots::new(dir.path()).expect("primary root");
    model.workspace.roots.add(&root, true).expect("second root");
    model.overlays.fuzzy = Some(FuzzyState {
        generation: 4,
        query: String::new(),
        entries: Vec::new(),
        filtered: Vec::new(),
        selected: 0,
    });

    model.handle_fuzzy_files(Some(root), 3, Ok(vec!["src/lib.rs".to_owned()]));

    assert!(
        model
            .overlays
            .fuzzy
            .as_ref()
            .expect("fuzzy")
            .entries
            .is_empty()
    );
}

#[test]
fn fuzzy_opens_absolute_secondary_root_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("lib.rs");
    fs::write(&file, "pub fn lib() {}").expect("file");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.overlays.fuzzy = Some(FuzzyState {
        generation: 1,
        query: String::new(),
        entries: vec![file.to_string_lossy().into_owned()],
        filtered: vec![0],
        selected: 0,
    });

    model.fuzzy_open();

    assert_eq!(model.session.path.as_ref(), Some(&file));
    assert_eq!(model.session.editor.text().slice(0..15), "pub fn lib() {}");
}

#[test]
fn fuzzy_keeps_root_files_when_primary_result_arrives_later() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("second");
    fs::create_dir_all(&root).expect("second root");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = dir.path().to_path_buf();
    model.workspace.roots =
        crate::workspace_roots::WorkspaceRoots::new(dir.path()).expect("primary root");
    model.workspace.roots.add(&root, true).expect("second root");
    model.overlays.fuzzy = Some(FuzzyState {
        generation: 4,
        query: String::new(),
        entries: Vec::new(),
        filtered: Vec::new(),
        selected: 0,
    });

    model.handle_fuzzy_files(Some(root.clone()), 4, Ok(vec!["src/lib.rs".to_owned()]));
    model.handle_fuzzy_files(None, 4, Ok(vec!["src/main.rs".to_owned()]));

    let entries = &model.overlays.fuzzy.as_ref().expect("fuzzy").entries;
    assert!(entries.iter().any(|entry| entry == "src/main.rs"));
    assert!(
        entries
            .iter()
            .any(|entry| entry == &root.join("src/lib.rs").to_string_lossy())
    );
}

#[test]
fn fuzzy_ranks_basename_then_frecency_then_short_paths() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.overlays.fuzzy = Some(FuzzyState {
        generation: 0,
        query: "main".to_owned(),
        entries: vec![
            "src/domain.rs".to_owned(),
            "deep/nested/main.rs".to_owned(),
            "src/main.rs".to_owned(),
            "tests/main_test.rs".to_owned(),
        ],
        filtered: Vec::new(),
        selected: 0,
    });
    model
        .workspace
        .recent
        .frecency
        .insert("tests/main_test.rs".to_owned(), 5);
    model.fuzzy_refresh();
    let fuzzy = model.overlays.fuzzy.as_ref().expect("fuzzy");
    let ranked = fuzzy
        .filtered
        .iter()
        .map(|index| fuzzy.entries[*index].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ranked,
        vec![
            "tests/main_test.rs",
            "src/main.rs",
            "deep/nested/main.rs",
            "src/domain.rs"
        ]
    );
}

#[test]
fn fuzzy_open_records_frecency() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("main.rs"), "fn main() {}").expect("file");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = dir.path().to_path_buf();
    model.overlays.fuzzy = Some(FuzzyState {
        generation: 0,
        query: String::new(),
        entries: vec!["main.rs".to_owned()],
        filtered: vec![0],
        selected: 0,
    });
    model.fuzzy_open();
    assert_eq!(model.workspace.recent.frecency.get("main.rs"), Some(&1));
}

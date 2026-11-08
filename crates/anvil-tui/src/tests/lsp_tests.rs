use super::*;
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
fn references_populate_and_jump_to_selection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let main = dir.path().join("main.rs");
    let other = dir.path().join("other.rs");
    fs::write(&main, "fn main() {}\n").expect("main");
    fs::write(&other, "fn target() {}\n").expect("other");
    let mut model = Model::open(&main).expect("open");
    let main_uri = lsp_worker::file_uri(&main);
    let other_uri = lsp_worker::file_uri(&other);
    model.apply_references_response(&serde_json::json!([
        {
            "uri": other_uri,
            "range": {
                "start": {"line": 0, "character": 3},
                "end": {"line": 0, "character": 9}
            }
        },
        {
            "uri": main_uri,
            "range": {
                "start": {"line": 0, "character": 3},
                "end": {"line": 0, "character": 7}
            }
        }
    ]));
    let references = model.references.as_ref().expect("references");
    assert_eq!(references.items.len(), 2);
    assert_eq!(references.items[0].path, main.canonicalize().expect("main"));
    assert_eq!(
        references.items[1].path,
        other.canonicalize().expect("other")
    );
    model.move_reference(1);
    model.apply_reference();
    assert_eq!(
        model.path.as_deref(),
        Some(other.canonicalize().expect("other").as_path())
    );
    assert_eq!(model.cursor, model.offset_at_utf16(0, 3));
    assert!(model.references.is_none());
}

#[test]
fn switching_buffers_clears_references() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "fn first() {}\n").expect("first");
    fs::write(&second, "fn second() {}\n").expect("second");
    let mut model = Model::open(&first).expect("open");
    model.open_path(&second).expect("second open");
    let uri = lsp_worker::file_uri(&second);
    model.apply_references_response(&serde_json::json!([{
        "uri": uri,
        "range": {
            "start": {"line": 0, "character": 3},
            "end": {"line": 0, "character": 9}
        }
    }]));
    assert!(model.references.is_some());
    model.switch_buffer(-1);
    assert!(model.references.is_none());
}

#[test]
fn document_highlights_ignore_stale_revisions() {
    let mut model = Model::new(EditorState::from_text("foo foo"), None);
    model.apply_document_highlights_response(&serde_json::json!([
        {
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 3}
            },
            "kind": 2
        },
        {
            "range": {
                "start": {"line": 0, "character": 4},
                "end": {"line": 0, "character": 7}
            },
            "kind": 3
        }
    ]));
    let highlights = model.document_highlights.as_ref().expect("highlights");
    assert_eq!(highlights.items.len(), 2);
    assert_eq!(highlights.items[1].kind, 3);
    assert_eq!(model.document_highlight_ranges(0, 7).len(), 2);
    model.insert_text("x");
    assert!(model.document_highlight_ranges(0, 8).is_empty());
}

#[test]
fn document_highlights_render_writes_underlined() {
    let mut spans = vec![ratatui::text::Span::raw("foo foo")];
    apply_document_highlights(&mut spans, 0, &[(0, 3, 2), (4, 7, 3)]);
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0].content.as_ref(), "foo");
    assert_eq!(spans[0].style.bg, Some(crate::theme::selection()));
    assert_eq!(spans[2].content.as_ref(), "foo");
    assert!(
        spans[2]
            .style
            .add_modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    );
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
        .any(|cell| cell.fg == theme::error());
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

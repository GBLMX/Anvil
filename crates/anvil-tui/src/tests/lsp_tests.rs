use super::*;
use anvil_lsp_client as lsp;
use anvil_session::provider::ProviderId;
#[test]
fn definition_response_moves_cursor() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "fn foo() {}\nfn main() { foo(); }\n").expect("write");
    let mut model = Model::open(&path).expect("open");
    let uri = anvil_lsp_client::file_uri(&path);
    model.apply_definition_response(&serde_json::json!([{
        "targetUri": uri,
        "targetSelectionRange": {
            "start": {"line": 1, "character": 3},
            "end": {"line": 1, "character": 7}
        }
    }]));
    assert_eq!(
        model.session.path.as_deref(),
        Some(path.canonicalize().expect("canonical").as_path())
    );
    assert_eq!(model.session.cursor, model.offset_at_utf16(1, 3));
}

#[test]
fn references_populate_and_jump_to_selection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let main = dir.path().join("main.rs");
    let other = dir.path().join("other.rs");
    fs::write(&main, "fn main() {}\n").expect("main");
    fs::write(&other, "fn target() {}\n").expect("other");
    let mut model = Model::open(&main).expect("open");
    let main_uri = anvil_lsp_client::file_uri(&main);
    let other_uri = anvil_lsp_client::file_uri(&other);
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
    let references = model
        .overlays
        .lsp_results
        .references
        .as_ref()
        .expect("references");
    assert_eq!(references.items.len(), 2);
    assert_eq!(references.items[0].path, main.canonicalize().expect("main"));
    assert_eq!(
        references.items[1].path,
        other.canonicalize().expect("other")
    );
    assert_eq!(model.top_overlay(), Some(OverlayKind::References));
    model.move_reference(1);
    model.apply_reference();
    assert!(model.overlays.stack.is_empty());
    assert_eq!(
        model.session.path.as_deref(),
        Some(other.canonicalize().expect("other").as_path())
    );
    assert_eq!(model.session.cursor, model.offset_at_utf16(0, 3));
    assert!(model.overlays.lsp_results.references.is_none());
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
    let uri = anvil_lsp_client::file_uri(&second);
    model.apply_references_response(&serde_json::json!([{
        "uri": uri,
        "range": {
            "start": {"line": 0, "character": 3},
            "end": {"line": 0, "character": 9}
        }
    }]));
    assert!(model.overlays.lsp_results.references.is_some());
    model.apply_call_hierarchy_calls_response(
        &serde_json::json!([{
            "from": {
                "name": "second",
                "uri": anvil_lsp_client::file_uri(&second),
                "selectionRange": {
                    "start": {"line": 0, "character": 3},
                    "end": {"line": 0, "character": 9}
                }
            }
        }]),
        crate::CallHierarchyDirection::Incoming,
    );
    assert!(model.overlays.lsp_results.call_hierarchy.is_some());
    model.switch_buffer(-1);
    assert!(model.overlays.lsp_results.references.is_none());
    assert!(model.overlays.lsp_results.call_hierarchy.is_none());
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
    let highlights = model
        .overlays
        .lsp_results
        .document_highlights
        .as_ref()
        .expect("highlights");
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
fn incoming_calls_populate_and_jump() {
    let dir = tempfile::tempdir().expect("tempdir");
    let main = dir.path().join("main.rs");
    let caller = dir.path().join("caller.rs");
    fs::write(&main, "fn main() {}\n").expect("main");
    fs::write(&caller, "fn caller() {}\n").expect("caller");
    let mut model = Model::open(&main).expect("open");
    let caller_uri = anvil_lsp_client::file_uri(&caller);
    model.apply_call_hierarchy_calls_response(
        &serde_json::json!([{
            "from": {
                "name": "caller",
                "uri": caller_uri,
                "selectionRange": {
                    "start": {"line": 0, "character": 3},
                    "end": {"line": 0, "character": 9}
                }
            }
        }]),
        crate::CallHierarchyDirection::Incoming,
    );
    let calls = model
        .overlays
        .lsp_results
        .call_hierarchy
        .as_ref()
        .expect("calls");
    assert_eq!(calls.items.len(), 1);
    assert_eq!(calls.items[0].name, "caller");
    model.apply_call_hierarchy();
    assert_eq!(
        model.session.path.as_deref(),
        Some(caller.canonicalize().expect("canonical").as_path())
    );
    assert_eq!(model.session.cursor, model.offset_at_utf16(0, 3));
    assert!(model.overlays.lsp_results.call_hierarchy.is_none());
}

#[test]
fn outgoing_calls_parse_to_item() {
    let mut model = Model::new(EditorState::from_text("caller"), None);
    let target = std::path::Path::new("/tmp/outgoing.rs");
    model.apply_call_hierarchy_calls_response(
        &serde_json::json!([{
            "to": {
                "name": "callee",
                "uri": anvil_lsp_client::file_uri(target),
                "selectionRange": {
                    "start": {"line": 0, "character": 3},
                    "end": {"line": 0, "character": 9}
                }
            }
        }]),
        crate::CallHierarchyDirection::Outgoing,
    );
    let calls = model
        .overlays
        .lsp_results
        .call_hierarchy
        .as_ref()
        .expect("calls");
    assert_eq!(calls.direction, crate::CallHierarchyDirection::Outgoing);
    assert_eq!(calls.items[0].name, "callee");
}

#[test]
#[ignore = "requires rust-analyzer and ANVIL_GATEWAY"]
fn call_hierarchy_round_trip_through_gateway() {
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().canonicalize().expect("canonical");
    let source = workspace.join("src");
    fs::create_dir(&source).expect("src");
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"call-smoke\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("manifest");
    let main = source.join("main.rs");
    fs::write(
        &main,
        "fn callee() {}\nfn caller() { callee(); }\nfn main() { caller(); }\n",
    )
    .expect("main");
    let (session, gateway, lsp_client) =
        crate::gateway::GatewaySession::start(&workspace).expect("gateway");
    let mut model = Model::with_gateway(gateway, workspace);
    model.language.lsp.client = Some(lsp_client);
    model.open_path(&main).expect("open");
    std::thread::sleep(Duration::from_secs(3));

    let wait = |model: &mut Model, direction: crate::CallHierarchyDirection, expected: &str| {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            model.poll_lsp();
            if let Some(calls) = &model.overlays.lsp_results.call_hierarchy {
                if calls.direction == direction
                    && calls.items.iter().any(|item| item.name == expected)
                {
                    return;
                }
            }
            assert!(
                Instant::now() < deadline,
                "no {direction:?} call to {expected}; status={}; pending={:?}; notifications={:?}",
                model.status.text,
                model.language.lsp_requests.pending,
                model.ui.notifications
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    };

    model.session.cursor = model.offset_at_utf16(1, 15);
    model.request_call_hierarchy(crate::CallHierarchyDirection::Incoming);
    wait(
        &mut model,
        crate::CallHierarchyDirection::Incoming,
        "caller",
    );

    model.session.cursor = model.offset_at_utf16(1, 3);
    model.request_call_hierarchy(crate::CallHierarchyDirection::Outgoing);
    wait(
        &mut model,
        crate::CallHierarchyDirection::Outgoing,
        "callee",
    );
    drop(session);
}

#[test]
#[ignore = "requires rust-analyzer and ANVIL_GATEWAY"]
fn folding_round_trip_through_gateway() {
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().canonicalize().expect("canonical");
    let source = workspace.join("src");
    fs::create_dir(&source).expect("src");
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"fold-smoke\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("manifest");
    let main = source.join("main.rs");
    fs::write(
        &main,
        "fn outer() {\n    if true {\n        let x = 1;\n        println!(\"{x}\");\n    }\n}\nfn main() { outer(); }\n",
    )
    .expect("main");
    let (session, gateway, lsp_client) =
        crate::gateway::GatewaySession::start(&workspace).expect("gateway");
    let mut model = Model::with_gateway(gateway, workspace);
    model.language.lsp.client = Some(lsp_client);
    model.open_path(&main).expect("open");
    std::thread::sleep(Duration::from_secs(3));
    model.request_folding_ranges(None);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        model.poll_lsp();
        if !model.overlays.lsp_results.folds.is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no folding ranges; status={}",
            model.status.text
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    model.close_all_folds();
    assert!(model.has_closed_folds());
    drop(session);
}

#[test]
#[ignore = "requires rust-analyzer and ANVIL_GATEWAY"]
fn completion_resolve_round_trip_through_gateway() {
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().canonicalize().expect("canonical");
    let source = workspace.join("src");
    fs::create_dir(&source).expect("src");
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"completion-smoke\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("manifest");
    let main = source.join("main.rs");
    fs::write(&main, "fn main() {\n    let value = 1;\n    val\n}\n").expect("main");
    let (session, gateway, lsp_client) =
        crate::gateway::GatewaySession::start(&workspace).expect("gateway");
    let mut model = Model::with_gateway(gateway, workspace);
    model.language.lsp.client = Some(lsp_client);
    model.open_path(&main).expect("open");
    std::thread::sleep(Duration::from_secs(3));
    model.session.cursor = model.offset_at_utf16(2, 7);
    model.request_completion();

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        model.poll_lsp();
        let resolved = model
            .overlays
            .lsp_results
            .completion
            .as_ref()
            .and_then(|completion| completion.filtered.get(completion.selected))
            .and_then(|index| {
                model
                    .overlays
                    .lsp_results
                    .completion
                    .as_ref()?
                    .items
                    .get(*index)
            })
            .is_some_and(|item| item.resolved);
        if resolved {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "completion resolve did not finish; status={}",
            model.status.text
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(session);
}

#[test]
#[ignore = "requires rust-analyzer and ANVIL_GATEWAY"]
fn semantic_tokens_round_trip_through_gateway() {
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().canonicalize().expect("canonical");
    let source = workspace.join("src");
    fs::create_dir(&source).expect("src");
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"semantic-smoke\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("manifest");
    let main = source.join("main.rs");
    fs::write(&main, "fn main() {\n    let value = 1;\n}\n").expect("main");
    let (session, gateway, lsp_client) =
        crate::gateway::GatewaySession::start(&workspace).expect("gateway");
    let mut model = Model::with_gateway(gateway, workspace);
    model.language.lsp.client = Some(lsp_client);
    model.open_path(&main).expect("open");
    std::thread::sleep(Duration::from_secs(3));
    model.poll_lsp();
    assert!(!model.overlays.lsp_results.semantic_legend.is_empty());
    model.request_semantic_tokens();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        model.poll_lsp();
        if !model.overlays.lsp_results.semantic_tokens.is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no semantic tokens; status={}",
            model.status.text
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!model.semantic_ranges(0, 40).is_empty());
    drop(session);
}

#[test]
fn gi_and_go_dispatch_call_hierarchy_requests() {
    use crossterm::event::Event;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "fn main() {}\n").expect("main");
    let mut model = Model::open(&path).expect("open");
    let press = |model: &mut Model, code| {
        model.dispatch(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    };
    press(&mut model, KeyCode::Char('g'));
    press(&mut model, KeyCode::Char('i'));
    assert_eq!(model.status.text, "E: lsp is not running");
    press(&mut model, KeyCode::Char('g'));
    press(&mut model, KeyCode::Char('o'));
    assert_eq!(model.status.text, "E: lsp is not running");
}

#[test]
fn path_completion_uses_gateway_directory_entries() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().canonicalize().expect("canonical");
    let src = workspace.join("src");
    fs::create_dir(&src).expect("src");
    fs::write(src.join("main.rs"), "fn main() {}\n").expect("main");
    fs::write(src.join("model.rs"), "struct Model;\n").expect("model");
    let (session, gateway, _) = crate::gateway::GatewaySession::start(&workspace).expect("gateway");
    let mut model = Model::with_gateway(gateway, workspace);
    model.session.path = Some(src.join("main.rs"));
    model.session.editor = EditorState::from_text("./mo").into();
    model.session.cursor = model.session.editor.text().len_bytes();
    model.request_completion();
    let completion = model
        .overlays
        .lsp_results
        .completion
        .as_ref()
        .expect("completion");
    let index = completion
        .items
        .iter()
        .position(|item| item.label == "./model.rs")
        .expect("path item");
    assert_eq!(completion.items[index].source.as_str(), "builtin.paths");
    model
        .overlays
        .lsp_results
        .completion
        .as_mut()
        .expect("completion")
        .selected = index;
    model.apply_completion();
    let text = model
        .session
        .editor
        .text()
        .slice(0..model.session.editor.text().len_bytes());
    assert_eq!(text, "./model.rs");
    drop(session);
}

#[test]
fn buffer_words_source_populates_local_completion() {
    let mut model = Model::new(EditorState::from_text("value value\nval"), None);
    model.session.cursor = model.session.editor.text().len_bytes();
    model.request_completion();
    let completion = model
        .overlays
        .lsp_results
        .completion
        .as_ref()
        .expect("completion");
    let index = completion
        .items
        .iter()
        .position(|item| item.label == "value")
        .expect("buffer word");
    assert_eq!(
        completion.items[index].source.as_str(),
        "builtin.buffer-words"
    );
    model
        .overlays
        .lsp_results
        .completion
        .as_mut()
        .expect("completion")
        .selected = index;
    model.apply_completion();
    let text = model
        .session
        .editor
        .text()
        .slice(0..model.session.editor.text().len_bytes());
    assert_eq!(text, "value value\nvalue");
}

#[test]
fn local_and_lsp_completions_merge_without_duplicates() {
    let mut model = Model::new(EditorState::from_text("value value\nval"), None);
    model.session.cursor = model.session.editor.text().len_bytes();
    model.request_completion();
    model
        .language
        .lsp_requests
        .pending
        .insert(20, LspRequestKind::Completion);
    model.handle_lsp_response(
        20,
        serde_json::json!({
            "isIncomplete": false,
            "items": [
                {"label": "value", "insertText": "value"},
                {"label": "lsp_only", "insertText": "lsp_only"}
            ]
        }),
        None,
    );
    let completion = model
        .overlays
        .lsp_results
        .completion
        .as_ref()
        .expect("completion");
    let labels: Vec<_> = completion
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    assert_eq!(labels.iter().filter(|label| **label == "value").count(), 1);
    assert!(labels.contains(&"lsp_only"));
}
#[test]
fn completion_response_uses_and_closes_the_overlay_stack() {
    let mut model = Model::new(EditorState::from_text("al"), None);
    model
        .language
        .lsp_requests
        .pending
        .insert(19, LspRequestKind::Completion);
    model.handle_lsp_response(
        19,
        serde_json::json!({
            "isIncomplete": false,
            "items": [{"label": "alpha", "kind": 3, "insertText": "alpha"}]
        }),
        None,
    );
    assert!(model.overlays.lsp_results.completion.is_some());
    assert_eq!(model.top_overlay(), Some(OverlayKind::Completion));

    handle_event(&mut model, key(KeyCode::Esc));
    assert!(model.overlays.lsp_results.completion.is_none());
    assert!(model.overlays.stack.is_empty());
}

#[test]
fn cancelling_completion_drops_pending_response() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    model.overlays.lsp_results.completion = Some(CompletionState {
        id: 9,
        items: vec![CompletionItem {
            source: ProviderId::new("builtin.test"),
            label: "item".to_owned(),
            filter_text: "item".to_owned(),
            sort_text: "item".to_owned(),
            preselect: false,
            insert: "item".to_owned(),
            snippet: None,
            detail: String::new(),
            replace: None,
            replace_bytes: None,
            documentation: None,
            raw: serde_json::Value::Null,
            resolved: false,
        }],
        filtered: vec![0],
        query: String::new(),
        selected: 0,
    });
    model
        .language
        .lsp_requests
        .pending
        .insert(9, LspRequestKind::Completion);
    model.cancel_completion();
    assert!(model.overlays.lsp_results.completion.is_none());
    assert!(!model.language.lsp_requests.pending.contains_key(&9));
    model.handle_lsp_response(9, serde_json::json!({"items": []}), None);
    assert!(model.overlays.lsp_results.completion.is_none());
    model
        .language
        .lsp_requests
        .pending
        .insert(10, LspRequestKind::Completion);
    model.cancel_completion();
    assert!(!model.language.lsp_requests.pending.contains_key(&10));
    model.handle_lsp_response(10, serde_json::json!({"items": []}), None);
    assert!(model.overlays.lsp_results.completion.is_none());
}

#[test]
fn completion_resolve_updates_selected_item() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    model.overlays.lsp_results.completion = Some(CompletionState {
        id: 9,
        items: vec![CompletionItem {
            source: ProviderId::new("builtin.test"),
            label: "item".to_owned(),
            filter_text: "item".to_owned(),
            sort_text: "item".to_owned(),
            preselect: false,
            insert: "item".to_owned(),
            snippet: None,
            detail: String::new(),
            replace: None,
            replace_bytes: None,
            documentation: None,
            raw: serde_json::json!({"label": "item"}),
            resolved: false,
        }],
        filtered: vec![0],
        query: String::new(),
        selected: 0,
    });
    model
        .language
        .lsp_requests
        .pending
        .insert(7, LspRequestKind::CompletionResolve);
    model.overlays.lsp_results.completion_resolve = Some((7, 0));
    model.handle_lsp_response(
        7,
        serde_json::json!({
            "label": "item",
            "detail": "resolved detail",
            "documentation": {"kind": "markdown", "value": "resolved docs"}
        }),
        None,
    );
    let item = &model
        .overlays
        .lsp_results
        .completion
        .as_ref()
        .expect("completion")
        .items[0];
    assert_eq!(item.detail, "resolved detail");
    assert_eq!(item.documentation.as_deref(), Some("resolved docs"));
    assert!(item.resolved);
    assert!(model.overlays.lsp_results.completion_resolve.is_none());
}

#[test]
fn completion_documentation_is_rendered() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    model.overlays.lsp_results.completion = Some(CompletionState {
        id: 9,
        items: vec![CompletionItem {
            source: ProviderId::new("builtin.test"),
            label: "item".to_owned(),
            filter_text: "item".to_owned(),
            sort_text: "item".to_owned(),
            preselect: false,
            insert: "item".to_owned(),
            snippet: None,
            detail: "detail".to_owned(),
            replace: None,
            replace_bytes: None,
            documentation: Some("resolved docs".to_owned()),
            raw: serde_json::json!({"label": "item"}),
            resolved: true,
        }],
        filtered: vec![0],
        query: String::new(),
        selected: 0,
    });
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
    assert!(rendered.contains("resolved docs"));
}

#[test]
fn applying_snippet_selects_and_navigates_stops() {
    let mut model = Model::new(EditorState::from_text(""), None);
    let snippet = crate::snippet::parse("fn ${1:name}(${2:args}) $0").expect("snippet");
    model.overlays.lsp_results.completion = Some(CompletionState {
        id: 9,
        items: vec![CompletionItem {
            source: ProviderId::new("builtin.test"),
            label: "fn".to_owned(),
            filter_text: "fn".to_owned(),
            sort_text: "fn".to_owned(),
            preselect: false,
            insert: snippet.text.clone(),
            snippet: Some(snippet),
            detail: String::new(),
            replace: None,
            replace_bytes: None,
            documentation: None,
            raw: serde_json::Value::Null,
            resolved: true,
        }],
        filtered: vec![0],
        query: String::new(),
        selected: 0,
    });
    model.apply_completion();
    assert_eq!(
        model
            .session
            .editor
            .text()
            .slice(0..model.session.editor.text().len_bytes()),
        "fn name(args) "
    );
    assert_eq!(model.session.editor.selection_bytes(), vec![(3, 7)]);
    assert!(model.next_snippet_stop());
    assert_eq!(model.session.editor.selection_bytes(), vec![(8, 12)]);
    assert!(model.next_snippet_stop());
    assert_eq!(model.session.editor.selection_bytes(), vec![(14, 14)]);
    assert!(model.next_snippet_stop());
    assert!(model.overlays.lsp_results.snippet.is_none());
}

#[test]
fn mirrored_snippet_edits_apply_to_all_tabstops() {
    let mut model = Model::new(EditorState::from_text(""), None);
    let snippet = crate::snippet::parse("${1:name} + ${1:name}").expect("snippet");
    model.overlays.lsp_results.completion = Some(CompletionState {
        id: 9,
        items: vec![CompletionItem {
            source: ProviderId::new("builtin.test"),
            label: "mirror".to_owned(),
            filter_text: "mirror".to_owned(),
            sort_text: "mirror".to_owned(),
            preselect: false,
            insert: snippet.text.clone(),
            snippet: Some(snippet),
            detail: String::new(),
            replace: None,
            replace_bytes: None,
            documentation: None,
            raw: serde_json::Value::Null,
            resolved: true,
        }],
        filtered: vec![0],
        query: String::new(),
        selected: 0,
    });
    model.apply_completion();
    assert_eq!(
        model.session.editor.selection_bytes(),
        vec![(0, 4), (7, 11)]
    );
    model.insert_text("x");
    assert_eq!(model.session.editor.text().slice(0..5), "x + x");
    assert_eq!(model.session.editor.selection_bytes(), vec![(1, 1), (5, 5)]);
    model.insert_text("y");
    assert_eq!(model.session.editor.text().slice(0..7), "xy + xy");
    assert_eq!(model.session.editor.selection_bytes(), vec![(2, 2), (7, 7)]);
    model.backspace();
    assert_eq!(model.session.editor.text().slice(0..5), "x + x");
    assert_eq!(model.session.editor.selection_bytes(), vec![(1, 1), (5, 5)]);
    model.backspace();
    assert_eq!(model.session.editor.text().slice(0..3), " + ");
    assert_eq!(model.session.editor.selection_bytes(), vec![(0, 0), (3, 3)]);
}

#[test]
fn rename_response_applies_workspace_edit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("rename.rs");
    fs::write(&file, "foo").expect("file");
    let mut model = Model::open(&file).expect("open");
    let uri = anvil_lsp_client::file_uri(&file);
    model
        .language
        .lsp_requests
        .pending
        .insert(7, LspRequestKind::Rename);
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
    assert_eq!(model.session.editor.text().slice(0..3), "bar");
    assert!(model.file.modified);
}

#[test]
fn code_action_response_applies_workspace_edit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("action.rs");
    fs::write(&file, "bad").expect("file");
    let mut model = Model::open(&file).expect("open");
    let uri = anvil_lsp_client::file_uri(&file);
    model
        .language
        .lsp_requests
        .pending
        .insert(8, LspRequestKind::CodeAction);
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
    assert!(model.overlays.lsp_results.code_actions.is_some());
    model.apply_code_action();
    assert_eq!(model.session.editor.text().slice(0..4), "good");
    assert!(model.overlays.lsp_results.code_actions.is_none());
}

#[test]
fn cancelling_code_action_drops_pending_response() {
    let mut model = Model::new(EditorState::from_text("text"), Some(PathBuf::from("a.rs")));
    model
        .language
        .lsp_requests
        .pending
        .insert(10, LspRequestKind::CodeAction);
    model.cancel_code_actions();
    assert!(!model.language.lsp_requests.pending.contains_key(&10));
    model.handle_lsp_response(10, serde_json::json!([{"title": "late"}]), None);
    assert!(model.overlays.lsp_results.code_actions.is_none());
}

#[test]
fn gopls_code_action_response_applies_organize_imports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.go");
    fs::write(&file, "package main\n\nimport \"fmt\"\n\nfunc main() {}\n").expect("file");
    let mut model = Model::open(&file).expect("open");
    let uri = anvil_lsp_client::file_uri(&file);
    model
        .language
        .lsp_requests
        .pending
        .insert(9, LspRequestKind::CodeAction);
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
    let actions = model
        .overlays
        .lsp_results
        .code_actions
        .as_ref()
        .expect("actions");
    assert_eq!(actions.items.len(), 2);
    assert!(actions.items[0].edit.is_some());
    model.apply_code_action();
    assert!(
        !model
            .session
            .editor
            .text()
            .slice(0..model.session.editor.text().len_bytes())
            .contains("fmt")
    );
    assert!(model.file.modified);
}

#[test]
fn document_symbols_populate_and_jump_to_selection() {
    let file = PathBuf::from("symbols.rs");
    let mut model = Model::new(
        EditorState::from_text("fn alpha() {\n    fn inner() {}\n}\nfn beta() {}\n"),
        Some(file),
    );
    model
        .language
        .lsp_requests
        .pending
        .insert(11, LspRequestKind::DocumentSymbol);
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
    let symbols = model
        .overlays
        .lsp_results
        .document_symbols
        .as_ref()
        .expect("symbols");
    assert_eq!(symbols.items.len(), 3);
    assert_eq!(symbols.items[1].depth, 1);
    assert_eq!(symbols.items[2].line, 3);
    model.apply_document_symbol();
    assert_eq!(model.session.cursor, 3);
    assert!(model.status.text.contains("alpha"));
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
    model
        .language
        .lsp_requests
        .pending
        .insert(14, LspRequestKind::DocumentSymbol);
    model.handle_lsp_response(14, serde_json::json!([value]), None);
    let symbols = model
        .overlays
        .lsp_results
        .document_symbols
        .as_ref()
        .expect("symbols");
    assert!(symbols.items.len() <= 65);
}

#[test]
fn formatting_response_applies_edits_without_writing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("format.rs");
    fs::write(&file, "fn main(){}").expect("file");
    let mut model = Model::open(&file).expect("open");
    model
        .language
        .lsp_requests
        .pending
        .insert(12, LspRequestKind::Formatting);
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
    assert_eq!(model.session.editor.text().slice(0..2), "//");
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
    let first_path = model.session.path.clone().expect("first path");
    model
        .language
        .lsp_requests
        .pending_edit_path
        .insert(13, first_path);
    model
        .language
        .lsp_requests
        .pending
        .insert(13, LspRequestKind::Formatting);
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
    assert_eq!(model.session.editor.text().slice(0..9), "fn first(");
    assert!(model.status.text.contains("inactive"));
}

#[test]
fn workspace_symbols_response_filters_and_jumps() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("symbols.rs");
    fs::write(&file, "fn alpha() {}\n").expect("file");
    let mut model = Model::open(&file).expect("open");
    let uri = anvil_lsp_client::file_uri(&file);
    model
        .language
        .lsp_requests
        .pending
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
    let symbols = model
        .overlays
        .lsp_results
        .workspace_symbols
        .as_ref()
        .expect("symbols");
    assert_eq!(symbols.items.len(), 2);
    assert_eq!(model.top_overlay(), Some(OverlayKind::WorkspaceSymbols));
    model.filter_workspace_symbols();
    assert_eq!(
        model
            .overlays
            .lsp_results
            .workspace_symbols
            .as_ref()
            .expect("symbols")
            .filtered
            .len(),
        2
    );
    model.apply_workspace_symbol();
    assert_eq!(model.session.cursor, 3);
    assert!(model.status.text.contains("alpha"));
    assert!(model.overlays.stack.is_empty());
}

#[test]
fn signature_help_response_populates_active_signature() {
    let mut model = Model::new(
        EditorState::from_text("fn main() {}"),
        Some(PathBuf::from("sig.rs")),
    );
    model
        .language
        .lsp_requests
        .pending
        .insert(17, LspRequestKind::SignatureHelp);
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
    let signature = model
        .overlays
        .lsp_results
        .signature_help
        .as_ref()
        .expect("signature");
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
    model
        .language
        .lsp_requests
        .pending
        .insert(15, LspRequestKind::Formatting);
    model
        .language
        .lsp_requests
        .pending_edit_revision
        .insert(15, model.session.editor.revision());
    model.session.editor.insert(0, "x");
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
    assert_eq!(model.session.editor.text().slice(0..3), "xfn");
    assert!(model.status.text.contains("stale"));
}

#[test]
fn rename_response_for_inactive_path_is_ignored() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "fn first(){}").expect("first");
    let mut model = Model::open(&first).expect("open");
    let first_path = model.session.path.clone().expect("first path");
    let uri = anvil_lsp_client::file_uri(&first_path);
    model
        .language
        .lsp_requests
        .pending_edit_path
        .insert(16, first_path);
    model
        .language
        .lsp_requests
        .pending_edit_revision
        .insert(16, model.session.editor.revision());
    model
        .language
        .lsp_requests
        .pending
        .insert(16, LspRequestKind::Rename);
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
    assert_eq!(model.session.editor.text().slice(0..9), "fn first(");
    assert!(model.status.text.contains("inactive"));
}

#[test]
fn diagnostic_jump_cycles_between_lines() {
    let mut model = Model::new(
        EditorState::from_text("one\ntwo\nthree"),
        Some(PathBuf::from("diag.rs")),
    );
    model.language.diagnostics.by_path.insert(
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
    assert_eq!(model.session.cursor, model.offset_at_utf16(2, 0));
    assert!(
        model
            .ui
            .diagnostic_peek
            .as_ref()
            .is_some_and(|peek| peek.diagnostics[0].message.contains("second"))
    );
    model.jump_diagnostic(false);
    assert_eq!(model.session.cursor, 0);
    assert!(
        model
            .ui
            .diagnostic_peek
            .as_ref()
            .is_some_and(|peek| peek.diagnostics[0].message.contains("first"))
    );
    model.jump_diagnostic(false);
    assert_eq!(model.session.cursor, model.offset_at_utf16(2, 0));
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
    model.language.diagnostics.by_path.insert(
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
fn hover_response_dismisses_cleanly_from_the_stack() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model
        .language
        .lsp_requests
        .pending
        .insert(17, LspRequestKind::Hover);
    model.handle_lsp_response(
        17,
        serde_json::json!({"contents": {"kind": "plaintext", "value": "docs"}}),
        None,
    );
    assert!(model.overlays.lsp_results.hover.is_some());
    assert_eq!(model.top_overlay(), Some(OverlayKind::Hover));

    handle_event(&mut model, key(KeyCode::Char('i')));
    assert!(model.overlays.lsp_results.hover.is_none());
    assert!(model.overlays.stack.is_empty());
    assert!(matches!(model.ui.input_mode, InputMode::Insert));

    handle_event(&mut model, key(KeyCode::Esc));
    assert!(model.overlays.stack.is_empty());
}

#[test]
fn diagnostic_peek_unknown_key_cancels_and_passes_through() {
    let mut model = Model::new(
        EditorState::from_text("one"),
        Some(PathBuf::from("diag.rs")),
    );
    model
        .language
        .diagnostics
        .by_path
        .insert(PathBuf::from("diag.rs"), vec![diagnostic(0, 0, "problem")]);
    model.jump_diagnostic(true);
    assert!(model.ui.diagnostic_peek.is_some());

    handle_event(&mut model, key(KeyCode::Char('i')));
    assert!(model.ui.diagnostic_peek.is_none());
    assert!(matches!(model.ui.input_mode, InputMode::Insert));
}

#[test]
fn diagnostic_peek_clears_on_revision_change() {
    let mut model = Model::new(
        EditorState::from_text("one"),
        Some(PathBuf::from("diag.rs")),
    );
    model
        .language
        .diagnostics
        .by_path
        .insert(PathBuf::from("diag.rs"), vec![diagnostic(0, 0, "first")]);
    model.jump_diagnostic(true);
    assert!(model.ui.diagnostic_peek.is_some());
    model.session.editor.insert(0, "x");
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
        .language
        .diagnostics
        .by_path
        .insert(PathBuf::from("diag.rs"), vec![diagnostic(0, 0, &message)]);
    model.view.viewport_height = 10;
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
        .language
        .diagnostics
        .by_path
        .insert(PathBuf::from("diag.rs"), vec![diagnostic(1, 1, "problem")]);
    model.rebuild_problems();
    model.ui.problems.visible = true;
    model.ui.focus.tree = true;
    model.jump_problem();
    assert_eq!(model.session.cursor, model.offset_at_utf16(1, 1));
    assert!(!model.ui.focus.tree);
    assert!(!model.ui.problems.visible);
}

#[test]
fn diagnostics_underline_matching_line() {
    let mut model = Model::new(
        EditorState::from_text("first\nsecond"),
        Some(PathBuf::from("diag.rs")),
    );
    model.language.diagnostics.by_path.insert(
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
    model.session.cursor = "a👨‍👩‍👧‍👦".len();
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
fn disabled_inlay_hints_are_not_requested() {
    let mut model = Model::new(EditorState::from_text("x"), Some(PathBuf::from("main.rs")));
    let key = ("rust-analyzer".to_owned(), "rust".to_owned());
    model.language.lsp.config = Some(key.clone());
    model.overlays.lsp_results.inlay_support.insert(key, true);
    model.config.settings.lsp.inlay_hints = false;
    model.poll_inlay_hints(10);
    assert!(model.overlays.lsp_results.inlay_pending.is_none());
    assert!(model.overlays.lsp_results.inlay_hints.is_empty());
}

#[test]
fn rename_response_rejects_a_stale_inactive_buffer_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "first").expect("first");
    fs::write(&second, "second").expect("second");
    let mut model = Model::open(&first).expect("open first");
    model.open_path(&second).expect("open second");
    let second_path = model.session.path.clone().expect("second path");
    model.switch_buffer(-1);
    let mut base = HashMap::new();
    let second_buffer = model
        .buffers
        .entries
        .iter_mut()
        .find(|buffer| buffer.path.as_ref() == Some(&second_path))
        .expect("second buffer");
    base.insert(second_path.clone(), second_buffer.editor.revision());
    second_buffer.editor.insert(0, "stale");
    model
        .language
        .lsp_requests
        .pending_edit_snapshot
        .insert(17, base);
    model
        .language
        .lsp_requests
        .pending
        .insert(17, LspRequestKind::Rename);
    let first_uri = anvil_lsp_client::file_uri(&first);
    model.handle_lsp_response(
        17,
        serde_json::json!({
            "changes": {
                first_uri: [{
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 0}
                    },
                    "newText": "X"
                }]
            }
        }),
        None,
    );
    assert_eq!(model.session.editor.text().slice(0..5), "first");
    assert!(model.status.text.contains("stale"));
}

#[test]
fn rename_response_rejects_a_stale_document_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("versioned.rs");
    fs::write(&file, "abc").expect("file");
    let mut model = Model::open(&file).expect("open");
    let path = model.session.path.clone().expect("path");
    model.language.lsp_requests.versions.insert(path.clone(), 3);
    model
        .language
        .lsp_requests
        .pending
        .insert(18, LspRequestKind::Rename);
    model.handle_lsp_response(
        18,
        serde_json::json!({
            "documentChanges": [{
                "textDocument": {"uri": anvil_lsp_client::file_uri(&path), "version": 2},
                "edits": [{
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 3}
                    },
                    "newText": "changed"
                }]
            }]
        }),
        None,
    );
    assert_eq!(model.session.editor.text().slice(0..3), "abc");
    assert!(model.status.text.contains("stale"));
}

#[test]
fn completion_uses_sort_text_preselect_and_filter_text() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    model
        .language
        .lsp_requests
        .pending
        .insert(21, LspRequestKind::Completion);
    model.handle_lsp_response(
        21,
        serde_json::json!({"items": [
            {"label": "zebra", "sortText": "2", "filterText": "zzz"},
            {"label": "alpha", "sortText": "1", "filterText": "alphabet", "preselect": true},
            {"label": "second", "sortText": "10", "filterText": "second"}
        ]}),
        None,
    );
    let completion = model
        .overlays
        .lsp_results
        .completion
        .as_ref()
        .expect("completion");
    assert_eq!(
        completion
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "second", "zebra"]
    );
    assert_eq!(completion.selected, 0);

    model
        .overlays
        .lsp_results
        .completion
        .as_mut()
        .expect("completion")
        .query = "bet".to_owned();
    model.filter_completion();
    let completion = model
        .overlays
        .lsp_results
        .completion
        .as_ref()
        .expect("completion");
    assert_eq!(completion.filtered.len(), 1);
    assert_eq!(completion.items[completion.filtered[0]].label, "alpha");
}

#[test]
fn definition_jump_back_and_forward_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let main = dir.path().join("main.rs");
    let other = dir.path().join("other.rs");
    fs::write(&main, "fn main() {}\n").expect("main");
    fs::write(&other, "fn target() {}\n").expect("other");
    let mut model = Model::open(&main).expect("open");
    let main = main.canonicalize().expect("main");
    let other = other.canonicalize().expect("other");
    let uri = anvil_lsp_client::file_uri(&other);

    model.apply_definition_response(&serde_json::json!([{
        "targetUri": uri,
        "targetSelectionRange": {
            "start": {"line": 0, "character": 3},
            "end": {"line": 0, "character": 9}
        }
    }]));
    assert_eq!(model.session.path.as_deref(), Some(other.as_path()));
    let target_cursor = model.session.cursor;

    model.jump_back();
    assert_eq!(model.session.path.as_deref(), Some(main.as_path()));
    assert_eq!(model.session.cursor, 0);

    model.jump_forward();
    assert_eq!(model.session.path.as_deref(), Some(other.as_path()));
    assert_eq!(model.session.cursor, target_cursor);
}

#[test]
fn jump_back_without_history_reports_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("main.rs");
    fs::write(&path, "fn main() {}\n").expect("write");
    let mut model = Model::open(&path).expect("open");
    model.jump_back();
    assert!(model.status.text.contains("no back jump"));
}

#[test]
fn jump_back_from_help_returns_to_history_target() {
    let dir = tempfile::tempdir().expect("tempdir");
    let main = dir.path().join("main.rs");
    let other = dir.path().join("other.rs");
    fs::write(&main, "fn main() {}\n").expect("main");
    fs::write(&other, "fn target() {}\n").expect("other");
    let mut model = Model::open(&main).expect("open");
    let main = main.canonicalize().expect("main");
    let uri = anvil_lsp_client::file_uri(&other);

    model.apply_definition_response(&serde_json::json!([{
        "targetUri": uri,
        "targetSelectionRange": {
            "start": {"line": 0, "character": 3},
            "end": {"line": 0, "character": 9}
        }
    }]));
    assert_eq!(
        model.session.path.as_deref(),
        Some(other.canonicalize().expect("other").as_path())
    );

    model.show_help();
    assert!(model.session.path.is_none());

    model.jump_back();
    assert_eq!(model.session.path.as_deref(), Some(main.as_path()));
    assert_eq!(model.session.cursor, 0);
}

#[test]
fn jump_back_refuses_to_discard_a_dirty_unnamed_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let main = dir.path().join("main.rs");
    let other = dir.path().join("other.rs");
    fs::write(&main, "fn main() {}\n").expect("main");
    fs::write(&other, "fn target() {}\n").expect("other");
    let mut model = Model::open(&main).expect("open");
    let uri = anvil_lsp_client::file_uri(&other);

    model.apply_definition_response(&serde_json::json!([{
        "targetUri": uri,
        "targetSelectionRange": {
            "start": {"line": 0, "character": 3},
            "end": {"line": 0, "character": 9}
        }
    }]));
    model.session.path = None;
    model.file.modified = true;
    let before = model
        .session
        .editor
        .text()
        .slice(0..model.session.editor.text().len_bytes());

    model.jump_back();

    assert!(model.session.path.is_none());
    assert!(model.file.modified);
    assert_eq!(
        model
            .session
            .editor
            .text()
            .slice(0..model.session.editor.text().len_bytes()),
        before
    );
    assert!(model.status.text.contains("save or discard"));
}

#[test]
fn workspace_symbol_keys_filter_and_pass_through() {
    let mut model = Model::new(
        EditorState::from_text("x"),
        Some(std::path::PathBuf::from("symbols.rs")),
    );
    model.start_page.welcome = false;
    model.session.cursor = 1;
    model.overlays.lsp_results.workspace_symbols = Some(WorkspaceSymbolState {
        items: vec![
            WorkspaceSymbolItem {
                name: "alpha".to_owned(),
                detail: String::new(),
                path: std::path::PathBuf::from("symbols.rs"),
                line: 0,
                character: 0,
            },
            WorkspaceSymbolItem {
                name: "beta".to_owned(),
                detail: String::new(),
                path: std::path::PathBuf::from("symbols.rs"),
                line: 0,
                character: 0,
            },
        ],
        filtered: vec![0, 1],
        query: String::new(),
        selected: 0,
    });

    handle_event(&mut model, key(KeyCode::Char('e')));
    handle_event(&mut model, key(KeyCode::Char('t')));
    let symbols = model
        .overlays
        .lsp_results
        .workspace_symbols
        .as_ref()
        .expect("symbols");
    assert_eq!(symbols.query, "et");
    assert_eq!(symbols.filtered, vec![1]);

    handle_event(&mut model, key(KeyCode::Home));
    assert!(model.overlays.lsp_results.workspace_symbols.is_none());
    assert_eq!(model.session.cursor, 0);
}

#[test]
fn completion_unknown_key_cancels_and_passes_through() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    model.ui.input_mode = InputMode::Insert;
    model.session.cursor = 1;
    model.overlays.lsp_results.completion = Some(CompletionState {
        id: 1,
        items: Vec::new(),
        filtered: Vec::new(),
        query: String::new(),
        selected: 0,
    });

    handle_event(&mut model, key(KeyCode::Home));
    assert!(model.overlays.lsp_results.completion.is_none());
    assert_eq!(model.session.cursor, 0);
}

#[test]
fn completion_char_and_backspace_update_buffer_and_query() {
    let mut model = Model::new(EditorState::from_text("x"), None);
    model.ui.input_mode = InputMode::Insert;
    model.overlays.lsp_results.completion = Some(CompletionState {
        id: 1,
        items: vec![CompletionItem {
            source: ProviderId::new("builtin.test"),
            label: "item".to_owned(),
            filter_text: "item".to_owned(),
            sort_text: "item".to_owned(),
            preselect: false,
            insert: "item".to_owned(),
            snippet: None,
            detail: String::new(),
            replace: None,
            replace_bytes: None,
            documentation: None,
            raw: serde_json::Value::Null,
            resolved: false,
        }],
        filtered: vec![0],
        query: String::new(),
        selected: 0,
    });

    handle_event(&mut model, key(KeyCode::Char('a')));
    assert_eq!(
        model
            .session
            .editor
            .text()
            .slice(0..model.session.editor.text().len_bytes()),
        "ax"
    );
    assert_eq!(
        model
            .overlays
            .lsp_results
            .completion
            .as_ref()
            .expect("completion")
            .query,
        "a"
    );

    handle_event(&mut model, key(KeyCode::Backspace));
    assert_eq!(
        model
            .session
            .editor
            .text()
            .slice(0..model.session.editor.text().len_bytes()),
        "x"
    );
    assert!(
        model
            .overlays
            .lsp_results
            .completion
            .as_ref()
            .expect("completion")
            .query
            .is_empty()
    );

    handle_event(&mut model, key(KeyCode::Tab));
    assert!(model.overlays.lsp_results.completion.is_none());
}

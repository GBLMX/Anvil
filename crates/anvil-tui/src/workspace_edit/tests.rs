use super::*;
use std::fs;

fn uri(path: &Path) -> String {
    anvil_lsp_client::file_uri(path)
}

fn open(dir: &tempfile::TempDir) -> Model {
    let file = dir.path().join("file.rs");
    fs::write(&file, "abc").expect("file");
    Model::open(&file).expect("open")
}

#[test]
fn applies_changes_across_open_and_unopened_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "one\n").expect("first");
    fs::write(&second, "two\n").expect("second");
    let mut model = Model::open(&first).expect("open");
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
        "changes": {
            uri(&first): [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
                "newText": "X"
            }],
            uri(&second): [{
                "range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 3}},
                "newText": "!"
            }]
        }
    }))
    .expect("parse");
    workspace_edit.apply(&mut model).expect("apply");
    assert_eq!(model.session.editor.text().slice(0..4), "Xone");
    let second_buffer = model
        .buffers
        .background
        .get(&second.canonicalize().expect("canonical"))
        .expect("second buffer");
    assert_eq!(second_buffer.editor.text().slice(0..4), "two!");
    assert!(second_buffer.modified);
}

#[test]
fn background_edits_do_not_consume_foreground_slots() {
    let dir = tempfile::tempdir().expect("tempdir");
    let paths: Vec<_> = (0..10)
        .map(|index| {
            let path = dir.path().join(format!("{index}.rs"));
            fs::write(&path, "text\n").expect("file");
            path
        })
        .collect();
    let mut model = Model::open(&paths[0]).expect("open");
    for path in &paths[1..8] {
        model.open_path(path).expect("open");
    }
    let unopened = &paths[9];
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
        "changes": {
            uri(unopened): [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
                "newText": "X"
            }]
        }
    }))
    .expect("parse");
    workspace_edit.apply(&mut model).expect("apply");
    assert_eq!(model.buffers.entries.len(), 8);
    assert!(
        model
            .buffers
            .background
            .contains_key(&unopened.canonicalize().expect("canonical"))
    );

    model.open_path(&paths[8]).expect("open ninth");
    assert_eq!(model.buffers.entries.len(), 9);
    assert!(
        model
            .buffers
            .background
            .contains_key(&unopened.canonicalize().expect("canonical"))
    );
    let error = model.open_path(unopened).unwrap_err();
    assert_eq!(error, "buffer limit reached (9)");
    assert!(
        model
            .buffers
            .background
            .contains_key(&unopened.canonicalize().expect("canonical"))
    );
}

#[test]
fn opening_a_background_buffer_promotes_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "first\n").expect("first");
    fs::write(&second, "two\n").expect("second");
    let mut model = Model::open(&first).expect("open");
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
        "changes": {
            uri(&second): [{
                "range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 3}},
                "newText": "!"
            }]
        }
    }))
    .expect("parse");
    workspace_edit.apply(&mut model).expect("apply");
    assert!(
        model
            .buffers
            .background
            .contains_key(&second.canonicalize().expect("canonical"))
    );

    model.open_path(&second).expect("promote");
    assert!(model.buffers.background.is_empty());
    assert_eq!(model.session.editor.text().slice(0..4), "two!");
    assert_eq!(model.buffers.entries.len(), 2);
}

#[test]
fn external_change_reloads_clean_background_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "first\n").expect("first");
    fs::write(&second, "two\n").expect("second");
    let mut model = Model::open(&first).expect("open");
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
        "changes": {
            uri(&second): [{
                "range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 3}},
                "newText": "!"
            }]
        }
    }))
    .expect("parse");
    workspace_edit.apply(&mut model).expect("apply");
    let path = second.canonicalize().expect("canonical");
    assert!(model.save_all());
    fs::write(&second, "disk\n").expect("write disk");

    model.handle_external_change(path.clone());
    let buffer = model.buffers.background.get(&path).expect("background");
    assert_eq!(buffer.editor.text().slice(0..5), "disk\n");
    assert!(!buffer.modified);

    fs::remove_file(&second).expect("remove");
    model.handle_external_change(path.clone());
    assert!(
        model
            .buffers
            .background
            .get(&path)
            .expect("background")
            .missing
    );
}

#[test]
fn external_change_prompts_for_modified_background_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "first\n").expect("first");
    fs::write(&second, "two\n").expect("second");
    let mut model = Model::open(&first).expect("open");
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
        "changes": {
            uri(&second): [{
                "range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 3}},
                "newText": "!"
            }]
        }
    }))
    .expect("parse");
    workspace_edit.apply(&mut model).expect("apply");
    let path = second.canonicalize().expect("canonical");
    fs::write(&second, "disk\n").expect("write disk");

    model.handle_external_change(path.clone());
    assert!(matches!(
        &model.ui.input_mode,
        InputMode::ConfirmReload { path: pending } if pending == &path
    ));
    let buffer = model.buffers.background.get(&path).expect("background");
    assert_eq!(buffer.editor.text().slice(0..4), "two!");
    assert!(buffer.modified);

    model.reload_external(&path).expect("reload");
    let buffer = model.buffers.background.get(&path).expect("background");
    assert_eq!(buffer.editor.text().slice(0..5), "disk\n");
    assert!(!buffer.modified);
}

#[test]
fn active_buffer_snapshot_tracks_workspace_edit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "first").expect("first");
    fs::write(&second, "second").expect("second");
    let mut model = Model::open(&first).expect("open first");
    model.open_path(&second).expect("open second");
    model.switch_buffer(-1);
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
        "changes": {
            uri(&first): [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
                "newText": "X"
            }]
        }
    }))
    .expect("parse");
    workspace_edit.apply(&mut model).expect("apply");
    assert_eq!(model.session.editor.text().slice(0..6), "Xfirst");
    assert_eq!(
        model.buffers.entries[model.buffers.active]
            .editor
            .text()
            .slice(0..6),
        "Xfirst"
    );
    assert!(model.buffers.entries[model.buffers.active].modified);
}

#[test]
fn save_all_writes_ghost_buffers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.rs");
    let second = dir.path().join("second.rs");
    fs::write(&first, "first").expect("first");
    fs::write(&second, "second").expect("second");
    let mut model = Model::open(&first).expect("open");
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
        "changes": {
            uri(&second): [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
                "newText": "G"
            }]
        }
    }))
    .expect("parse");
    workspace_edit.apply(&mut model).expect("apply");
    assert!(
        model
            .buffers
            .background
            .contains_key(&second.canonicalize().expect("canonical"))
    );
    assert!(model.buffers.entries.is_empty());
    assert!(model.any_modified());
    assert_eq!(fs::read_to_string(&second).expect("before save"), "second");
    assert!(model.save_all());
    assert_eq!(fs::read_to_string(&second).expect("after save"), "Gsecond");
    assert!(!model.any_modified());
}

#[test]
fn rejects_overlapping_changes_without_modifying_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("file.rs");
    fs::write(&file, "abc").expect("file");
    let mut model = Model::open(&file).expect("open");
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
            "changes": {
                uri(&file): [
                    {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 2}}, "newText": "x"},
                    {"range": {"start": {"line": 0, "character": 1}, "end": {"line": 0, "character": 3}}, "newText": "y"}
                ]
            }
        }))
        .expect("parse");
    assert!(workspace_edit.apply(&mut model).is_err());
    assert_eq!(model.session.editor.text().slice(0..3), "abc");
}

#[test]
fn rejects_positions_inside_a_surrogate_pair() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("file.rs");
    fs::write(&file, "a🦀b").expect("file");
    let mut model = Model::open(&file).expect("open");
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
        "changes": {
            uri(&file): [{
                "range": {"start": {"line": 0, "character": 2}, "end": {"line": 0, "character": 3}},
                "newText": "x"
            }]
        }
    }))
    .expect("parse");
    assert!(workspace_edit.apply(&mut model).is_err());
    assert_eq!(model.session.editor.text().slice(0..6), "a🦀b");
}

#[test]
fn rejects_stale_lsp_document_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut model = open(&dir);
    let path = model.session.path.clone().expect("path");
    model.language.lsp_requests.versions.insert(path.clone(), 3);
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
        "documentChanges": [{
            "textDocument": {"uri": uri(&path), "version": 2},
            "edits": [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
                "newText": "X"
            }]
        }]
    }))
    .expect("parse");
    assert!(workspace_edit.apply(&mut model).is_err());
    assert_eq!(model.session.editor.text().slice(0..3), "abc");
}

#[test]
fn applies_current_lsp_document_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut model = open(&dir);
    let path = model.session.path.clone().expect("path");
    model.language.lsp_requests.versions.insert(path.clone(), 2);
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
        "documentChanges": [{
            "textDocument": {"uri": uri(&path), "version": 2},
            "edits": [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
                "newText": "X"
            }]
        }]
    }))
    .expect("parse");
    workspace_edit.apply(&mut model).expect("apply");
    assert_eq!(model.session.editor.text().slice(0..4), "Xabc");
}

#[test]
fn rejects_stale_buffer_snapshot_before_any_change() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut model = open(&dir);
    let path = model.session.path.clone().expect("path");
    let mut base = HashMap::new();
    base.insert(path.clone(), model.session.editor.revision());
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
        "changes": {
            uri(&path): [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
                "newText": "X"
            }]
        }
    }))
    .expect("parse")
    .with_base_revisions(Some(base));
    model.session.editor.insert(0, "stale");
    assert!(workspace_edit.apply(&mut model).is_err());
    assert_eq!(model.session.editor.text().slice(0..8), "staleabc");
}

#[test]
fn applies_create_rename_and_delete_document_changes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let old = dir.path().join("old.txt");
    let renamed = dir.path().join("renamed.txt");
    let created = dir.path().join("created.txt");
    let doomed = dir.path().join("doomed.txt");
    fs::write(&old, "old").expect("old");
    fs::write(&doomed, "doomed").expect("doomed");
    let mut model = Model::open(&old).expect("open");
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
        "documentChanges": [
            {"kind": "create", "uri": uri(&created)},
            {"kind": "rename", "oldUri": uri(&old), "newUri": uri(&renamed)},
            {"kind": "delete", "uri": uri(&doomed)}
        ]
    }))
    .expect("parse");
    workspace_edit.apply(&mut model).expect("apply");
    assert!(!old.exists());
    assert!(renamed.exists());
    assert!(created.exists());
    assert!(!doomed.exists());
    assert_eq!(model.session.path.as_deref(), Some(renamed.as_path()));
}

#[test]
fn refuses_to_delete_a_modified_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut model = open(&dir);
    let path = model.session.path.clone().expect("path");
    model.session.editor.insert(0, "x");
    model.file.modified = true;
    let workspace_edit = WorkspaceEdit::from_lsp(&serde_json::json!({
        "documentChanges": [{"kind": "delete", "uri": uri(&path)}]
    }))
    .expect("parse");
    assert!(workspace_edit.apply(&mut model).is_err());
    assert!(path.exists());
}

#[test]
#[ignore = "requires anvil-gateway"]
fn applies_directory_resources_through_gateway() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().canonicalize().expect("canonical");
    let old = workspace.join("old");
    let renamed = workspace.join("renamed");
    let created = workspace.join("created.txt");
    let moved = workspace.join("moved.txt");
    fs::create_dir(&old).expect("old dir");
    fs::write(old.join("file.txt"), "content").expect("nested file");
    let (session, gateway, _lsp) =
        crate::gateway::GatewaySession::start(&workspace).expect("gateway");
    let mut model = Model::with_gateway(gateway, workspace.clone());
    model.workspace.require_gateway = true;

    let create = WorkspaceEdit::from_lsp(&serde_json::json!({
        "documentChanges": [{"kind": "create", "uri": uri(&created)}]
    }))
    .expect("parse create");
    create.apply(&mut model).expect("create file");
    assert!(created.exists());

    let file_rename = WorkspaceEdit::from_lsp(&serde_json::json!({
        "documentChanges": [{
            "kind": "rename",
            "oldUri": uri(&created),
            "newUri": uri(&moved)
        }]
    }))
    .expect("parse file rename");
    file_rename.apply(&mut model).expect("rename file");
    assert!(!created.exists());
    assert!(moved.exists());

    let file_delete = WorkspaceEdit::from_lsp(&serde_json::json!({
        "documentChanges": [{"kind": "delete", "uri": uri(&moved)}]
    }))
    .expect("parse file delete");
    file_delete.apply(&mut model).expect("delete file");
    assert!(!moved.exists());

    let rename = WorkspaceEdit::from_lsp(&serde_json::json!({
        "documentChanges": [{
            "kind": "rename",
            "oldUri": uri(&old),
            "newUri": uri(&renamed)
        }]
    }))
    .expect("parse rename");
    rename.apply(&mut model).expect("rename directory");
    assert!(!old.exists());
    assert!(renamed.join("file.txt").exists());

    let delete = WorkspaceEdit::from_lsp(&serde_json::json!({
        "documentChanges": [{
            "kind": "delete",
            "uri": uri(&renamed),
            "options": {"recursive": true}
        }]
    }))
    .expect("parse delete");
    delete.apply(&mut model).expect("delete directory");
    assert!(!renamed.exists());
    drop(session);
}

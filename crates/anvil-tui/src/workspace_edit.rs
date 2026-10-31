//! LSP `WorkspaceEdit` parsing and multi-file application.
//!
//! Edits are validated against every target document before any buffer is
//! changed, using UTF-16 LSP positions and descending byte application.

use super::*;
use crate::lsp;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
struct RawTextEdit {
    range: lsp::Range,
    #[serde(rename = "newText")]
    new_text: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WorkspaceEdit {
    changes: HashMap<PathBuf, Vec<RawTextEdit>>,
}

impl WorkspaceEdit {
    pub(crate) fn from_lsp(value: &serde_json::Value) -> Result<Self, String> {
        let mut changes: HashMap<PathBuf, Vec<RawTextEdit>> = HashMap::new();
        if let Some(entries) = value.get("changes").and_then(serde_json::Value::as_object) {
            for (uri, edits) in entries {
                let path = uri_path(uri)?;
                let edits: Vec<RawTextEdit> = serde_json::from_value(edits.clone())
                    .map_err(|error| format!("invalid workspace edit: {error}"))?;
                changes.entry(path).or_default().extend(edits);
            }
        }
        if let Some(document_changes) = value
            .get("documentChanges")
            .and_then(serde_json::Value::as_array)
        {
            for document in document_changes {
                let Some(uri) = document
                    .get("textDocument")
                    .and_then(|text_document| text_document.get("uri"))
                    .and_then(serde_json::Value::as_str)
                else {
                    return Err("unsupported workspace document change".to_owned());
                };
                let path = uri_path(uri)?;
                let edits = document
                    .get("edits")
                    .cloned()
                    .ok_or_else(|| "workspace document change has no edits".to_owned())?;
                let edits: Vec<RawTextEdit> = serde_json::from_value(edits)
                    .map_err(|error| format!("invalid workspace edit: {error}"))?;
                changes.entry(path).or_default().extend(edits);
            }
        }
        Ok(Self { changes })
    }

    pub(crate) fn apply(&self, model: &mut Model) -> Result<(), String> {
        let mut states: HashMap<PathBuf, EditorState> = HashMap::new();
        for buffer in &model.buffers {
            if let Some(path) = &buffer.path {
                states.insert(path.clone(), buffer.editor.clone());
            }
        }
        if let Some(path) = &model.path {
            states.insert(path.clone(), model.editor.clone());
        }

        let mut prepared: HashMap<PathBuf, EditorState> = HashMap::new();
        for (path, edits) in &self.changes {
            if edits.is_empty() {
                continue;
            }
            let state = if let Some(state) = states.get(path) {
                state.clone()
            } else {
                let text = model.read_file(path)?;
                EditorState::from_text(&text)
            };
            let mut ranges = Vec::with_capacity(edits.len());
            for edit in edits {
                let start =
                    offset_at_utf16(&state, edit.range.start.line, edit.range.start.character)
                        .ok_or_else(|| format!("invalid edit start in {}", path.display()))?;
                let end = offset_at_utf16(&state, edit.range.end.line, edit.range.end.character)
                    .ok_or_else(|| format!("invalid edit end in {}", path.display()))?;
                if start > end {
                    return Err(format!("invalid edit range in {}", path.display()));
                }
                ranges.push((start, end, edit.new_text.clone()));
            }
            ranges.sort_by_key(|(start, end, _)| (*start, *end));
            for pair in ranges.windows(2) {
                if pair[0].1 > pair[1].0 {
                    return Err(format!("overlapping edits in {}", path.display()));
                }
            }
            let mut state = state;
            for (start, end, text) in ranges.into_iter().rev() {
                state.set_selection_bytes(start, end);
                state.delete(start..end);
                state.insert(start, &text);
            }
            prepared.insert(path.clone(), state);
        }

        for (path, state) in prepared {
            if model.path.as_ref() == Some(&path) {
                model.editor = state;
                model.cursor = model.cursor.min(model.editor.text().len_bytes());
                model.modified = true;
                model.syntax.borrow_mut().invalidate();
                if let Some(buffer) = model
                    .buffers
                    .iter_mut()
                    .find(|buffer| buffer.path.as_ref() == Some(&path))
                {
                    buffer.editor = model.editor.clone();
                    buffer.cursor = model.cursor;
                    buffer.modified = true;
                }
            } else if let Some(buffer) = model
                .buffers
                .iter_mut()
                .find(|buffer| buffer.path.as_ref() == Some(&path))
            {
                buffer.editor = state;
                buffer.modified = true;
            } else {
                model.buffers.push(Buffer {
                    editor: state,
                    path: Some(path),
                    cursor: 0,
                    scroll: 0,
                    hscroll: 0,
                    modified: true,
                    search_query: String::new(),
                });
            }
        }
        model.dirty = true;
        Ok(())
    }
}

fn uri_path(uri: &str) -> Result<PathBuf, String> {
    lsp::uri_to_path(uri).ok_or_else(|| format!("invalid workspace URI: {uri}"))
}

fn offset_at_utf16(editor: &EditorState, line: u32, character: u32) -> Option<usize> {
    let line = line as usize;
    let start = editor.index().line_to_offset(line)?;
    let end = editor
        .index()
        .offset_to_line(start)
        .map(|position| position.end)?;
    let text = editor.text().slice(start..end);
    let mut units = 0;
    for (offset, codepoint) in text.char_indices() {
        if units >= character as usize {
            return Some(start + offset);
        }
        units += codepoint.len_utf16();
    }
    Some(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp_worker;

    fn uri(path: &Path) -> String {
        lsp_worker::file_uri(path)
    }

    #[test]
    fn applies_changes_across_open_and_unopened_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("first.rs");
        let second = dir.path().join("second.rs");
        std::fs::write(&first, "one\n").expect("first");
        std::fs::write(&second, "two\n").expect("second");
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
        assert_eq!(model.editor.text().slice(0..4), "Xone");
        let second_buffer = model
            .buffers
            .iter()
            .find(|buffer| buffer.path.as_ref() == Some(&second.canonicalize().expect("canonical")))
            .expect("second buffer");
        assert_eq!(second_buffer.editor.text().slice(0..4), "two!");
        assert!(second_buffer.modified);
        model.path = Some(second.clone());
    }

    #[test]
    fn active_buffer_snapshot_tracks_workspace_edit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("first.rs");
        let second = dir.path().join("second.rs");
        std::fs::write(&first, "first").expect("first");
        std::fs::write(&second, "second").expect("second");
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
        assert_eq!(model.editor.text().slice(0..6), "Xfirst");
        assert_eq!(
            model.buffers[model.active].editor.text().slice(0..6),
            "Xfirst"
        );
        assert!(model.buffers[model.active].modified);
    }

    #[test]
    fn rejects_overlapping_changes_without_modifying_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("file.rs");
        std::fs::write(&file, "abc").expect("file");
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
        assert_eq!(model.editor.text().slice(0..3), "abc");
    }
}

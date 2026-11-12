//! LSP `WorkspaceEdit` parsing and multi-file application.
//!
//! Edits are validated against every target document before any buffer is
//! changed, using UTF-16 LSP positions and descending byte application.
//! `documentChanges` resource operations are applied in order, and known LSP
//! document versions and editor revisions are checked before any mutation.

use super::*;
use crate::lsp;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
struct RawTextEdit {
    range: lsp::Range,
    #[serde(rename = "newText")]
    new_text: String,
}

#[derive(Debug, Clone)]
enum Operation {
    Text {
        path: PathBuf,
        version: Option<i64>,
        edits: Vec<RawTextEdit>,
    },
    Create {
        path: PathBuf,
        overwrite: bool,
        ignore_if_exists: bool,
    },
    Rename {
        from: PathBuf,
        to: PathBuf,
        overwrite: bool,
        ignore_if_exists: bool,
    },
    Delete {
        path: PathBuf,
        recursive: bool,
        ignore_if_missing: bool,
    },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WorkspaceEdit {
    operations: Vec<Operation>,
    base_revisions: HashMap<PathBuf, u64>,
}

impl WorkspaceEdit {
    pub(crate) fn from_lsp(value: &serde_json::Value) -> Result<Self, String> {
        let mut operations = Vec::new();
        if let Some(entries) = value.get("changes").and_then(serde_json::Value::as_object) {
            for (uri, edits) in entries {
                let path = uri_path(uri)?;
                operations.push(Operation::Text {
                    path,
                    version: None,
                    edits: parse_edits(edits)?,
                });
            }
        }
        if let Some(document_changes) = value
            .get("documentChanges")
            .and_then(serde_json::Value::as_array)
        {
            for document in document_changes {
                operations.push(parse_document_change(document)?);
            }
        }
        Ok(Self {
            operations,
            base_revisions: HashMap::new(),
        })
    }

    pub(crate) fn with_base_revisions(mut self, revisions: Option<HashMap<PathBuf, u64>>) -> Self {
        if let Some(revisions) = revisions {
            self.base_revisions = revisions;
        }
        self
    }

    pub(crate) fn apply(&self, model: &mut Model) -> Result<(), String> {
        self.check_revisions(model)?;
        let prepared = self.prepare(model)?;
        self.apply_resources(model)?;
        commit_states(model, prepared);
        Ok(())
    }

    fn check_revisions(&self, model: &Model) -> Result<(), String> {
        for (path, expected) in &self.base_revisions {
            let current = if model.path.as_ref() == Some(path) {
                Some(model.editor.revision())
            } else {
                model
                    .buffers
                    .iter()
                    .find(|buffer| buffer.path.as_ref() == Some(path))
                    .map(|buffer| buffer.editor.revision())
                    .or_else(|| {
                        model.split.as_ref().and_then(|pane| {
                            (pane.buffer.path.as_ref() == Some(path))
                                .then(|| pane.buffer.editor.revision())
                        })
                    })
            };
            if current != Some(*expected) {
                return Err(format!("stale workspace edit for {}", path.display()));
            }
        }
        for operation in &self.operations {
            let Operation::Text {
                path,
                version: Some(version),
                ..
            } = operation
            else {
                continue;
            };
            let Ok(version) = u64::try_from(*version) else {
                return Err(format!("invalid document version in {}", path.display()));
            };
            if let Some(current) = model.lsp_document_version(path)
                && current != version
            {
                return Err(format!(
                    "stale document version for {}: expected {version}, actual {current}",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    fn prepare(&self, model: &mut Model) -> Result<HashMap<PathBuf, EditorState>, String> {
        let mut states: HashMap<PathBuf, Option<EditorState>> = HashMap::new();
        let mut touched = HashSet::new();
        for operation in &self.operations {
            match operation {
                Operation::Text { path, edits, .. } => {
                    if edits.is_empty() {
                        continue;
                    }
                    let state = state_slot(&mut states, model, path)?;
                    apply_text_edits(state, edits, path)?;
                    touched.insert(path.clone());
                }
                Operation::Create {
                    path,
                    overwrite,
                    ignore_if_exists,
                } => {
                    let exists = virtual_exists(&states, model, path)?;
                    if exists && !overwrite {
                        if *ignore_if_exists {
                            continue;
                        }
                        return Err(format!("file exists: {}", path.display()));
                    }
                    if exists && is_directory(model, path) {
                        return Err(format!(
                            "refusing to overwrite directory: {}",
                            path.display()
                        ));
                    }
                    if exists && *overwrite && path_is_open(model, path) {
                        return Err(format!(
                            "refusing to overwrite open buffer: {}",
                            path.display()
                        ));
                    }
                    states.insert(path.clone(), Some(EditorState::from_text("")));
                }
                Operation::Rename {
                    from,
                    to,
                    overwrite,
                    ignore_if_exists,
                } => {
                    let state = match states.get(from) {
                        Some(state) => state.clone(),
                        None => load_state(model, from)?,
                    };
                    let Some(state) = state else {
                        return Err(format!("rename source is missing: {}", from.display()));
                    };
                    if virtual_exists(&states, model, to)? {
                        if !overwrite {
                            if *ignore_if_exists {
                                continue;
                            }
                            return Err(format!("rename target exists: {}", to.display()));
                        }
                        if is_directory(model, to) {
                            return Err(format!(
                                "refusing to overwrite directory: {}",
                                to.display()
                            ));
                        }
                        if path_is_open(model, to) {
                            return Err(format!(
                                "refusing to overwrite open buffer: {}",
                                to.display()
                            ));
                        }
                        states.insert(to.clone(), None);
                    }
                    states.insert(from.clone(), None);
                    states.insert(to.clone(), Some(state));
                    touched = touched
                        .into_iter()
                        .map(|path| remap_path(&path, from, to))
                        .collect();
                }
                Operation::Delete {
                    path,
                    recursive,
                    ignore_if_missing,
                } => {
                    let state = match states.remove(path) {
                        Some(state) => state,
                        None => load_state(model, path)?,
                    };
                    if state.is_none() {
                        if *ignore_if_missing {
                            continue;
                        }
                        return Err(format!("delete target is missing: {}", path.display()));
                    }
                    if path_is_modified(model, path) {
                        return Err(format!(
                            "refusing to delete modified buffer: {}",
                            path.display()
                        ));
                    }
                    if !recursive && is_directory(model, path) {
                        return Err(format!(
                            "refusing to delete directory without recursive option: {}",
                            path.display()
                        ));
                    }
                    states.insert(path.clone(), None);
                    touched.remove(path);
                }
            }
        }
        let mut prepared = HashMap::new();
        for path in touched {
            if let Some(Some(state)) = states.get(&path) {
                prepared.insert(path, state.clone());
            }
        }
        Ok(prepared)
    }

    fn apply_resources(&self, model: &mut Model) -> Result<(), String> {
        for operation in &self.operations {
            match operation {
                Operation::Create {
                    path,
                    overwrite,
                    ignore_if_exists,
                } => {
                    if path_exists(model, path)? && !overwrite {
                        if *ignore_if_exists {
                            continue;
                        }
                        return Err(format!("file exists: {}", path.display()));
                    }
                    model.write_file(path, "")?;
                    model.tree.stale = true;
                    model.sync_watch_paths();
                    model.dirty = true;
                }
                Operation::Rename {
                    from,
                    to,
                    overwrite,
                    ignore_if_exists,
                } => {
                    if !path_exists(model, from)? {
                        return Err(format!("rename source is missing: {}", from.display()));
                    }
                    if path_exists(model, to)? {
                        if !overwrite {
                            if *ignore_if_exists {
                                continue;
                            }
                            return Err(format!("rename target exists: {}", to.display()));
                        }
                        if is_directory(model, to) {
                            return Err(format!(
                                "refusing to overwrite directory: {}",
                                to.display()
                            ));
                        }
                        model.remove_path(to)?;
                    }
                    model.rename_path(from, to)?;
                    model.retarget_path(from, to);
                }
                Operation::Delete {
                    path,
                    recursive,
                    ignore_if_missing,
                } => {
                    if !path_exists(model, path)? {
                        if *ignore_if_missing {
                            continue;
                        }
                        return Err(format!("delete target is missing: {}", path.display()));
                    }
                    if !recursive && is_directory(model, path) {
                        return Err(format!(
                            "refusing to delete directory without recursive option: {}",
                            path.display()
                        ));
                    }
                    model.remove_path(path)?;
                    model.close_paths_under(path);
                }
                Operation::Text { .. } => {}
            }
        }
        Ok(())
    }
}

fn parse_document_change(document: &serde_json::Value) -> Result<Operation, String> {
    let kind = document.get("kind").and_then(serde_json::Value::as_str);
    match kind {
        Some("create") => {
            let path = uri_path(
                document
                    .get("uri")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "create document change has no URI".to_owned())?,
            )?;
            let options = document.get("options");
            Ok(Operation::Create {
                path,
                overwrite: bool_option(options, "overwrite"),
                ignore_if_exists: bool_option(options, "ignoreIfExists"),
            })
        }
        Some("rename") => {
            let from = uri_path(
                document
                    .get("oldUri")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "rename document change has no old URI".to_owned())?,
            )?;
            let to = uri_path(
                document
                    .get("newUri")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "rename document change has no new URI".to_owned())?,
            )?;
            let options = document.get("options");
            Ok(Operation::Rename {
                from,
                to,
                overwrite: bool_option(options, "overwrite"),
                ignore_if_exists: bool_option(options, "ignoreIfExists"),
            })
        }
        Some("delete") => {
            let path = uri_path(
                document
                    .get("uri")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "delete document change has no URI".to_owned())?,
            )?;
            let options = document.get("options");
            Ok(Operation::Delete {
                path,
                recursive: bool_option(options, "recursive"),
                ignore_if_missing: bool_option(options, "ignoreIfNotExists"),
            })
        }
        Some(other) => Err(format!("unsupported workspace document change: {other}")),
        None => {
            let text_document = document
                .get("textDocument")
                .ok_or_else(|| "workspace document change has no text document".to_owned())?;
            let path = uri_path(
                text_document
                    .get("uri")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "workspace document change has no URI".to_owned())?,
            )?;
            let version = text_document
                .get("version")
                .and_then(serde_json::Value::as_i64);
            let edits = document
                .get("edits")
                .ok_or_else(|| "workspace document change has no edits".to_owned())?;
            Ok(Operation::Text {
                path,
                version,
                edits: parse_edits(edits)?,
            })
        }
    }
}

fn parse_edits(value: &serde_json::Value) -> Result<Vec<RawTextEdit>, String> {
    serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid workspace edit: {error}"))
}

fn bool_option(options: Option<&serde_json::Value>, name: &str) -> bool {
    options
        .and_then(|options| options.get(name))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn uri_path(uri: &str) -> Result<PathBuf, String> {
    lsp::uri_to_path(uri).ok_or_else(|| format!("invalid workspace URI: {uri}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathKind {
    Missing,
    File,
    Directory,
    Other,
}

fn path_kind(model: &mut Model, path: &Path) -> Result<PathKind, String> {
    if !model.workspace.require_gateway {
        return match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => Ok(PathKind::Directory),
            Ok(metadata) if metadata.is_file() => Ok(PathKind::File),
            Ok(_) => Ok(PathKind::Other),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(PathKind::Missing),
            Err(error) => Err(error.to_string()),
        };
    }
    let relative = model.relative_path(path)?;
    let gateway = model
        .workspace
        .gateway
        .as_mut()
        .ok_or_else(|| "gateway unavailable".to_owned())?;
    if gateway.read_dir(&relative).is_ok() {
        return Ok(PathKind::Directory);
    }
    if gateway.file_size(&relative).is_ok() {
        return Ok(PathKind::File);
    }
    Ok(PathKind::Missing)
}

fn path_exists(model: &mut Model, path: &Path) -> Result<bool, String> {
    Ok(path_kind(model, path)? != PathKind::Missing)
}

fn is_directory(model: &mut Model, path: &Path) -> bool {
    matches!(path_kind(model, path), Ok(PathKind::Directory))
}

fn virtual_exists(
    states: &HashMap<PathBuf, Option<EditorState>>,
    model: &mut Model,
    path: &Path,
) -> Result<bool, String> {
    if let Some(state) = states.get(path) {
        return Ok(state.is_some());
    }
    path_exists(model, path)
}

fn load_state(model: &mut Model, path: &Path) -> Result<Option<EditorState>, String> {
    if model.path.as_deref() == Some(path) {
        return Ok(Some(model.editor.clone().into_inner()));
    }
    if let Some(buffer) = model
        .buffers
        .iter()
        .find(|buffer| buffer.path.as_deref() == Some(path))
    {
        return Ok(Some(buffer.editor.clone().into_inner()));
    }
    if !path_exists(model, path)? {
        return Ok(None);
    }
    if is_directory(model, path) {
        return Ok(Some(EditorState::from_text("")));
    }
    let text = model.read_file(path)?;
    Ok(Some(EditorState::from_text(&text)))
}

fn state_slot<'a>(
    states: &'a mut HashMap<PathBuf, Option<EditorState>>,
    model: &mut Model,
    path: &Path,
) -> Result<&'a mut EditorState, String> {
    if !states.contains_key(path) {
        let state = load_state(model, path)?;
        states.insert(path.to_path_buf(), state);
    }
    match states.get_mut(path) {
        Some(Some(state)) => Ok(state),
        Some(None) => Err(format!(
            "workspace edit target is missing: {}",
            path.display()
        )),
        None => unreachable!("state inserted above"),
    }
}

fn apply_text_edits(
    state: &mut EditorState,
    edits: &[RawTextEdit],
    path: &Path,
) -> Result<(), String> {
    let mut ranges = Vec::with_capacity(edits.len());
    for edit in edits {
        let start = offset_at_utf16(state, edit.range.start.line, edit.range.start.character)
            .ok_or_else(|| format!("invalid edit start in {}", path.display()))?;
        let end = offset_at_utf16(state, edit.range.end.line, edit.range.end.character)
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
    for (start, end, text) in ranges.into_iter().rev() {
        state.set_selection_bytes(start, end);
        state.delete(start..end);
        state.insert(start, &text);
    }
    Ok(())
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

fn commit_states(model: &mut Model, prepared: HashMap<PathBuf, EditorState>) {
    for (path, state) in prepared {
        if model.path.as_ref() == Some(&path) {
            model.editor = state.into();
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
            buffer.editor = state.into();
            buffer.modified = true;
        } else {
            model.buffers.push(Buffer {
                editor: state.into(),
                path: Some(path),
                cursor: 0,
                visual_anchor: None,
                visual_line: false,
                missing: false,
                scroll: 0,
                hscroll: 0,
                modified: true,
                search_query: String::new(),
                syntax: Default::default(),
            });
        }
    }
    model.dirty = true;
}

fn remap_path(path: &Path, from: &Path, to: &Path) -> PathBuf {
    if path == from {
        return to.to_path_buf();
    }
    if let Ok(rest) = path.strip_prefix(from) {
        return to.join(rest);
    }
    path.to_path_buf()
}

fn path_is_open(model: &Model, path: &Path) -> bool {
    model.path.as_deref() == Some(path)
        || model
            .buffers
            .iter()
            .any(|buffer| buffer.path.as_deref() == Some(path))
        || model
            .split
            .as_ref()
            .is_some_and(|pane| pane.buffer.path.as_deref() == Some(path))
}

fn path_is_modified(model: &Model, path: &Path) -> bool {
    let affected = |candidate: &Path| candidate == path || candidate.starts_with(path);
    let active = model.path.as_deref().is_some_and(affected) && model.modified;
    let buffer = model
        .buffers
        .iter()
        .any(|buffer| buffer.path.as_deref().is_some_and(affected) && buffer.modified);
    let split = model.split.as_ref().is_some_and(|pane| {
        pane.buffer.path.as_deref().is_some_and(affected) && pane.buffer.modified
    });
    active || buffer || split
}

impl Model {
    fn retarget_path(&mut self, from: &Path, to: &Path) {
        if let Some(path) = self.path.as_mut() {
            *path = remap_path(path, from, to);
        }
        for buffer in &mut self.buffers {
            if let Some(path) = buffer.path.as_mut() {
                *path = remap_path(path, from, to);
            }
        }
        if let Some(pane) = self.split.as_mut()
            && let Some(path) = pane.buffer.path.as_mut()
        {
            *path = remap_path(path, from, to);
        }
        if let Some(back) = self.help_back.as_mut()
            && let Some(path) = back.path.as_mut()
        {
            *path = remap_path(path, from, to);
        }
        let diagnostics = std::mem::take(&mut self.language.diagnostics.by_path);
        self.language.diagnostics.by_path = diagnostics
            .into_iter()
            .map(|(path, value)| (remap_path(&path, from, to), value))
            .collect();
        self.language
            .lsp_requests
            .versions
            .retain(|path, _| path != from && !path.starts_with(from));
        self.git.diff_checked = None;
        self.git.marks.clear();
        self.clear_lsp_ui();
        self.syntax
            .borrow_mut()
            .switch_path(self.path.as_deref(), true);
        self.tree.stale = true;
        self.sync_watch_paths();
        self.dirty = true;
    }

    fn close_paths_under(&mut self, path: &Path) {
        let affected = |candidate: Option<&Path>| {
            candidate.is_some_and(|candidate| candidate == path || candidate.starts_with(path))
        };
        if self
            .split
            .as_ref()
            .is_some_and(|pane| affected(pane.buffer.path.as_deref()))
        {
            self.split = None;
            self.ui.focus.split = false;
        }
        loop {
            let Some(index) = self
                .buffers
                .iter()
                .position(|buffer| affected(buffer.path.as_deref()))
            else {
                break;
            };
            self.close_buffer(index, true);
        }
        self.tree.stale = true;
        self.sync_watch_paths();
        self.dirty = true;
    }

    pub(crate) fn lsp_document_version(&self, path: &Path) -> Option<u64> {
        self.language.lsp_requests.versions.get(path).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp_worker;
    use std::fs;

    fn uri(path: &Path) -> String {
        lsp_worker::file_uri(path)
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
        assert_eq!(model.editor.text().slice(0..4), "Xone");
        let second_buffer = model
            .buffers
            .iter()
            .find(|buffer| buffer.path.as_ref() == Some(&second.canonicalize().expect("canonical")))
            .expect("second buffer");
        assert_eq!(second_buffer.editor.text().slice(0..4), "two!");
        assert!(second_buffer.modified);
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
        assert_eq!(model.editor.text().slice(0..6), "Xfirst");
        assert_eq!(
            model.buffers[model.active].editor.text().slice(0..6),
            "Xfirst"
        );
        assert!(model.buffers[model.active].modified);
    }

    #[test]
    fn save_all_writes_ghost_buffers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("first.rs");
        let second = dir.path().join("second.rs");
        fs::write(&first, "first").expect("first");
        fs::write(&second, "second").expect("second");
        let mut model = Model::open(&first).expect("open");
        model.open_path(&second).expect("second");
        model.switch_buffer(-1);
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
        assert_eq!(fs::read_to_string(&second).expect("before save"), "second");
        assert!(model.save_all());
        assert_eq!(fs::read_to_string(&second).expect("after save"), "Gsecond");
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
        assert_eq!(model.editor.text().slice(0..3), "abc");
    }

    #[test]
    fn rejects_stale_lsp_document_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut model = open(&dir);
        let path = model.path.clone().expect("path");
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
        assert_eq!(model.editor.text().slice(0..3), "abc");
    }

    #[test]
    fn applies_current_lsp_document_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut model = open(&dir);
        let path = model.path.clone().expect("path");
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
        assert_eq!(model.editor.text().slice(0..4), "Xabc");
    }

    #[test]
    fn rejects_stale_buffer_snapshot_before_any_change() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut model = open(&dir);
        let path = model.path.clone().expect("path");
        let mut base = HashMap::new();
        base.insert(path.clone(), model.editor.revision());
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
        model.editor.insert(0, "stale");
        assert!(workspace_edit.apply(&mut model).is_err());
        assert_eq!(model.editor.text().slice(0..8), "staleabc");
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
        assert_eq!(model.path.as_deref(), Some(renamed.as_path()));
    }

    #[test]
    fn refuses_to_delete_a_modified_buffer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut model = open(&dir);
        let path = model.path.clone().expect("path");
        model.editor.insert(0, "x");
        model.modified = true;
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
}

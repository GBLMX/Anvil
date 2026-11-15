//! LSP `WorkspaceEdit` parsing and multi-file application.
//!
//! Edits are validated against every target document before any buffer is
//! changed, using UTF-16 LSP positions and descending byte application.
//! `documentChanges` resource operations are applied in order, and known LSP
//! document versions and editor revisions are checked before any mutation.

use super::*;
use anvil_lsp_client as lsp;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

mod model;
use model::{path_is_modified, path_is_open, remap_path};
#[cfg(test)]
mod tests;

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

#[derive(Debug)]
enum ResourceUndo {
    Remove(PathBuf),
    Rename { from: PathBuf, to: PathBuf },
    Restore { backup: PathBuf, original: PathBuf },
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
        if model.file.read_only {
            model.status.text = "E: read-only buffer".to_owned();
            model.file.dirty = true;
            return Err("read-only buffer".to_owned());
        }
        self.check_revisions(model)?;
        let prepared = self.prepare(model)?;
        self.apply_resources(model)?;
        commit_states(model, prepared);
        Ok(())
    }

    fn check_revisions(&self, model: &Model) -> Result<(), String> {
        for (path, expected) in &self.base_revisions {
            let current = if model.session.path.as_ref() == Some(path) {
                Some(model.session.editor.revision())
            } else {
                model
                    .buffers
                    .entries
                    .iter()
                    .find(|buffer| buffer.path.as_ref() == Some(path))
                    .map(|buffer| buffer.editor.revision())
                    .or_else(|| {
                        model.buffers.split.as_ref().and_then(|pane| {
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
        let mut undo = Vec::new();
        if let Err(error) = self.stage_resources(model, &mut undo) {
            return Err(match rollback_resources(model, &undo) {
                Ok(()) => format!("{error}; rollback complete"),
                Err(rollback) => format!("{error}; rollback failed: {rollback}"),
            });
        }
        self.apply_resource_model_state(model);
        for action in &undo {
            if let ResourceUndo::Restore { backup, .. } = action
                && path_exists(model, backup)?
            {
                model.remove_path(backup)?;
            }
        }
        Ok(())
    }

    fn stage_resources(
        &self,
        model: &mut Model,
        undo: &mut Vec<ResourceUndo>,
    ) -> Result<(), String> {
        let mut backup_counter = 0usize;
        for operation in &self.operations {
            match operation {
                Operation::Create {
                    path,
                    overwrite,
                    ignore_if_exists,
                } => {
                    let exists = path_exists(model, path)?;
                    if exists && !overwrite {
                        if *ignore_if_exists {
                            continue;
                        }
                        return Err(format!("file exists: {}", path.display()));
                    }
                    if exists && *overwrite {
                        backup_existing(model, path, &mut backup_counter, undo)?;
                    }
                    model.write_file(path, "")?;
                    undo.push(ResourceUndo::Remove(path.clone()));
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
                        backup_existing(model, to, &mut backup_counter, undo)?;
                    }
                    model.rename_path(from, to)?;
                    undo.push(ResourceUndo::Rename {
                        from: to.clone(),
                        to: from.clone(),
                    });
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
                    let backup = backup_path(model, path, backup_counter)?;
                    backup_counter += 1;
                    model.rename_path(path, &backup)?;
                    undo.push(ResourceUndo::Restore {
                        backup,
                        original: path.clone(),
                    });
                }
                Operation::Text { .. } => {}
            }
        }
        Ok(())
    }

    fn apply_resource_model_state(&self, model: &mut Model) {
        for operation in &self.operations {
            match operation {
                Operation::Rename { from, to, .. } => model.retarget_path(from, to),
                Operation::Delete { path, .. } => model.close_paths_under(path),
                Operation::Create { .. } | Operation::Text { .. } => {}
            }
        }
        if self
            .operations
            .iter()
            .any(|operation| !matches!(operation, Operation::Text { .. }))
        {
            model.tree.stale = true;
            model.sync_watch_paths();
            model.file.dirty = true;
        }
    }
}

fn backup_existing(
    model: &mut Model,
    path: &Path,
    counter: &mut usize,
    undo: &mut Vec<ResourceUndo>,
) -> Result<(), String> {
    let backup = backup_path(model, path, *counter)?;
    *counter += 1;
    model.rename_path(path, &backup)?;
    undo.push(ResourceUndo::Restore {
        backup,
        original: path.to_path_buf(),
    });
    Ok(())
}

fn backup_path(model: &mut Model, path: &Path, counter: usize) -> Result<PathBuf, String> {
    let root = model
        .workspace
        .roots
        .owning_root(path)
        .map(|root| root.path.clone())
        .or_else(|| path.parent().map(Path::to_path_buf))
        .ok_or_else(|| format!("no backup root for {}", path.display()))?;
    for attempt in 0..128 {
        let candidate = root.join(format!(
            ".anvil-tx-{}-{counter}-{attempt}",
            std::process::id()
        ));
        if !path_exists(model, &candidate)? {
            return Ok(candidate);
        }
    }
    Err(format!(
        "could not allocate a backup for {}",
        path.display()
    ))
}

fn rollback_resources(model: &mut Model, undo: &[ResourceUndo]) -> Result<(), String> {
    for action in undo.iter().rev() {
        match action {
            ResourceUndo::Remove(path) => {
                if path_exists(model, path)? {
                    model.remove_path(path)?;
                }
            }
            ResourceUndo::Rename { from, to } => {
                if path_exists(model, from)? {
                    model.rename_path(from, to)?;
                }
            }
            ResourceUndo::Restore { backup, original } => {
                if path_exists(model, backup)? {
                    if path_exists(model, original)? {
                        model.remove_path(original)?;
                    }
                    model.rename_path(backup, original)?;
                }
            }
        }
    }
    Ok(())
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
    let relative = model.workspace.relative_path(path)?;
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
    if model.session.path.as_deref() == Some(path) {
        return Ok(Some(model.session.editor.clone().into_inner()));
    }
    if let Some(buffer) = model
        .buffers
        .entries
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
        let start = state
            .index()
            .offset_at_utf16(
                state.text(),
                edit.range.start.line as usize,
                edit.range.start.character as usize,
            )
            .ok_or_else(|| format!("invalid edit start in {}", path.display()))?;
        let end = state
            .index()
            .offset_at_utf16(
                state.text(),
                edit.range.end.line as usize,
                edit.range.end.character as usize,
            )
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
    if let Some(&(start, end, _)) = ranges.first() {
        state.set_selection_bytes(start, end);
    }
    state
        .apply_transaction_at(
            state.revision(),
            ranges
                .into_iter()
                .map(|(start, end, text)| (start..end, text))
                .collect(),
        )
        .map(|_| ())
        .map_err(|error| format!("invalid edit transaction in {}: {error}", path.display()))
}

fn commit_states(model: &mut Model, prepared: HashMap<PathBuf, EditorState>) {
    for (path, state) in prepared {
        let cursor = state
            .selection_bytes()
            .first()
            .map(|(_, head)| *head)
            .unwrap_or(0);
        if model.session.path.as_ref() == Some(&path) {
            model.session.editor = state.into();
            model.session.cursor = cursor;
            model.file.modified = true;
            model.syntax_cache.highlights.borrow_mut().invalidate();
            if let Some(buffer) = model
                .buffers
                .entries
                .iter_mut()
                .find(|buffer| buffer.path.as_ref() == Some(&path))
            {
                buffer.editor = model.session.editor.clone();
                buffer.cursor = model.session.cursor;
                buffer.modified = true;
                buffer.syntax = Default::default();
            }
        } else if let Some(buffer) = model
            .buffers
            .entries
            .iter_mut()
            .find(|buffer| buffer.path.as_ref() == Some(&path))
        {
            buffer.editor = state.into();
            buffer.cursor = cursor;
            buffer.modified = true;
            buffer.syntax = Default::default();
        } else if let Some(buffer) = model.buffers.background.get_mut(&path) {
            buffer.editor = state.into();
            buffer.cursor = cursor;
            buffer.modified = true;
            buffer.syntax = Default::default();
        } else {
            model.buffers.background.insert(
                path.clone(),
                Buffer {
                    id: BufferId::next(),
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
                },
            );
        }
    }
    model.sync_watch_paths();
    model.file.dirty = true;
}

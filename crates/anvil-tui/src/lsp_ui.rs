//! LSP request state, completion, definition, and hover UI behavior.

use super::{
    CallHierarchyDirection, CodeActionItem, CodeActionState, CompletionItem, CompletionState,
    LspRequestKind, Model, ReferenceItem, ReferenceState, SignatureHelpState, SignatureItem,
    Snippet, SnippetSession, SymbolItem, SymbolState, WorkspaceEdit, WorkspaceSymbolItem,
    WorkspaceSymbolState,
};
use crate::{lsp, lsp_worker};
use anvil_core::TextBuffer;
use anvil_session::ui::InputMode;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

mod responses;
mod settings;
pub(crate) use settings::lsp_settings;

pub(crate) fn expand_snippet(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'$' {
            if bytes.get(index + 1) == Some(&b'{')
                && let Some(end) = text[index + 2..].find('}')
            {
                let body = &text[index + 2..index + 2 + end];
                let default = body.split_once(':').map(|(_, value)| value).unwrap_or("");
                output.push_str(default);
                index += end + 3;
                continue;
            }
            if bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
                index += 1;
                while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                    index += 1;
                }
                continue;
            }
        }
        let character = text[index..].chars().next().expect("char");
        output.push(character);
        index += character.len_utf8();
    }
    output
}

fn hover_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Object(object) => object
            .get("value")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(hover_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

const MAX_SYMBOL_DEPTH: usize = 64;
const MAX_SYMBOLS: usize = 4096;

fn symbols_from_lsp(items: &[serde_json::Value]) -> Vec<SymbolItem> {
    let mut symbols = Vec::new();
    collect_symbols(items, 0, &mut symbols);
    symbols
}

fn collect_symbols(items: &[serde_json::Value], depth: usize, symbols: &mut Vec<SymbolItem>) {
    if depth > MAX_SYMBOL_DEPTH || symbols.len() >= MAX_SYMBOLS {
        return;
    }
    for item in items {
        if symbols.len() >= MAX_SYMBOLS {
            break;
        }
        let Some(name) = item.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let range = item
            .get("selectionRange")
            .or_else(|| item.get("range"))
            .or_else(|| {
                item.get("location")
                    .and_then(|location| location.get("range"))
            })
            .and_then(|range| serde_json::from_value::<lsp::Range>(range.clone()).ok());
        if let Some(range) = range {
            symbols.push(SymbolItem {
                name: name.to_owned(),
                detail: item
                    .get("detail")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                line: range.start.line as usize,
                character: range.start.character as usize,
                depth,
            });
        }
        if let Some(children) = item.get("children").and_then(serde_json::Value::as_array) {
            collect_symbols(children, depth + 1, symbols);
        }
    }
}

impl Model {
    pub(crate) fn offset_at_utf16(&self, line: usize, character: usize) -> usize {
        let Some(start) = self.editor.index().line_to_offset(line) else {
            return self.editor.text().len_bytes();
        };
        let end = self
            .editor
            .index()
            .offset_to_line(start)
            .map(|position| position.end)
            .unwrap_or(start);
        let text = self.editor.text().slice(start..end);
        let mut units = 0;
        for (offset, codepoint) in text.char_indices() {
            if units >= character {
                return start + offset;
            }
            units += codepoint.len_utf16();
        }
        end
    }

    fn lsp_position(&self) -> Option<(u64, u64)> {
        let position = self
            .editor
            .index()
            .position(self.editor.text(), self.cursor)?;
        let line_start = self.editor.index().line_to_offset(position.line)?;
        let prefix = self.editor.text().slice(line_start..self.cursor);
        let character = prefix.chars().map(char::len_utf16).sum::<usize>();
        Some((position.line as u64, character as u64))
    }

    pub(crate) fn send_lsp_request(
        &mut self,
        kind: LspRequestKind,
        method: &str,
        params: serde_json::Value,
    ) -> bool {
        let Some(path) = self.path.clone() else {
            self.status = "E: lsp requires an open file".to_owned();
            self.dirty = true;
            return false;
        };
        let id = self.language.next_lsp_request;
        self.language.next_lsp_request += 1;
        if self.active_lsp_worker().is_none() {
            self.status = "E: lsp is not running".to_owned();
            self.dirty = true;
            return false;
        }
        let text = self.editor.text().slice(0..self.editor.text().len_bytes());
        if let Err(error) = self.sync_lsp_document(&path, text) {
            self.status = format!("E: lsp request: {error}");
            self.dirty = true;
            return false;
        }
        let Some(worker) = self.active_lsp_worker() else {
            self.status = "E: lsp is not running".to_owned();
            self.dirty = true;
            return false;
        };
        if let Err(error) = worker.request(id, method, params) {
            self.status = format!("E: lsp request: {error}");
            self.dirty = true;
            return false;
        }
        self.language.pending_lsp.insert(id, kind);
        if matches!(
            kind,
            LspRequestKind::Rename
                | LspRequestKind::CodeAction
                | LspRequestKind::Formatting
                | LspRequestKind::SignatureHelp
                | LspRequestKind::DocumentHighlight
                | LspRequestKind::CallHierarchyPrepare(_)
                | LspRequestKind::CallHierarchyIncoming
                | LspRequestKind::CallHierarchyOutgoing
                | LspRequestKind::FoldingRange
                | LspRequestKind::SemanticTokens
                | LspRequestKind::CodeLens
                | LspRequestKind::StickySymbols
        ) {
            self.language
                .pending_edit_revision
                .insert(id, self.editor.revision());
            self.language.pending_edit_path.insert(id, path);
            self.language
                .pending_edit_snapshot
                .insert(id, self.buffer_revisions());
        }
        true
    }

    pub(crate) fn sync_lsp_document(&mut self, path: &Path, text: String) -> Result<(), String> {
        let Some(worker) = self.active_lsp_worker() else {
            return Err("lsp is not running".to_owned());
        };
        worker.sync(path, text)?;
        self.record_lsp_version(path);
        Ok(())
    }

    fn record_lsp_version(&mut self, path: &Path) {
        if let Some(version) = self.language.lsp_versions.get_mut(path) {
            *version += 1;
        } else {
            self.language.lsp_versions.insert(path.to_path_buf(), 1);
        }
    }

    pub(crate) fn buffer_revisions(&self) -> HashMap<PathBuf, u64> {
        let mut revisions = HashMap::new();
        if let Some(path) = &self.path {
            revisions.insert(path.clone(), self.editor.revision());
        }
        for buffer in &self.buffers {
            if let Some(path) = &buffer.path {
                revisions
                    .entry(path.clone())
                    .or_insert_with(|| buffer.editor.revision());
            }
        }
        if let Some(pane) = &self.split
            && let Some(path) = &pane.buffer.path
        {
            revisions
                .entry(path.clone())
                .or_insert_with(|| pane.buffer.editor.revision());
        }
        revisions
    }

    pub(crate) fn jump_diagnostic(&mut self, forward: bool) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.hover = None;
        let Some(path) = self.path.clone() else {
            self.status = "E: no file".to_owned();
            self.dirty = true;
            return;
        };
        let Some(diagnostics) = self.language.diagnostics.get(&path) else {
            self.status = "no diagnostics".to_owned();
            self.dirty = true;
            return;
        };
        if diagnostics.is_empty() {
            self.status = "no diagnostics".to_owned();
            self.dirty = true;
            return;
        }
        let current = self
            .editor
            .index()
            .position(self.editor.text(), self.cursor)
            .map(|position| position.line)
            .unwrap_or(0);
        let primary: Vec<&crate::lsp::Diagnostic> = if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity.is_some())
        {
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity.is_some())
                .collect()
        } else {
            diagnostics.iter().collect()
        };
        let mut positions: Vec<(usize, usize)> = primary
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.range.start.line as usize,
                    diagnostic.range.start.character as usize,
                )
            })
            .collect();
        positions.sort();
        let selected = if forward {
            positions
                .iter()
                .find(|(line, _)| *line > current)
                .or_else(|| positions.first())
        } else {
            positions
                .iter()
                .rev()
                .find(|(line, _)| *line < current)
                .or_else(|| positions.last())
        };
        let Some(&(line, character)) = selected else {
            return;
        };
        self.record_jump();
        self.cursor = self.offset_at_utf16(line, character);
        self.open_diagnostic_peek(path, line as u32, character as u32);
        let index = positions
            .iter()
            .position(|item| *item == (line, character))
            .unwrap_or(0);
        self.status = format!("diagnostic {}/{}", index + 1, positions.len());
        self.dirty = true;
    }

    pub(crate) fn notify_lsp_save(&mut self, path: &std::path::Path, text: &str) {
        let sent = if let Some(key) = Self::lsp_key(path)
            && let Some(worker) = self.language.lsp_workers.get(&key)
        {
            worker.save(path, text.to_owned()).is_ok()
        } else {
            false
        };
        if sent {
            self.record_lsp_version(path);
        }
    }

    pub(crate) fn clear_lsp_requests(&mut self) {
        self.language.pending_lsp.clear();
        self.language.pending_edit_revision.clear();
        self.language.pending_edit_path.clear();
        self.language.pending_edit_snapshot.clear();
        self.signature_help = None;
        self.workspace_symbols = None;
        self.references = None;
        self.document_highlights = None;
        self.call_hierarchy = None;
        self.clear_folds();
        self.clear_semantic_tokens();
        self.inlay_hints.clear();
        self.inlay_pending = None;
        self.inlay_checked = None;
        self.dirty = true;
    }

    pub(crate) fn cancel_signature_help(&mut self) {
        self.language
            .pending_lsp
            .retain(|_, kind| *kind != LspRequestKind::SignatureHelp);
        self.signature_help = None;
        self.prune_edit_requests();
        self.dirty = true;
    }

    pub(crate) fn request_signature_help(&mut self, trigger: char) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_symbols();
        self.cancel_signature_help();
        self.hover = None;
        self.ui.diagnostic_peek = None;
        let Some(uri) = self.path.as_ref().map(|path| lsp_worker::file_uri(path)) else {
            return;
        };
        let Some((line, character)) = self.lsp_position() else {
            return;
        };
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character},
            "context": {"triggerKind": 2, "triggerCharacter": trigger.to_string()}
        });
        if self.send_lsp_request(
            LspRequestKind::SignatureHelp,
            "textDocument/signatureHelp",
            params,
        ) {
            self.status = "signature".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn prune_edit_requests(&mut self) {
        let pending: Vec<u64> = self.language.pending_lsp.keys().copied().collect();
        self.language
            .pending_edit_revision
            .retain(|id, _| pending.contains(id));
        self.language
            .pending_edit_path
            .retain(|id, _| pending.contains(id));
        self.language
            .pending_edit_snapshot
            .retain(|id, _| pending.contains(id));
    }

    pub(crate) fn cancel_completion(&mut self) {
        self.language.pending_lsp.retain(|_, kind| {
            !matches!(
                kind,
                LspRequestKind::Completion | LspRequestKind::CompletionResolve
            )
        });
        self.completion = None;
        self.completion_resolve = None;
    }

    pub(crate) fn cancel_code_actions(&mut self) {
        self.language.pending_lsp.retain(|_, kind| {
            !matches!(
                kind,
                LspRequestKind::CodeAction | LspRequestKind::DocumentSymbol
            )
        });
        self.prune_edit_requests();
        self.code_actions = None;
        self.document_symbols = None;
        self.dirty = true;
    }

    pub(crate) fn auto_completion(&mut self) {
        if self.active_lsp_worker().is_some() {
            self.request_completion();
        }
    }

    pub(crate) fn request_completion(&mut self) {
        self.cancel_completion();
        self.hover = None;
        self.ui.diagnostic_peek = None;
        self.cancel_code_actions();
        let Some(uri) = self.path.as_ref().map(|path| lsp_worker::file_uri(path)) else {
            return;
        };
        let Some((line, character)) = self.lsp_position() else {
            return;
        };
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character},
            "context": {"triggerKind": 1}
        });
        if self.send_lsp_request(
            LspRequestKind::Completion,
            "textDocument/completion",
            params,
        ) {
            self.status = "completion".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn request_completion_resolve(&mut self) {
        let Some(completion) = self.completion.as_ref() else {
            return;
        };
        let Some(index) = completion.filtered.get(completion.selected).copied() else {
            return;
        };
        let Some(item) = completion.items.get(index) else {
            return;
        };
        if item.resolved
            || self
                .completion_resolve
                .is_some_and(|(_, pending)| pending == index)
        {
            return;
        }
        let raw = item.raw.clone();
        let id = self.language.next_lsp_request;
        if self.send_lsp_request(
            LspRequestKind::CompletionResolve,
            "completionItem/resolve",
            raw,
        ) {
            self.completion_resolve = Some((id, index));
        }
    }

    pub(crate) fn request_definition(&mut self) {
        self.cancel_completion();
        self.ui.diagnostic_peek = None;
        self.cancel_code_actions();
        let Some(uri) = self.path.as_ref().map(|path| lsp_worker::file_uri(path)) else {
            return;
        };
        let Some((line, character)) = self.lsp_position() else {
            return;
        };
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        });
        if self.send_lsp_request(
            LspRequestKind::Definition,
            "textDocument/definition",
            params,
        ) {
            self.status = "definition".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn request_references(&mut self) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_symbols();
        self.cancel_workspace_symbols();
        self.cancel_references();
        self.hover = None;
        self.ui.diagnostic_peek = None;
        let Some(uri) = self.path.as_ref().map(|path| lsp_worker::file_uri(path)) else {
            return;
        };
        let Some((line, character)) = self.lsp_position() else {
            return;
        };
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character},
            "context": {"includeDeclaration": true}
        });
        if self.send_lsp_request(
            LspRequestKind::References,
            "textDocument/references",
            params,
        ) {
            self.status = "references".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn request_document_highlights(&mut self) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_highlights();
        self.hover = None;
        self.ui.diagnostic_peek = None;
        let Some(uri) = self.path.as_ref().map(|path| lsp_worker::file_uri(path)) else {
            return;
        };
        let Some((line, character)) = self.lsp_position() else {
            return;
        };
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        });
        if self.send_lsp_request(
            LspRequestKind::DocumentHighlight,
            "textDocument/documentHighlight",
            params,
        ) {
            self.status = "highlights".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn cancel_document_highlights(&mut self) {
        self.language
            .pending_lsp
            .retain(|_, kind| *kind != LspRequestKind::DocumentHighlight);
        self.prune_edit_requests();
        self.document_highlights = None;
        self.dirty = true;
    }

    pub(crate) fn request_call_hierarchy(&mut self, direction: CallHierarchyDirection) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_highlights();
        self.cancel_call_hierarchy();
        self.hover = None;
        self.ui.diagnostic_peek = None;
        let Some(uri) = self.path.as_ref().map(|path| lsp_worker::file_uri(path)) else {
            return;
        };
        let Some((line, character)) = self.lsp_position() else {
            return;
        };
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        });
        if self.send_lsp_request(
            LspRequestKind::CallHierarchyPrepare(direction),
            "textDocument/prepareCallHierarchy",
            params,
        ) {
            self.status = "call hierarchy".to_owned();
        }
        self.dirty = true;
    }

    fn request_call_hierarchy_calls(
        &mut self,
        item: serde_json::Value,
        direction: CallHierarchyDirection,
    ) {
        let (kind, method, label) = match direction {
            CallHierarchyDirection::Incoming => (
                LspRequestKind::CallHierarchyIncoming,
                "callHierarchy/incomingCalls",
                "incoming calls",
            ),
            CallHierarchyDirection::Outgoing => (
                LspRequestKind::CallHierarchyOutgoing,
                "callHierarchy/outgoingCalls",
                "outgoing calls",
            ),
        };
        if self.send_lsp_request(kind, method, serde_json::json!({ "item": item })) {
            self.status = label.to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn cancel_call_hierarchy(&mut self) {
        self.language.pending_lsp.retain(|_, kind| {
            !matches!(
                kind,
                LspRequestKind::CallHierarchyPrepare(_)
                    | LspRequestKind::CallHierarchyIncoming
                    | LspRequestKind::CallHierarchyOutgoing
            )
        });
        self.prune_edit_requests();
        self.call_hierarchy = None;
        self.dirty = true;
    }

    pub(crate) fn request_hover(&mut self) {
        self.cancel_completion();
        self.ui.diagnostic_peek = None;
        self.cancel_code_actions();
        let Some(uri) = self.path.as_ref().map(|path| lsp_worker::file_uri(path)) else {
            return;
        };
        let Some((line, character)) = self.lsp_position() else {
            return;
        };
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        });
        if self.send_lsp_request(LspRequestKind::Hover, "textDocument/hover", params) {
            self.status = "hover".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn start_rename(&mut self) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.hover = None;
        self.ui.diagnostic_peek = None;
        self.ui.input_mode = InputMode::Rename {
            name: String::new(),
        };
        self.status = "rename symbol".to_owned();
        self.dirty = true;
    }

    pub(crate) fn request_rename(&mut self, new_name: &str) {
        if new_name.trim().is_empty() {
            self.status = "E: empty rename".to_owned();
            self.dirty = true;
            return;
        }
        let Some(uri) = self.path.as_ref().map(|path| lsp_worker::file_uri(path)) else {
            return;
        };
        let Some((line, character)) = self.lsp_position() else {
            return;
        };
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character},
            "newName": new_name
        });
        if self.send_lsp_request(LspRequestKind::Rename, "textDocument/rename", params) {
            self.status = "renaming".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn request_code_actions(&mut self) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.hover = None;
        self.ui.diagnostic_peek = None;
        let Some(path) = self.path.clone() else {
            return;
        };
        let uri = lsp_worker::file_uri(&path);
        let Some((line, _character)) = self.lsp_position() else {
            return;
        };
        let line_start = self
            .editor
            .index()
            .line_to_offset(line as usize)
            .unwrap_or(0);
        let line_end = self
            .editor
            .index()
            .offset_to_line(line_start)
            .map(|position| position.end)
            .unwrap_or(line_start);
        let line_text = self.editor.text().slice(line_start..line_end);
        let line_units = line_text.chars().map(char::len_utf16).sum::<usize>();
        let diagnostics = self
            .language
            .diagnostics
            .get(&path)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|diagnostic| serde_json::to_value(diagnostic).ok())
            .collect::<Vec<_>>();
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": line, "character": 0},
                "end": {"line": line, "character": line_units}
            },
            "context": {"diagnostics": diagnostics, "triggerKind": 1}
        });
        if self.send_lsp_request(
            LspRequestKind::CodeAction,
            "textDocument/codeAction",
            params,
        ) {
            self.status = "code actions".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn cancel_references(&mut self) {
        self.language
            .pending_lsp
            .retain(|_, kind| *kind != LspRequestKind::References);
        self.references = None;
        self.dirty = true;
    }

    pub(crate) fn cancel_document_symbols(&mut self) {
        self.language
            .pending_lsp
            .retain(|_, kind| *kind != LspRequestKind::DocumentSymbol);
        self.document_symbols = None;
        self.dirty = true;
    }

    pub(crate) fn cancel_workspace_symbols(&mut self) {
        self.language
            .pending_lsp
            .retain(|_, kind| *kind != LspRequestKind::WorkspaceSymbol);
        self.workspace_symbols = None;
        self.prune_edit_requests();
        self.dirty = true;
    }

    pub(crate) fn request_workspace_symbols(&mut self) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_symbols();
        self.cancel_workspace_symbols();
        self.hover = None;
        self.ui.diagnostic_peek = None;
        let Some(path) = self.path.as_ref() else {
            return;
        };
        let params = serde_json::json!({"query": ""});
        let _ = path;
        if self.send_lsp_request(LspRequestKind::WorkspaceSymbol, "workspace/symbol", params) {
            self.status = "workspace symbols".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn move_workspace_symbol(&mut self, delta: isize) {
        if let Some(symbols) = self.workspace_symbols.as_mut() {
            symbols.selected = symbols
                .selected
                .saturating_add_signed(delta)
                .min(symbols.filtered.len().saturating_sub(1));
            self.dirty = true;
        }
    }

    pub(crate) fn filter_workspace_symbols(&mut self) {
        if let Some(symbols) = self.workspace_symbols.as_mut() {
            let query = symbols.query.to_lowercase();
            symbols.filtered = symbols
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    item.name.to_lowercase().contains(&query)
                        || item.detail.to_lowercase().contains(&query)
                        || item.path.to_string_lossy().to_lowercase().contains(&query)
                })
                .map(|(index, _)| index)
                .collect();
            symbols.selected = symbols
                .selected
                .min(symbols.filtered.len().saturating_sub(1));
            self.dirty = true;
        }
    }

    pub(crate) fn apply_workspace_symbol(&mut self) {
        let Some(symbols) = self.workspace_symbols.take() else {
            return;
        };
        let Some(symbol) = symbols
            .filtered
            .get(symbols.selected)
            .and_then(|index| symbols.items.get(*index))
            .cloned()
        else {
            return;
        };
        self.record_jump();
        if self.path.as_ref() != Some(&symbol.path)
            && let Err(error) = self.open_path(&symbol.path)
        {
            self.status = format!("E: {error}");
            self.dirty = true;
            return;
        }
        self.cursor = self.offset_at_utf16(symbol.line, symbol.character);
        self.status = format!("symbol {}", symbol.name);
        self.dirty = true;
    }

    fn apply_workspace_symbols_response(&mut self, result: &serde_json::Value) {
        let Some(items) = result.as_array() else {
            self.status = "lsp: no workspace symbols".to_owned();
            self.workspace_symbols = None;
            return;
        };
        let mut parsed = Vec::new();
        for item in items {
            let Some(name) = item.get("name").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some(location) = item.get("location") else {
                continue;
            };
            let Some(uri) = location.get("uri").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some(path) = lsp::uri_to_path(uri) else {
                continue;
            };
            let range = location.get("range").or_else(|| item.get("range"));
            let Some(start) = range
                .and_then(|range| range.get("start"))
                .and_then(serde_json::Value::as_object)
            else {
                continue;
            };
            let line = start
                .get("line")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;
            let character = start
                .get("character")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;
            let detail = item
                .get("containerName")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            parsed.push(WorkspaceSymbolItem {
                name: name.to_owned(),
                detail,
                path,
                line,
                character,
            });
        }
        if parsed.is_empty() {
            self.status = "lsp: no workspace symbols".to_owned();
            self.workspace_symbols = None;
        } else {
            self.workspace_symbols = Some(WorkspaceSymbolState {
                filtered: (0..parsed.len()).collect(),
                items: parsed,
                query: String::new(),
                selected: 0,
            });
            self.status = "workspace symbols".to_owned();
        }
    }

    pub(crate) fn request_document_symbols(&mut self) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_symbols();
        self.hover = None;
        self.ui.diagnostic_peek = None;
        let Some(uri) = self.path.as_ref().map(|path| lsp_worker::file_uri(path)) else {
            return;
        };
        let params = serde_json::json!({"textDocument": {"uri": uri}});
        if self.send_lsp_request(
            LspRequestKind::DocumentSymbol,
            "textDocument/documentSymbol",
            params,
        ) {
            self.status = "document symbols".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn move_document_symbol(&mut self, delta: isize) {
        if let Some(symbols) = self.document_symbols.as_mut() {
            symbols.selected = symbols
                .selected
                .saturating_add_signed(delta)
                .min(symbols.items.len().saturating_sub(1));
            self.dirty = true;
        }
    }

    pub(crate) fn apply_document_symbol(&mut self) {
        let Some(symbols) = self.document_symbols.take() else {
            return;
        };
        let Some(symbol) = symbols.items.get(symbols.selected).cloned() else {
            return;
        };
        self.record_jump();
        self.cursor = self.offset_at_utf16(symbol.line, symbol.character);
        self.status = format!("symbol {}", symbol.name);
        self.dirty = true;
    }

    pub(crate) fn move_reference(&mut self, delta: isize) {
        if let Some(references) = self.references.as_mut() {
            references.selected = references
                .selected
                .saturating_add_signed(delta)
                .min(references.items.len().saturating_sub(1));
            self.dirty = true;
        }
    }

    pub(crate) fn apply_reference(&mut self) {
        let Some(references) = self.references.take() else {
            return;
        };
        let Some(reference) = references.items.get(references.selected).cloned() else {
            return;
        };
        self.record_jump();
        if self.path.as_ref() != Some(&reference.path)
            && let Err(error) = self.open_path(&reference.path)
        {
            self.status = format!("E: {error}");
            self.dirty = true;
            return;
        }
        self.cursor = self.offset_at_utf16(reference.line, reference.character);
        self.status = format!(
            "reference {}:{}",
            reference.path.display(),
            reference.line + 1
        );
        self.dirty = true;
    }

    pub(crate) fn move_call_hierarchy(&mut self, delta: isize) {
        if let Some(calls) = self.call_hierarchy.as_mut() {
            calls.selected = calls
                .selected
                .saturating_add_signed(delta)
                .min(calls.items.len().saturating_sub(1));
            self.dirty = true;
        }
    }

    pub(crate) fn apply_call_hierarchy(&mut self) {
        let Some(calls) = self.call_hierarchy.take() else {
            return;
        };
        let Some(item) = calls.items.get(calls.selected).cloned() else {
            return;
        };
        self.record_jump();
        if self.path.as_ref() != Some(&item.path)
            && let Err(error) = self.open_path(&item.path)
        {
            self.status = format!("E: {error}");
            self.dirty = true;
            return;
        }
        self.cursor = self.offset_at_utf16(item.line, item.character);
        let direction = match calls.direction {
            CallHierarchyDirection::Incoming => "incoming",
            CallHierarchyDirection::Outgoing => "outgoing",
        };
        self.status = format!("{direction} call {}:{}", item.name, item.line + 1);
        self.dirty = true;
    }

    pub(crate) fn request_formatting(&mut self) -> bool {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_symbols();
        self.language
            .pending_lsp
            .retain(|_, kind| *kind != LspRequestKind::Formatting);
        self.prune_edit_requests();
        self.hover = None;
        self.ui.diagnostic_peek = None;
        let Some(path) = self.path.clone() else {
            return false;
        };
        let uri = lsp_worker::file_uri(&path);
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 4, "insertSpaces": true}
        });
        let sent = self.send_lsp_request(
            LspRequestKind::Formatting,
            "textDocument/formatting",
            params,
        );
        if sent {
            self.status = "formatting".to_owned();
        }
        self.dirty = true;
        sent
    }

    pub(crate) fn request_format_after_save(&mut self) {
        if !self.settings.editor.format_on_save
            || self.suppress_format_on_save
            || self.format_on_save_pending
            || self.active_lsp_worker().is_none()
        {
            return;
        }
        if self.request_formatting() {
            self.format_on_save_pending = true;
        }
    }

    pub(crate) fn move_code_action(&mut self, delta: isize) {
        if let Some(actions) = self.code_actions.as_mut() {
            actions.selected = actions
                .selected
                .saturating_add_signed(delta)
                .min(actions.items.len().saturating_sub(1));
            self.dirty = true;
        }
    }

    pub(crate) fn apply_code_action(&mut self) {
        let Some(actions) = self.code_actions.take() else {
            return;
        };
        let Some(item) = actions.items.get(actions.selected) else {
            return;
        };
        if let Some(reason) = &item.disabled {
            self.status = format!("E: {reason}");
        } else if let Some(edit) = &item.edit {
            match edit.apply(self) {
                Ok(()) => self.status = format!("applied {}", item.title),
                Err(error) => self.status = format!("E: {error}"),
            }
        } else {
            self.status = "E: command code actions are not supported".to_owned();
        }
        self.dirty = true;
    }

    fn apply_code_action_response(
        &mut self,
        result: &serde_json::Value,
        base: Option<HashMap<PathBuf, u64>>,
    ) {
        if result.is_null() {
            self.status = "lsp: no code actions".to_owned();
            self.code_actions = None;
            return;
        }
        let Some(items) = result.as_array() else {
            self.status = "E: lsp: invalid code action response".to_owned();
            return;
        };
        let items: Vec<CodeActionItem> = items
            .iter()
            .filter_map(|item| {
                let title = item.get("title")?.as_str()?.to_owned();
                let mut disabled = item
                    .get("disabled")
                    .and_then(|disabled| disabled.get("reason"))
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned);
                let edit = match item.get("edit") {
                    Some(edit) => match WorkspaceEdit::from_lsp(edit)
                        .map(|edit| edit.with_base_revisions(base.clone()))
                    {
                        Ok(edit) => Some(edit),
                        Err(error) => {
                            disabled = Some(format!("invalid edit: {error}"));
                            None
                        }
                    },
                    None => None,
                };
                Some(CodeActionItem {
                    title,
                    edit,
                    disabled,
                })
            })
            .collect();
        if items.is_empty() {
            self.status = "lsp: no code actions".to_owned();
            self.code_actions = None;
        } else {
            self.code_actions = Some(CodeActionState { items, selected: 0 });
            self.status = "code actions".to_owned();
        }
    }

    pub(crate) fn apply_completion(&mut self) {
        let Some(completion) = self.completion.take() else {
            return;
        };
        let Some(item) = completion
            .filtered
            .get(completion.selected)
            .and_then(|index| completion.items.get(*index))
        else {
            return;
        };
        let label = item.label.clone();
        let insert = item.insert.clone();
        let replace = item.replace;
        let mut snippet = item.snippet.clone();
        self.snippet = None;
        if let Some((start_line, start_char, end_line, end_char)) = replace {
            let start = self.offset_at_utf16(start_line, start_char);
            let end = self.offset_at_utf16(end_line, end_char).max(start);
            self.editor.set_selection_bytes(start, end);
            self.editor.delete(start..end);
            self.editor.insert(start, &insert);
            self.cursor = start + insert.len();
            self.modified = true;
            if let Some(snippet) = snippet.take() {
                self.start_snippet(&snippet, start);
            }
        } else {
            let selections = self.editor.selection_bytes();
            if selections.len() != 1 {
                snippet = None;
            }
            let start = selections
                .first()
                .map(|(anchor, head)| (*anchor).min(*head))
                .unwrap_or(self.cursor);
            self.insert_text(&insert);
            if let Some(snippet) = snippet {
                self.start_snippet(&snippet, start);
            }
        }
        self.status = format!("completed {label}");
        self.dirty = true;
    }

    fn start_snippet(&mut self, snippet: &Snippet, base: usize) {
        self.snippet = SnippetSession::new(snippet.offset_stops(base));
        self.goto_snippet_stop();
    }

    fn goto_snippet_stop(&mut self) {
        let ranges = self
            .snippet
            .as_ref()
            .map(SnippetSession::mirror_ranges)
            .unwrap_or_default();
        let Some((start, _)) = ranges.first().copied() else {
            self.snippet = None;
            return;
        };
        let mirrors = ranges.len();
        self.editor.set_selections_bytes(ranges);
        self.cursor = start;
        if mirrors > 1 {
            self.status = format!("snippet stop ({mirrors} mirrors)");
        }
        self.dirty = true;
    }

    fn select_snippet_groups(&mut self, ranges: Vec<(usize, usize)>) {
        if let Some((start, _)) = ranges.first().copied() {
            self.editor.set_selections_bytes(ranges);
            self.cursor = start;
            self.dirty = true;
        }
    }

    pub(crate) fn next_snippet_stop(&mut self) -> bool {
        let Some(mut session) = self.snippet.take() else {
            return false;
        };
        if session.next().is_some() {
            let ranges = session.mirror_ranges();
            self.snippet = Some(session);
            self.select_snippet_groups(ranges);
            return true;
        }
        let end = session.current().map(|(_, end)| end).unwrap_or(self.cursor);
        self.cursor = end;
        self.editor.set_selection_bytes(end, end);
        self.dirty = true;
        true
    }

    pub(crate) fn previous_snippet_stop(&mut self) -> bool {
        let Some(mut session) = self.snippet.take() else {
            return false;
        };
        let moved = session.previous();
        if moved.is_some() {
            let ranges = session.mirror_ranges();
            self.snippet = Some(session);
            self.select_snippet_groups(ranges);
        } else {
            self.snippet = Some(session);
        }
        true
    }

    pub(crate) fn cancel_snippet(&mut self) {
        self.snippet = None;
        self.dirty = true;
    }

    pub(crate) fn move_completion(&mut self, delta: isize) {
        if let Some(completion) = self.completion.as_mut() {
            completion.selected = completion
                .selected
                .saturating_add_signed(delta)
                .min(completion.filtered.len().saturating_sub(1));
            self.dirty = true;
        }
        self.request_completion_resolve();
    }

    pub(crate) fn filter_completion(&mut self) {
        if let Some(completion) = self.completion.as_mut() {
            let query = completion.query.to_lowercase();
            completion.filtered = completion
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.filter_text.to_lowercase().contains(&query))
                .map(|(index, _)| index)
                .collect();
            let preferred = if query.is_empty() {
                completion
                    .filtered
                    .iter()
                    .position(|index| completion.items[*index].preselect)
                    .unwrap_or(0)
            } else {
                0
            };
            completion.selected = preferred.min(completion.filtered.len().saturating_sub(1));
            self.dirty = true;
        }
        self.request_completion_resolve();
    }
}

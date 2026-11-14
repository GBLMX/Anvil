//! LSP request state, completion, definition, and hover UI behavior.

use super::{
    CallHierarchyDirection, CodeActionItem, CodeActionState, CompletionItem, CompletionState,
    LspRequestKind, Model, OverlayKind, ReferenceItem, ReferenceState, SignatureHelpState,
    SignatureItem, Snippet, SnippetSession, SymbolItem, SymbolState, WorkspaceEdit,
    WorkspaceSymbolItem, WorkspaceSymbolState,
};
use anvil_core::TextBuffer;
use anvil_lsp_client as lsp;
use anvil_session::ui::InputMode;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

mod completion;
mod responses;
mod settings;
mod symbols;
mod transport;
#[cfg(test)]
pub(crate) use settings::lsp_settings;
pub(crate) use settings::lsp_settings_with_service;

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
    pub(crate) fn jump_diagnostic(&mut self, forward: bool) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.close_hover();
        let Some(path) = self.session.path.clone() else {
            self.status.text = "E: no file".to_owned();
            self.file.dirty = true;
            return;
        };
        let Some(diagnostics) = self.language.diagnostics.by_path.get(&path) else {
            self.status.text = "no diagnostics".to_owned();
            self.file.dirty = true;
            return;
        };
        if diagnostics.is_empty() {
            self.status.text = "no diagnostics".to_owned();
            self.file.dirty = true;
            return;
        }
        let current = self
            .session
            .editor
            .index()
            .position(self.session.editor.text(), self.session.cursor)
            .map(|position| position.line)
            .unwrap_or(0);
        let primary: Vec<&anvil_lsp_client::Diagnostic> = if diagnostics
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
        self.session.cursor = self.offset_at_utf16(line, character);
        self.open_diagnostic_peek(path, line as u32, character as u32);
        let index = positions
            .iter()
            .position(|item| *item == (line, character))
            .unwrap_or(0);
        self.status.text = format!("diagnostic {}/{}", index + 1, positions.len());
        self.file.dirty = true;
    }

    pub(crate) fn notify_lsp_save(&mut self, path: &std::path::Path, text: &str) {
        let sent = if let Some(key) = self.lsp_key(path)
            && let Some(worker) = self.language.lsp.workers.get(&key)
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
        self.language.lsp_requests.pending.clear();
        self.language.lsp_requests.pending_edit_revision.clear();
        self.language.lsp_requests.pending_edit_path.clear();
        self.language.lsp_requests.pending_edit_snapshot.clear();
        self.overlays.lsp_results.signature_help = None;
        self.overlays.lsp_results.workspace_symbols = None;
        self.close_overlay(OverlayKind::WorkspaceSymbols);
        self.overlays.lsp_results.references = None;
        self.close_overlay(OverlayKind::References);
        self.overlays.lsp_results.document_highlights = None;
        self.overlays.lsp_results.call_hierarchy = None;
        self.close_overlay(OverlayKind::CallHierarchy);
        self.clear_folds();
        self.clear_semantic_tokens();
        self.overlays.lsp_results.inlay_hints.clear();
        self.overlays.lsp_results.inlay_pending = None;
        self.overlays.lsp_results.inlay_checked = None;
        self.file.dirty = true;
    }

    pub(crate) fn cancel_signature_help(&mut self) {
        self.language
            .lsp_requests
            .pending
            .retain(|_, kind| *kind != LspRequestKind::SignatureHelp);
        self.overlays.lsp_results.signature_help = None;
        self.prune_edit_requests();
        self.file.dirty = true;
    }

    pub(crate) fn request_signature_help(&mut self, trigger: char) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_symbols();
        self.cancel_signature_help();
        self.close_hover();
        self.close_diagnostic_peek();
        let Some(uri) = self
            .session
            .path
            .as_ref()
            .map(|path| anvil_lsp_client::file_uri(path))
        else {
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
            self.status.text = "signature".to_owned();
        }
        self.file.dirty = true;
    }

    pub(crate) fn prune_edit_requests(&mut self) {
        let pending: Vec<u64> = self.language.lsp_requests.pending.keys().copied().collect();
        self.language
            .lsp_requests
            .pending_edit_revision
            .retain(|id, _| pending.contains(id));
        self.language
            .lsp_requests
            .pending_edit_path
            .retain(|id, _| pending.contains(id));
        self.language
            .lsp_requests
            .pending_edit_snapshot
            .retain(|id, _| pending.contains(id));
    }

    pub(crate) fn close_hover(&mut self) {
        if self.overlays.lsp_results.hover.take().is_some() {
            self.close_overlay(OverlayKind::Hover);
            self.file.dirty = true;
        }
    }

    pub(crate) fn cancel_code_actions(&mut self) {
        self.language.lsp_requests.pending.retain(|_, kind| {
            !matches!(
                kind,
                LspRequestKind::CodeAction | LspRequestKind::DocumentSymbol
            )
        });
        self.prune_edit_requests();
        self.overlays.lsp_results.code_actions = None;
        self.close_overlay(OverlayKind::CodeActions);
        self.overlays.lsp_results.document_symbols = None;
        self.close_overlay(OverlayKind::DocumentSymbols);
        self.file.dirty = true;
    }

    pub(crate) fn request_definition(&mut self) {
        self.request_goto(
            LspRequestKind::Definition,
            "textDocument/definition",
            "definition",
        );
    }

    pub(crate) fn request_declaration(&mut self) {
        self.request_goto(
            LspRequestKind::Declaration,
            "textDocument/declaration",
            "declaration",
        );
    }

    pub(crate) fn request_type_definition(&mut self) {
        self.request_goto(
            LspRequestKind::TypeDefinition,
            "textDocument/typeDefinition",
            "type definition",
        );
    }

    pub(crate) fn request_implementation(&mut self) {
        self.request_goto(
            LspRequestKind::Implementation,
            "textDocument/implementation",
            "implementation",
        );
    }

    fn request_goto(&mut self, kind: LspRequestKind, method: &str, label: &str) {
        self.cancel_completion();
        self.close_diagnostic_peek();
        self.cancel_code_actions();
        let Some(uri) = self
            .session
            .path
            .as_ref()
            .map(|path| anvil_lsp_client::file_uri(path))
        else {
            return;
        };
        let Some((line, character)) = self.lsp_position() else {
            return;
        };
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        });
        if self.send_lsp_request(kind, method, params) {
            self.status.text = label.to_owned();
        }
        self.file.dirty = true;
    }

    pub(crate) fn request_references(&mut self) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_symbols();
        self.cancel_workspace_symbols();
        self.cancel_references();
        self.close_hover();
        self.close_diagnostic_peek();
        let Some(uri) = self
            .session
            .path
            .as_ref()
            .map(|path| anvil_lsp_client::file_uri(path))
        else {
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
            self.status.text = "references".to_owned();
        }
        self.file.dirty = true;
    }

    pub(crate) fn request_document_highlights(&mut self) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_highlights();
        self.close_hover();
        self.close_diagnostic_peek();
        let Some(uri) = self
            .session
            .path
            .as_ref()
            .map(|path| anvil_lsp_client::file_uri(path))
        else {
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
            self.status.text = "highlights".to_owned();
        }
        self.file.dirty = true;
    }

    pub(crate) fn cancel_document_highlights(&mut self) {
        self.language
            .lsp_requests
            .pending
            .retain(|_, kind| *kind != LspRequestKind::DocumentHighlight);
        self.prune_edit_requests();
        self.overlays.lsp_results.document_highlights = None;
        self.file.dirty = true;
    }

    pub(crate) fn request_call_hierarchy(&mut self, direction: CallHierarchyDirection) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_highlights();
        self.cancel_call_hierarchy();
        self.close_hover();
        self.close_diagnostic_peek();
        let Some(uri) = self
            .session
            .path
            .as_ref()
            .map(|path| anvil_lsp_client::file_uri(path))
        else {
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
            self.status.text = "call hierarchy".to_owned();
        }
        self.file.dirty = true;
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
            self.status.text = label.to_owned();
        }
        self.file.dirty = true;
    }

    pub(crate) fn cancel_call_hierarchy(&mut self) {
        self.language.lsp_requests.pending.retain(|_, kind| {
            !matches!(
                kind,
                LspRequestKind::CallHierarchyPrepare(_)
                    | LspRequestKind::CallHierarchyIncoming
                    | LspRequestKind::CallHierarchyOutgoing
            )
        });
        self.prune_edit_requests();
        self.overlays.lsp_results.call_hierarchy = None;
        self.close_overlay(OverlayKind::CallHierarchy);
        self.file.dirty = true;
    }

    pub(crate) fn request_hover(&mut self) {
        self.cancel_completion();
        self.close_diagnostic_peek();
        self.cancel_code_actions();
        let Some(uri) = self
            .session
            .path
            .as_ref()
            .map(|path| anvil_lsp_client::file_uri(path))
        else {
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
            self.status.text = "hover".to_owned();
        }
        self.file.dirty = true;
    }

    pub(crate) fn start_rename(&mut self) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.close_hover();
        self.close_diagnostic_peek();
        self.ui.input_mode = InputMode::Rename {
            name: String::new(),
        };
        self.status.text = "rename symbol".to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn request_rename(&mut self, new_name: &str) {
        if new_name.trim().is_empty() {
            self.status.text = "E: empty rename".to_owned();
            self.file.dirty = true;
            return;
        }
        let Some(uri) = self
            .session
            .path
            .as_ref()
            .map(|path| anvil_lsp_client::file_uri(path))
        else {
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
            self.status.text = "renaming".to_owned();
        }
        self.file.dirty = true;
    }

    pub(crate) fn request_code_actions(&mut self) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.close_hover();
        self.close_diagnostic_peek();
        let Some(path) = self.session.path.clone() else {
            return;
        };
        let uri = anvil_lsp_client::file_uri(&path);
        let Some((line, _character)) = self.lsp_position() else {
            return;
        };
        let line_start = self
            .session
            .editor
            .index()
            .line_to_offset(line as usize)
            .unwrap_or(0);
        let line_end = self
            .session
            .editor
            .index()
            .offset_to_line(line_start)
            .map(|position| position.end)
            .unwrap_or(line_start);
        let line_units = self
            .session
            .editor
            .index()
            .utf16_column(self.session.editor.text(), line_end)
            .unwrap_or(0);
        let diagnostics = self
            .language
            .diagnostics
            .by_path
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
            self.status.text = "code actions".to_owned();
        }
        self.file.dirty = true;
    }

    pub(crate) fn cancel_references(&mut self) {
        self.language
            .lsp_requests
            .pending
            .retain(|_, kind| *kind != LspRequestKind::References);
        self.overlays.lsp_results.references = None;
        self.close_overlay(OverlayKind::References);
        self.file.dirty = true;
    }

    pub(crate) fn cancel_document_symbols(&mut self) {
        self.language
            .lsp_requests
            .pending
            .retain(|_, kind| *kind != LspRequestKind::DocumentSymbol);
        self.overlays.lsp_results.document_symbols = None;
        self.close_overlay(OverlayKind::DocumentSymbols);
        self.file.dirty = true;
    }

    pub(crate) fn cancel_workspace_symbols(&mut self) {
        self.language
            .lsp_requests
            .pending
            .retain(|_, kind| *kind != LspRequestKind::WorkspaceSymbol);
        self.overlays.lsp_results.workspace_symbols = None;
        self.close_overlay(OverlayKind::WorkspaceSymbols);
        self.prune_edit_requests();
        self.file.dirty = true;
    }

    pub(crate) fn request_formatting(&mut self) -> bool {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_symbols();
        self.language
            .lsp_requests
            .pending
            .retain(|_, kind| *kind != LspRequestKind::Formatting);
        self.prune_edit_requests();
        self.close_hover();
        self.close_diagnostic_peek();
        let Some(path) = self.session.path.clone() else {
            return false;
        };
        let uri = anvil_lsp_client::file_uri(&path);
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
            self.status.text = "formatting".to_owned();
        }
        self.file.dirty = true;
        sent
    }

    pub(crate) fn request_format_after_save(&mut self) {
        if !self.config.settings.editor.format_on_save
            || self.language.lsp_requests.suppress_format_on_save
            || self.language.lsp_requests.format_on_save_pending
            || self.active_lsp_worker().is_none()
        {
            return;
        }
        if self.request_formatting() {
            self.language.lsp_requests.format_on_save_pending = true;
        }
    }

    pub(crate) fn move_code_action(&mut self, delta: isize) {
        if let Some(actions) = self.overlays.lsp_results.code_actions.as_mut() {
            actions.selected = actions
                .selected
                .saturating_add_signed(delta)
                .min(actions.items.len().saturating_sub(1));
            self.file.dirty = true;
        }
    }

    pub(crate) fn apply_code_action(&mut self) {
        let Some(actions) = self.overlays.lsp_results.code_actions.take() else {
            return;
        };
        self.close_overlay(OverlayKind::CodeActions);
        let Some(item) = actions.items.get(actions.selected) else {
            return;
        };
        if let Some(reason) = &item.disabled {
            self.status.text = format!("E: {reason}");
        } else if let Some(edit) = &item.edit {
            match edit.apply(self) {
                Ok(()) => self.status.text = format!("applied {}", item.title),
                Err(error) => self.status.text = format!("E: {error}"),
            }
        } else {
            self.status.text = "E: command code actions are not supported".to_owned();
        }
        self.file.dirty = true;
    }
}

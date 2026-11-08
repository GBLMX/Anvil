//! LSP response handling and DTO application.

use super::*;
use crate::{
    CallHierarchyDirection, CallHierarchyItem, CallHierarchyState, DocumentHighlightItem,
    DocumentHighlightState,
};

impl Model {
    fn apply_document_symbols_response(&mut self, result: &serde_json::Value) {
        let Some(items) = result.as_array() else {
            self.status = "lsp: no document symbols".to_owned();
            self.document_symbols = None;
            return;
        };
        let items = symbols_from_lsp(items);
        if items.is_empty() {
            self.status = "lsp: no document symbols".to_owned();
            self.document_symbols = None;
        } else {
            self.document_symbols = Some(SymbolState { items, selected: 0 });
            self.status = "document symbols".to_owned();
        }
    }

    fn apply_signature_help_response(&mut self, result: &serde_json::Value) {
        let Some(signatures) = result
            .get("signatures")
            .and_then(serde_json::Value::as_array)
        else {
            self.status = "lsp: no signature".to_owned();
            self.signature_help = None;
            return;
        };
        let signatures: Vec<SignatureItem> = signatures
            .iter()
            .filter_map(|signature| {
                let label = signature
                    .get("label")
                    .and_then(serde_json::Value::as_str)?
                    .to_owned();
                let documentation = signature
                    .get("documentation")
                    .map(hover_text)
                    .unwrap_or_default();
                Some(SignatureItem {
                    label,
                    documentation,
                })
            })
            .collect();
        if signatures.is_empty() {
            self.status = "lsp: no signature".to_owned();
            self.signature_help = None;
            return;
        }
        let active_signature = result
            .get("activeSignature")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;
        let active_parameter = result
            .get("activeParameter")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;
        self.signature_help = Some(SignatureHelpState {
            active_signature: active_signature.min(signatures.len() - 1),
            active_parameter,
            signatures,
        });
        self.status = "signature".to_owned();
    }

    fn apply_formatting_response(&mut self, result: &serde_json::Value, path: &Path) {
        if result.is_null() || result.as_array().is_some_and(Vec::is_empty) {
            self.status = "lsp: no formatting changes".to_owned();
            return;
        }
        let uri = lsp_worker::file_uri(path);
        let changes = serde_json::json!({"changes": {uri: result.clone()}});
        match WorkspaceEdit::from_lsp(&changes).and_then(|edit| edit.apply(self)) {
            Ok(()) => self.status = "formatted".to_owned(),
            Err(error) => self.status = format!("E: format: {error}"),
        }
    }

    pub(crate) fn handle_lsp_response(
        &mut self,
        id: u64,
        result: serde_json::Value,
        error: Option<String>,
    ) {
        let Some(kind) = self.pending_lsp.remove(&id) else {
            return;
        };
        let edit_revision = self.pending_edit_revision.remove(&id);
        let edit_path = self.pending_edit_path.remove(&id);
        if let Some(error) = error {
            if kind == LspRequestKind::CompletionResolve {
                self.completion_resolve = None;
            }
            self.status = format!("E: lsp {kind:?}: {error}");
            self.dirty = true;
            return;
        }
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
        ) && edit_revision.is_some_and(|revision| revision != self.editor.revision())
        {
            self.status = "lsp: stale edit response ignored".to_owned();
            self.dirty = true;
            return;
        }
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
        ) && edit_path
            .as_ref()
            .is_some_and(|path| self.path.as_ref() != Some(path))
        {
            self.status = "lsp: edit response for inactive buffer".to_owned();
            self.dirty = true;
            return;
        }
        match kind {
            LspRequestKind::Completion => self.apply_completion_response(id, &result),
            LspRequestKind::CompletionResolve => {
                self.apply_completion_resolve_response(id, &result);
            }
            LspRequestKind::Definition => self.apply_definition_response(&result),
            LspRequestKind::References => self.apply_references_response(&result),
            LspRequestKind::DocumentHighlight => self.apply_document_highlights_response(&result),
            LspRequestKind::CallHierarchyPrepare(direction) => {
                self.apply_call_hierarchy_prepare_response(&result, direction);
            }
            LspRequestKind::CallHierarchyIncoming => {
                self.apply_call_hierarchy_calls_response(&result, CallHierarchyDirection::Incoming);
            }
            LspRequestKind::CallHierarchyOutgoing => {
                self.apply_call_hierarchy_calls_response(&result, CallHierarchyDirection::Outgoing);
            }
            LspRequestKind::FoldingRange => self.apply_folding_response(&result),
            LspRequestKind::Hover => self.apply_hover_response(&result),
            LspRequestKind::Rename => {
                if result.is_null() {
                    self.status = "lsp: no rename".to_owned();
                } else {
                    match WorkspaceEdit::from_lsp(&result).and_then(|edit| edit.apply(self)) {
                        Ok(()) => self.status = "renamed".to_owned(),
                        Err(error) => self.status = format!("E: rename: {error}"),
                    }
                }
            }
            LspRequestKind::CodeAction => self.apply_code_action_response(&result),
            LspRequestKind::SignatureHelp => self.apply_signature_help_response(&result),
            LspRequestKind::WorkspaceSymbol => {
                self.apply_workspace_symbols_response(&result);
            }
            LspRequestKind::InlayHint => self.apply_inlay_hints_response(id, &result),
            LspRequestKind::Formatting => {
                let Some(path) = edit_path.or_else(|| self.path.clone()) else {
                    self.status = "lsp: no formatting path".to_owned();
                    return;
                };
                self.apply_formatting_response(&result, &path);
            }
            LspRequestKind::DocumentSymbol => self.apply_document_symbols_response(&result),
        }
        self.dirty = true;
    }

    fn apply_completion_response(&mut self, id: u64, result: &serde_json::Value) {
        let items = result
            .get("items")
            .and_then(serde_json::Value::as_array)
            .or_else(|| result.as_array());
        let Some(items) = items else {
            self.status = "E: lsp: invalid completion response".to_owned();
            return;
        };
        let items: Vec<CompletionItem> = items
            .iter()
            .filter_map(|item| {
                let label = item.get("label")?.as_str()?.to_owned();
                let insert = item
                    .get("textEdit")
                    .and_then(|edit| edit.get("newText"))
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| item.get("insertText").and_then(serde_json::Value::as_str))
                    .unwrap_or(&label)
                    .to_owned();
                let detail = item
                    .get("detail")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let documentation = item
                    .get("documentation")
                    .map(hover_text)
                    .filter(|text| !text.is_empty());
                let replace = item
                    .get("textEdit")
                    .and_then(|edit| edit.get("range"))
                    .and_then(|range| {
                        let start = range.get("start")?;
                        let end = range.get("end")?;
                        Some((
                            start.get("line")?.as_u64()? as usize,
                            start.get("character")?.as_u64()? as usize,
                            end.get("line")?.as_u64()? as usize,
                            end.get("character")?.as_u64()? as usize,
                        ))
                    });
                Some(CompletionItem {
                    label,
                    insert: expand_snippet(&insert),
                    detail,
                    replace,
                    documentation,
                    raw: item.clone(),
                    resolved: false,
                })
            })
            .take(200)
            .collect();
        if items.is_empty() {
            self.status = "lsp: no completions".to_owned();
            self.completion = None;
        } else {
            let filtered = (0..items.len()).collect();
            self.completion = Some(CompletionState {
                id,
                items,
                filtered,
                query: String::new(),
                selected: 0,
            });
            self.status = "completion".to_owned();
            self.request_completion_resolve();
        }
    }

    fn apply_completion_resolve_response(&mut self, id: u64, result: &serde_json::Value) {
        let Some((pending, index)) = self.completion_resolve else {
            return;
        };
        if pending != id {
            return;
        }
        self.completion_resolve = None;
        let Some(completion) = self.completion.as_mut() else {
            return;
        };
        let Some(item) = completion.items.get_mut(index) else {
            return;
        };
        if let Some(documentation) = result.get("documentation") {
            let text = hover_text(documentation);
            item.documentation = (!text.is_empty()).then_some(text);
        }
        if let Some(detail) = result.get("detail").and_then(serde_json::Value::as_str) {
            item.detail = detail.to_owned();
        }
        item.raw = result.clone();
        item.resolved = true;
        self.dirty = true;
    }

    pub(crate) fn apply_definition_response(&mut self, result: &serde_json::Value) {
        let location = result
            .as_array()
            .and_then(|locations| locations.first())
            .unwrap_or(result);
        let Some(uri) = location
            .get("uri")
            .or_else(|| location.get("targetUri"))
            .and_then(serde_json::Value::as_str)
        else {
            self.status = "lsp: no definition".to_owned();
            return;
        };
        let Some(path) = lsp::uri_to_path(uri) else {
            self.status = "E: lsp: invalid definition URI".to_owned();
            return;
        };
        let range = location
            .get("range")
            .or_else(|| location.get("targetSelectionRange"))
            .or_else(|| location.get("targetRange"));
        let line = range
            .and_then(|range| range.get("start"))
            .and_then(|start| start.get("line"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;
        let character = range
            .and_then(|range| range.get("start"))
            .and_then(|start| start.get("character"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;
        if self.path.as_ref() != Some(&path) {
            if let Err(error) = self.open_path(&path) {
                self.status = format!("E: {error}");
                return;
            }
        }
        self.cursor = self.offset_at_utf16(line, character);
        self.status = format!("definition {}", path.display());
    }

    pub(crate) fn apply_references_response(&mut self, result: &serde_json::Value) {
        let Some(items) = result.as_array() else {
            self.status = "lsp: no references".to_owned();
            self.references = None;
            return;
        };
        let mut references = Vec::new();
        for item in items {
            if references.len() >= 4096 {
                break;
            }
            let Some(uri) = item
                .get("uri")
                .or_else(|| item.get("targetUri"))
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let Some(path) = lsp::uri_to_path(uri) else {
                continue;
            };
            let range = item
                .get("range")
                .or_else(|| item.get("targetSelectionRange"))
                .or_else(|| item.get("targetRange"));
            let Some(start) = range
                .and_then(|range| range.get("start"))
                .and_then(serde_json::Value::as_object)
            else {
                continue;
            };
            references.push(ReferenceItem {
                path,
                line: start
                    .get("line")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as usize,
                character: start
                    .get("character")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as usize,
            });
        }
        references.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.line.cmp(&right.line))
                .then(left.character.cmp(&right.character))
        });
        if references.is_empty() {
            self.status = "lsp: no references".to_owned();
            self.references = None;
        } else {
            let count = references.len();
            self.references = Some(ReferenceState {
                items: references,
                selected: 0,
            });
            self.status = format!("references: {count}");
        }
    }

    pub(crate) fn apply_document_highlights_response(&mut self, result: &serde_json::Value) {
        let Some(items) = result.as_array() else {
            self.status = "lsp: no highlights".to_owned();
            self.document_highlights = None;
            return;
        };
        let mut highlights = Vec::new();
        for item in items {
            if highlights.len() >= 4096 {
                break;
            }
            let Some(range) = item.get("range") else {
                continue;
            };
            let Some(start) = range.get("start") else {
                continue;
            };
            let Some(end) = range.get("end") else {
                continue;
            };
            let start_line = start
                .get("line")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;
            let start_character = start
                .get("character")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;
            let end_line = end
                .get("line")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;
            let end_character = end
                .get("character")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;
            let start = self.offset_at_utf16(start_line, start_character);
            let end = self.offset_at_utf16(end_line, end_character);
            if end <= start {
                continue;
            }
            highlights.push(DocumentHighlightItem {
                start,
                end,
                kind: item
                    .get("kind")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(1) as u8,
            });
        }
        highlights.sort_by_key(|highlight| (highlight.start, highlight.end));
        highlights.dedup_by_key(|highlight| (highlight.start, highlight.end));
        if highlights.is_empty() {
            self.status = "lsp: no highlights".to_owned();
            self.document_highlights = None;
        } else {
            let count = highlights.len();
            self.status = format!("highlights: {count}");
            self.document_highlights = Some(DocumentHighlightState {
                revision: self.editor.revision(),
                items: highlights,
            });
        }
    }

    fn apply_call_hierarchy_prepare_response(
        &mut self,
        result: &serde_json::Value,
        direction: CallHierarchyDirection,
    ) {
        let Some(item) = result.as_array().and_then(|items| items.first()).cloned() else {
            self.status = "lsp: no call hierarchy item".to_owned();
            return;
        };
        self.request_call_hierarchy_calls(item, direction);
    }

    pub(crate) fn apply_call_hierarchy_calls_response(
        &mut self,
        result: &serde_json::Value,
        direction: CallHierarchyDirection,
    ) {
        let Some(calls) = result.as_array() else {
            self.status = "lsp: no calls".to_owned();
            self.call_hierarchy = None;
            return;
        };
        let nested = match direction {
            CallHierarchyDirection::Incoming => "from",
            CallHierarchyDirection::Outgoing => "to",
        };
        let mut items = Vec::new();
        for call in calls {
            if items.len() >= 4096 {
                break;
            }
            let Some(item) = call.get(nested) else {
                continue;
            };
            let Some(item) = call_hierarchy_item_from_lsp(item) else {
                continue;
            };
            items.push(item);
        }
        items.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.line.cmp(&right.line))
                .then(left.character.cmp(&right.character))
                .then(left.name.cmp(&right.name))
        });
        items.dedup_by(|left, right| {
            left.path == right.path && left.line == right.line && left.character == right.character
        });
        if items.is_empty() {
            self.status = "lsp: no calls".to_owned();
            self.call_hierarchy = None;
        } else {
            let count = items.len();
            let label = match direction {
                CallHierarchyDirection::Incoming => "incoming",
                CallHierarchyDirection::Outgoing => "outgoing",
            };
            self.status = format!("{label} calls: {count}");
            self.call_hierarchy = Some(CallHierarchyState {
                direction,
                items,
                selected: 0,
            });
        }
    }

    fn apply_hover_response(&mut self, result: &serde_json::Value) {
        let Some(contents) = result.get("contents") else {
            self.status = "lsp: no hover".to_owned();
            return;
        };
        let text = hover_text(contents);
        if text.is_empty() {
            self.status = "lsp: no hover".to_owned();
        } else {
            self.hover = Some(text);
            self.status = "hover".to_owned();
        }
    }
}

fn call_hierarchy_item_from_lsp(item: &serde_json::Value) -> Option<CallHierarchyItem> {
    let name = item.get("name")?.as_str()?.to_owned();
    let uri = item.get("uri")?.as_str()?;
    let path = lsp::uri_to_path(uri)?;
    let range = item.get("selectionRange").or_else(|| item.get("range"))?;
    let start = range.get("start")?;
    Some(CallHierarchyItem {
        name,
        path,
        line: start
            .get("line")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize,
        character: start
            .get("character")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize,
    })
}

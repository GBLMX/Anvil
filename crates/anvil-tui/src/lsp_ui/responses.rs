//! LSP response handling and DTO application.

use super::*;
use crate::{
    CallHierarchyDirection, CallHierarchyItem, CallHierarchyState, DocumentHighlightItem,
    DocumentHighlightState,
};
use std::collections::HashMap;
use std::path::PathBuf;

impl Model {
    fn apply_document_symbols_response(&mut self, result: &serde_json::Value) {
        let Some(items) = result.as_array() else {
            self.status.text = "lsp: no document symbols".to_owned();
            self.overlays.lsp_results.document_symbols = None;
            return;
        };
        let items = symbols_from_lsp(items);
        if items.is_empty() {
            self.status.text = "lsp: no document symbols".to_owned();
            self.overlays.lsp_results.document_symbols = None;
        } else {
            self.overlays.lsp_results.document_symbols = Some(SymbolState { items, selected: 0 });
            self.status.text = "document symbols".to_owned();
        }
    }

    fn apply_sticky_symbols_response(&mut self, result: &serde_json::Value) {
        self.overlays.lsp_results.sticky_pending = false;
        let items = result
            .as_array()
            .map(|items| symbols_from_lsp(items))
            .unwrap_or_default();
        self.overlays.lsp_results.sticky_symbols_path = self.session.path.clone();
        self.overlays.lsp_results.sticky_symbols_revision = Some(self.session.editor.revision());
        self.overlays.lsp_results.sticky_symbols = items;
        self.dirty = true;
    }

    fn apply_signature_help_response(&mut self, result: &serde_json::Value) {
        let Some(signatures) = result
            .get("signatures")
            .and_then(serde_json::Value::as_array)
        else {
            self.status.text = "lsp: no signature".to_owned();
            self.overlays.lsp_results.signature_help = None;
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
            self.status.text = "lsp: no signature".to_owned();
            self.overlays.lsp_results.signature_help = None;
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
        self.overlays.lsp_results.signature_help = Some(SignatureHelpState {
            active_signature: active_signature.min(signatures.len() - 1),
            active_parameter,
            signatures,
        });
        self.status.text = "signature".to_owned();
    }

    fn apply_formatting_response(
        &mut self,
        result: &serde_json::Value,
        path: &Path,
        base: Option<HashMap<PathBuf, u64>>,
    ) {
        if result.is_null() || result.as_array().is_some_and(Vec::is_empty) {
            self.status.text = "lsp: no formatting changes".to_owned();
            return;
        }
        let uri = lsp_worker::file_uri(path);
        let changes = serde_json::json!({"changes": {uri: result.clone()}});
        match WorkspaceEdit::from_lsp(&changes)
            .map(|edit| edit.with_base_revisions(base))
            .and_then(|edit| edit.apply(self))
        {
            Ok(()) => self.status.text = "formatted".to_owned(),
            Err(error) => self.status.text = format!("E: format: {error}"),
        }
    }

    fn clear_format_on_save(&mut self, kind: LspRequestKind) {
        if kind == LspRequestKind::Formatting {
            self.language.lsp_requests.format_on_save_pending = false;
        }
    }

    fn finish_format_on_save(&mut self) {
        if !self.language.lsp_requests.format_on_save_pending {
            return;
        }
        self.language.lsp_requests.format_on_save_pending = false;
        if !self.modified {
            return;
        }
        self.language.lsp_requests.suppress_format_on_save = true;
        let saved = self.save();
        self.language.lsp_requests.suppress_format_on_save = false;
        if saved {
            self.status.text = "formatted and saved".to_owned();
            self.dirty = true;
        }
    }

    pub(crate) fn handle_lsp_response(
        &mut self,
        id: u64,
        result: serde_json::Value,
        error: Option<String>,
    ) {
        let Some(kind) = self.language.lsp_requests.pending.remove(&id) else {
            return;
        };
        let edit_revision = self.language.lsp_requests.pending_edit_revision.remove(&id);
        let edit_path = self.language.lsp_requests.pending_edit_path.remove(&id);
        let edit_snapshot = self.language.lsp_requests.pending_edit_snapshot.remove(&id);
        if let Some(error) = error {
            if kind == LspRequestKind::CodeLens {
                self.overlays.lsp_results.code_lens = None;
                self.dirty = true;
                return;
            }
            if kind == LspRequestKind::CompletionResolve {
                self.overlays.lsp_results.completion_resolve = None;
            }
            if kind == LspRequestKind::StickySymbols {
                self.overlays.lsp_results.sticky_pending = false;
                self.dirty = true;
                return;
            }
            self.clear_format_on_save(kind);
            self.status.text = format!("E: lsp {kind:?}: {error}");
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
                | LspRequestKind::SemanticTokens
                | LspRequestKind::StickySymbols
        ) && edit_revision.is_some_and(|revision| revision != self.session.editor.revision())
        {
            if kind == LspRequestKind::StickySymbols {
                self.overlays.lsp_results.sticky_pending = false;
            }
            self.clear_format_on_save(kind);
            self.status.text = "lsp: stale edit response ignored".to_owned();
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
                | LspRequestKind::SemanticTokens
                | LspRequestKind::CodeLens
                | LspRequestKind::StickySymbols
        ) && edit_path
            .as_ref()
            .is_some_and(|path| self.session.path.as_ref() != Some(path))
        {
            if kind == LspRequestKind::StickySymbols {
                self.overlays.lsp_results.sticky_pending = false;
            }
            self.clear_format_on_save(kind);
            self.status.text = "lsp: edit response for inactive buffer".to_owned();
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
            LspRequestKind::SemanticTokens => self.apply_semantic_tokens_response(&result),
            LspRequestKind::CodeLens => self.apply_code_lens_response(&result, edit_revision),
            LspRequestKind::Hover => self.apply_hover_response(&result),
            LspRequestKind::Rename => {
                if result.is_null() {
                    self.status.text = "lsp: no rename".to_owned();
                } else {
                    match WorkspaceEdit::from_lsp(&result)
                        .map(|edit| edit.with_base_revisions(edit_snapshot))
                        .and_then(|edit| edit.apply(self))
                    {
                        Ok(()) => self.status.text = "renamed".to_owned(),
                        Err(error) => self.status.text = format!("E: rename: {error}"),
                    }
                }
            }
            LspRequestKind::CodeAction => self.apply_code_action_response(&result, edit_snapshot),
            LspRequestKind::SignatureHelp => self.apply_signature_help_response(&result),
            LspRequestKind::WorkspaceSymbol => {
                self.apply_workspace_symbols_response(&result);
            }
            LspRequestKind::InlayHint => self.apply_inlay_hints_response(id, &result),
            LspRequestKind::ExecuteCommand => {
                self.status.text = "code lens command executed".to_owned();
                self.request_code_lens();
            }
            LspRequestKind::Formatting => {
                let Some(path) = edit_path.or_else(|| self.session.path.clone()) else {
                    self.status.text = "lsp: no formatting path".to_owned();
                    return;
                };
                self.apply_formatting_response(&result, &path, edit_snapshot);
                self.finish_format_on_save();
            }
            LspRequestKind::DocumentSymbol => self.apply_document_symbols_response(&result),
            LspRequestKind::StickySymbols => self.apply_sticky_symbols_response(&result),
        }
        self.dirty = true;
    }

    fn apply_completion_response(&mut self, id: u64, result: &serde_json::Value) {
        let items = result
            .get("items")
            .and_then(serde_json::Value::as_array)
            .or_else(|| result.as_array());
        let Some(items) = items else {
            self.status.text = "E: lsp: invalid completion response".to_owned();
            return;
        };
        let mut items: Vec<CompletionItem> = items
            .iter()
            .take(2000)
            .filter_map(|item| {
                let label = item.get("label")?.as_str()?.to_owned();
                let filter_text = item
                    .get("filterText")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&label)
                    .to_owned();
                let sort_text = item
                    .get("sortText")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&label)
                    .to_owned();
                let preselect = item
                    .get("preselect")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let insert = item
                    .get("textEdit")
                    .and_then(|edit| edit.get("newText"))
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| item.get("insertText").and_then(serde_json::Value::as_str))
                    .unwrap_or(&label)
                    .to_owned();
                let snippet = (item
                    .get("insertTextFormat")
                    .and_then(serde_json::Value::as_u64)
                    == Some(2))
                .then(|| crate::snippet::parse(&insert).ok())
                .flatten();
                let insert = snippet
                    .as_ref()
                    .map(|snippet| snippet.text.clone())
                    .unwrap_or_else(|| expand_snippet(&insert));
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
                    filter_text,
                    sort_text,
                    preselect,
                    insert,
                    snippet,
                    detail,
                    replace,
                    documentation,
                    raw: item.clone(),
                    resolved: false,
                })
            })
            .collect();
        items.sort_by(|left, right| {
            left.sort_text
                .cmp(&right.sort_text)
                .then_with(|| left.label.cmp(&right.label))
        });
        items.truncate(200);
        if items.is_empty() {
            self.status.text = "lsp: no completions".to_owned();
            self.overlays.lsp_results.completion = None;
        } else {
            let filtered = (0..items.len()).collect();
            let selected = items.iter().position(|item| item.preselect).unwrap_or(0);
            self.overlays.lsp_results.completion = Some(CompletionState {
                id,
                items,
                filtered,
                query: String::new(),
                selected,
            });
            self.status.text = "completion".to_owned();
            self.request_completion_resolve();
        }
    }

    fn apply_completion_resolve_response(&mut self, id: u64, result: &serde_json::Value) {
        let Some((pending, index)) = self.overlays.lsp_results.completion_resolve else {
            return;
        };
        if pending != id {
            return;
        }
        self.overlays.lsp_results.completion_resolve = None;
        let Some(completion) = self.overlays.lsp_results.completion.as_mut() else {
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
            self.status.text = "lsp: no definition".to_owned();
            return;
        };
        let Some(path) = lsp::uri_to_path(uri) else {
            self.status.text = "E: lsp: invalid definition URI".to_owned();
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
        self.record_jump();
        if self.session.path.as_ref() != Some(&path) {
            if let Err(error) = self.open_path(&path) {
                self.status.text = format!("E: {error}");
                return;
            }
        }
        self.session.cursor = self.offset_at_utf16(line, character);
        self.status.text = format!("definition {}", path.display());
    }

    pub(crate) fn apply_references_response(&mut self, result: &serde_json::Value) {
        let Some(items) = result.as_array() else {
            self.status.text = "lsp: no references".to_owned();
            self.overlays.lsp_results.references = None;
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
            self.status.text = "lsp: no references".to_owned();
            self.overlays.lsp_results.references = None;
        } else {
            let count = references.len();
            self.overlays.lsp_results.references = Some(ReferenceState {
                items: references,
                selected: 0,
            });
            self.status.text = format!("references: {count}");
        }
    }

    pub(crate) fn apply_document_highlights_response(&mut self, result: &serde_json::Value) {
        let Some(items) = result.as_array() else {
            self.status.text = "lsp: no highlights".to_owned();
            self.overlays.lsp_results.document_highlights = None;
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
            self.status.text = "lsp: no highlights".to_owned();
            self.overlays.lsp_results.document_highlights = None;
        } else {
            let count = highlights.len();
            self.status.text = format!("highlights: {count}");
            self.overlays.lsp_results.document_highlights = Some(DocumentHighlightState {
                revision: self.session.editor.revision(),
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
            self.status.text = "lsp: no call hierarchy item".to_owned();
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
            self.status.text = "lsp: no calls".to_owned();
            self.overlays.lsp_results.call_hierarchy = None;
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
            self.status.text = "lsp: no calls".to_owned();
            self.overlays.lsp_results.call_hierarchy = None;
        } else {
            let count = items.len();
            let label = match direction {
                CallHierarchyDirection::Incoming => "incoming",
                CallHierarchyDirection::Outgoing => "outgoing",
            };
            self.status.text = format!("{label} calls: {count}");
            self.overlays.lsp_results.call_hierarchy = Some(CallHierarchyState {
                direction,
                items,
                selected: 0,
            });
        }
    }

    fn apply_hover_response(&mut self, result: &serde_json::Value) {
        let Some(contents) = result.get("contents") else {
            self.status.text = "lsp: no hover".to_owned();
            return;
        };
        let text = hover_text(contents);
        if text.is_empty() {
            self.status.text = "lsp: no hover".to_owned();
        } else {
            self.overlays.lsp_results.hover = Some(text);
            self.status.text = "hover".to_owned();
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

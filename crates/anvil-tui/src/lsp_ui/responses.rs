//! LSP response handling and DTO application.

use super::*;
use crate::{
    CallHierarchyDirection, CallHierarchyItem, CallHierarchyState, DocumentHighlightItem,
    DocumentHighlightState,
};
use std::collections::HashMap;
use std::path::PathBuf;
fn completion_edit_range(value: &serde_json::Value) -> Option<(usize, usize, usize, usize)> {
    let range = value
        .get("range")
        .or_else(|| value.get("replace"))
        .or_else(|| value.get("insert"))
        .unwrap_or(value);
    let start = range.get("start")?;
    let end = range.get("end")?;
    Some((
        start.get("line")?.as_u64()? as usize,
        start.get("character")?.as_u64()? as usize,
        end.get("line")?.as_u64()? as usize,
        end.get("character")?.as_u64()? as usize,
    ))
}

fn signature_parameter_ranges(signature: &serde_json::Value, label: &str) -> Vec<(usize, usize)> {
    signature
        .get("parameters")
        .and_then(serde_json::Value::as_array)
        .map(|parameters| {
            parameters
                .iter()
                .filter_map(|parameter| match parameter.get("label")? {
                    serde_json::Value::String(text) => {
                        let start = label.find(text)?;
                        Some((start, start + text.len()))
                    }
                    serde_json::Value::Array(range) => {
                        let start = range.first()?.as_u64()?;
                        let end = range.get(1)?.as_u64()?;
                        Some((
                            utf16_offset_to_byte(label, start),
                            utf16_offset_to_byte(label, end),
                        ))
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn utf16_offset_to_byte(text: &str, offset: u64) -> usize {
    let mut units = 0u64;
    for (index, character) in text.char_indices() {
        if units >= offset {
            return index;
        }
        units += character.len_utf16() as u64;
    }
    text.len()
}

impl Model {
    pub(crate) fn apply_document_symbols_response(&mut self, result: &serde_json::Value) {
        let silent = std::mem::take(&mut self.project.silent_symbol_refresh);
        let requested_path = self.project.document_symbol_path.take();
        if let Some(requested_path) = requested_path
            && self.session.path.as_ref() != Some(&requested_path)
        {
            return;
        }
        let Some(items) = result.as_array() else {
            if !silent {
                self.status.text = "lsp: no document symbols".to_owned();
                self.overlays.lsp_results.document_symbols = None;
                self.close_overlay(OverlayKind::DocumentSymbols);
            }
            return;
        };
        let items = symbols_from_lsp(items);
        if let Some(path) = self.session.path.clone() {
            self.index_document_symbols(&path, &items);
        }
        if silent {
            return;
        }
        if items.is_empty() {
            self.status.text = "lsp: no document symbols".to_owned();
            self.overlays.lsp_results.document_symbols = None;
            self.close_overlay(OverlayKind::DocumentSymbols);
        } else {
            self.overlays.lsp_results.document_symbols = Some(SymbolState { items, selected: 0 });
            self.push_overlay_if_idle(OverlayKind::DocumentSymbols);
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
        self.file.dirty = true;
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
                let parameters = signature_parameter_ranges(signature, &label);
                Some(SignatureItem {
                    label,
                    documentation,
                    parameters,
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
        let uri = anvil_lsp_client::file_uri(path);
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
            self.language.lsp_requests.set_format_on_save_pending(false);
        }
    }

    fn finish_format_on_save(&mut self) {
        if !self.language.lsp_requests.format_on_save_pending() {
            return;
        }
        self.language.lsp_requests.set_format_on_save_pending(false);
        if !self.file.modified {
            return;
        }
        self.language.lsp_requests.set_suppress_format_on_save(true);
        let saved = self.save();
        self.language
            .lsp_requests
            .set_suppress_format_on_save(false);
        if saved {
            self.status.text = "formatted and saved".to_owned();
            self.file.dirty = true;
        }
    }

    pub(crate) fn handle_lsp_response(
        &mut self,
        id: u64,
        result: serde_json::Value,
        error: Option<String>,
    ) {
        let Some(request) = self.language.lsp_requests.take_response(id) else {
            return;
        };
        if let Some(error) = error {
            if request.kind == LspRequestKind::CodeLens {
                self.language.code_lens = None;
                self.file.dirty = true;
                return;
            }
            if request.kind == LspRequestKind::CompletionResolve {
                self.language.completion.resolve = None;
            }
            if request.kind == LspRequestKind::InlineCompletion {
                self.language.completion.inline = None;
                self.language.completion.inline_requests.remove(&id);
            }
            if request.kind == LspRequestKind::StickySymbols {
                self.overlays.lsp_results.sticky_pending = false;
                self.file.dirty = true;
                return;
            }
            self.clear_format_on_save(request.kind);
            self.status.text = format!("E: lsp {:?}: {error}", request.kind);
            self.file.dirty = true;
            return;
        }
        if let Some(reason) = self.language.lsp_requests.stale_reason(
            &request,
            self.session.editor.revision(),
            self.session.path.as_deref(),
        ) {
            if request.kind == LspRequestKind::StickySymbols {
                self.overlays.lsp_results.sticky_pending = false;
            }
            self.clear_format_on_save(request.kind);
            self.status.text = match reason {
                StaleResponse::EditRevision => "lsp: stale edit response ignored".to_owned(),
                StaleResponse::InactiveBuffer => {
                    "lsp: edit response for inactive buffer".to_owned()
                }
            };
            self.file.dirty = true;
            return;
        }
        let PendingRequest {
            kind,
            edit_revision,
            edit_path,
            edit_snapshot,
        } = request;
        match kind {
            LspRequestKind::Completion => self.apply_completion_response(&result),
            LspRequestKind::CompletionResolve => {
                self.apply_completion_resolve_response(id, &result);
            }
            LspRequestKind::InlineCompletion => {
                let request = self.language.completion.inline_requests.remove(&id);
                self.apply_inline_completion_response(&result, request);
            }
            LspRequestKind::Definition => self.apply_goto_response(&result, "definition"),
            LspRequestKind::Declaration => self.apply_goto_response(&result, "declaration"),
            LspRequestKind::TypeDefinition => {
                self.apply_goto_response(&result, "type definition");
            }
            LspRequestKind::Implementation => {
                self.apply_goto_response(&result, "implementation");
            }
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
            LspRequestKind::CompletionCommand => {
                self.status.text = "completion command executed".to_owned();
            }
            LspRequestKind::CodeActionCommand => {
                self.status.text = "code action command executed".to_owned();
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
        self.file.dirty = true;
    }

    fn apply_completion_response(&mut self, result: &serde_json::Value) {
        let items = result
            .get("items")
            .and_then(serde_json::Value::as_array)
            .or_else(|| result.as_array());
        let Some(items) = items else {
            self.status.text = "E: lsp: invalid completion response".to_owned();
            return;
        };
        let defaults = result.get("itemDefaults");
        let default_edit_range = defaults
            .and_then(|defaults| defaults.get("editRange"))
            .and_then(completion_edit_range);
        let default_insert_format = defaults
            .and_then(|defaults| defaults.get("insertTextFormat"))
            .and_then(serde_json::Value::as_u64);
        let default_insert_text_mode = defaults
            .and_then(|defaults| defaults.get("insertTextMode"))
            .and_then(serde_json::Value::as_u64);
        let default_commit_characters = defaults
            .and_then(|defaults| defaults.get("commitCharacters"))
            .cloned();
        let all_commit_characters = self
            .language
            .lsp
            .config
            .as_ref()
            .and_then(|key| self.language.completion.all_commit_characters.get(key))
            .cloned()
            .map(serde_json::Value::from);
        let is_incomplete = result
            .get("isIncomplete")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let items: Vec<CompletionItem> = items
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
                let text_edit = item.get("textEdit");
                let insert = text_edit
                    .and_then(|edit| edit.get("newText"))
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| item.get("insertText").and_then(serde_json::Value::as_str))
                    .or_else(|| item.get("textEditText").and_then(serde_json::Value::as_str))
                    .unwrap_or(&label)
                    .to_owned();
                let insert_format = item
                    .get("insertTextFormat")
                    .and_then(serde_json::Value::as_u64)
                    .or(default_insert_format);
                let is_snippet = insert_format == Some(2);
                let snippet = is_snippet
                    .then(|| crate::snippet::parse(&insert).ok())
                    .flatten();
                let insert = if is_snippet {
                    snippet
                        .as_ref()
                        .map(|snippet| snippet.text.clone())
                        .unwrap_or(insert)
                } else {
                    expand_snippet(&insert)
                };
                let detail = item
                    .get("detail")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let documentation = item
                    .get("documentation")
                    .map(hover_text)
                    .filter(|text| !text.is_empty());
                let replace = text_edit
                    .and_then(completion_edit_range)
                    .or(default_edit_range);
                let mut raw = item.clone();
                if raw.get("commitCharacters").is_none()
                    && let Some(characters) = default_commit_characters
                        .as_ref()
                        .or(all_commit_characters.as_ref())
                {
                    raw["commitCharacters"] = characters.clone();
                }
                if raw.get("insertTextMode").is_none()
                    && let Some(mode) = default_insert_text_mode
                {
                    raw["insertTextMode"] = serde_json::Value::from(mode);
                }
                Some(CompletionItem {
                    source: anvil_session::provider::ProviderId::new("builtin.lsp"),
                    label,
                    filter_text,
                    sort_text,
                    preselect,
                    insert,
                    snippet,
                    detail,
                    replace,
                    replace_bytes: None,
                    documentation,
                    raw,
                    resolved: false,
                })
            })
            .collect();
        let previous = self.language.completion.completion.take();
        let previous_selected = previous.as_ref().and_then(|completion| {
            completion
                .filtered
                .get(completion.selected)
                .and_then(|index| completion.items.get(*index))
                .map(|item| item.label.clone())
        });
        let mut merged = previous
            .map(|completion| completion.items)
            .unwrap_or_default()
            .into_iter()
            .filter(|item| item.source.as_str() != "builtin.lsp")
            .collect::<Vec<_>>();
        for item in items {
            if merged
                .iter()
                .any(|existing| existing.label == item.label && existing.insert == item.insert)
            {
                continue;
            }
            merged.push(item);
        }
        let mut items = merged;
        items.sort_by(|left, right| {
            left.sort_text
                .cmp(&right.sort_text)
                .then_with(|| left.label.cmp(&right.label))
        });
        items.truncate(200);
        self.language.completion.incomplete = is_incomplete;
        if items.is_empty() {
            self.status.text = "lsp: no completions".to_owned();
            self.language.completion.completion = None;
            self.close_overlay(OverlayKind::Completion);
        } else {
            let text = self.session.editor.text();
            let query = crate::completion_sources::completion_prefix(
                &text.slice(0..text.len_bytes()),
                self.session.cursor,
            );
            let filtered = (0..items.len()).collect();
            let recent = self.language.completion.recent.clone();
            let selected = previous_selected
                .and_then(|label| items.iter().position(|item| item.label == label))
                .or_else(|| items.iter().position(|item| item.preselect))
                .or_else(|| {
                    recent.and_then(|label| items.iter().position(|item| item.label == label))
                })
                .unwrap_or(0);
            self.language.completion.documentation_scroll = 0;
            self.language.completion.completion = Some(CompletionState {
                items,
                filtered,
                query,
                selected,
            });
            self.push_overlay_if_idle(OverlayKind::Completion);
            self.status.text = "completion".to_owned();
            self.filter_completion();
        }
    }

    fn apply_completion_resolve_response(&mut self, id: u64, result: &serde_json::Value) {
        let Some((pending, index)) = self.language.completion.resolve else {
            return;
        };
        if pending != id {
            return;
        }
        self.language.completion.resolve = None;
        let Some(completion) = self.language.completion.completion.as_mut() else {
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
        let mut raw = result.clone();
        if raw.get("additionalTextEdits").is_none()
            && let Some(edits) = item.raw.get("additionalTextEdits").cloned()
        {
            raw["additionalTextEdits"] = edits;
        }
        if raw.get("command").is_none()
            && let Some(command) = item.raw.get("command").cloned()
        {
            raw["command"] = command;
        }
        item.raw = raw;
        item.resolved = true;
        self.file.dirty = true;
    }

    fn apply_inline_completion_response(
        &mut self,
        result: &serde_json::Value,
        request: Option<(u64, usize, Option<PathBuf>)>,
    ) {
        let Some((revision, cursor, path)) = request else {
            self.language.completion.inline = None;
            return;
        };
        if revision != self.session.editor.revision()
            || cursor != self.session.cursor
            || path != self.session.path
        {
            self.language.completion.inline = None;
            return;
        }
        if !matches!(self.ui.input_mode, InputMode::Insert)
            || self.language.completion.completion.is_some()
            || self.language.completion.snippet.is_some()
        {
            self.language.completion.inline = None;
            return;
        }
        let item = result
            .get("items")
            .and_then(serde_json::Value::as_array)
            .and_then(|items| items.first());
        let Some(item) = item else {
            self.language.completion.inline = None;
            return;
        };
        let text = item
            .get("insertText")
            .and_then(|value| {
                value
                    .as_str()
                    .or_else(|| value.get("value").and_then(serde_json::Value::as_str))
            })
            .unwrap_or_default();
        if text.is_empty() {
            self.language.completion.inline = None;
            return;
        }
        let replace = item.get("range").and_then(|range| {
            let start = range.get("start")?;
            let end = range.get("end")?;
            Some((
                start.get("line")?.as_u64()? as usize,
                start.get("character")?.as_u64()? as usize,
                end.get("line")?.as_u64()? as usize,
                end.get("character")?.as_u64()? as usize,
            ))
        });
        self.language.completion.inline = Some(crate::state::InlineCompletionState {
            text: text.to_owned(),
            revision: self.session.editor.revision(),
            cursor: self.session.cursor,
            path: self.session.path.clone(),
            replace,
        });
        self.file.dirty = true;
    }

    #[cfg(test)]
    pub(crate) fn apply_definition_response(&mut self, result: &serde_json::Value) {
        self.apply_goto_response(result, "definition");
    }

    pub(crate) fn apply_goto_response(&mut self, result: &serde_json::Value, label: &str) {
        let location = result
            .as_array()
            .and_then(|locations| locations.first())
            .unwrap_or(result);
        let Some(uri) = location
            .get("uri")
            .or_else(|| location.get("targetUri"))
            .and_then(serde_json::Value::as_str)
        else {
            self.status.text = format!("lsp: no {label}");
            return;
        };
        let Some(path) = lsp::uri_to_path(uri) else {
            self.status.text = format!("E: lsp: invalid {label} URI");
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
        if self.session.path.as_ref() != Some(&path)
            && let Err(error) = self.open_path(&path)
        {
            self.status.text = format!("E: {error}");
            return;
        }
        self.session.cursor = self.offset_at_utf16(line, character);
        self.status.text = format!("{label} {}", path.display());
    }

    pub(crate) fn apply_references_response(&mut self, result: &serde_json::Value) {
        let Some(items) = result.as_array() else {
            self.status.text = "lsp: no references".to_owned();
            self.overlays.lsp_results.references = None;
            self.close_overlay(OverlayKind::References);
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
            self.close_overlay(OverlayKind::References);
        } else {
            let count = references.len();
            self.overlays.lsp_results.references = Some(ReferenceState {
                items: references,
                selected: 0,
            });
            self.push_overlay_if_idle(OverlayKind::References);
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
            self.close_overlay(OverlayKind::CallHierarchy);
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
            self.close_overlay(OverlayKind::CallHierarchy);
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
            self.push_overlay_if_idle(OverlayKind::CallHierarchy);
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
            self.push_overlay_if_idle(OverlayKind::Hover);
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

impl Model {
    pub(crate) fn apply_workspace_symbols_response(&mut self, result: &serde_json::Value) {
        let Some(items) = result.as_array() else {
            if !self.open_cached_workspace_symbols() {
                self.status.text = "lsp: no workspace symbols".to_owned();
                self.overlays.lsp_results.workspace_symbols = None;
                self.close_overlay(OverlayKind::WorkspaceSymbols);
            }
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
        self.index_workspace_symbols(&parsed);
        if parsed.is_empty() {
            if !self.open_cached_workspace_symbols() {
                self.status.text = "lsp: no workspace symbols".to_owned();
                self.overlays.lsp_results.workspace_symbols = None;
                self.close_overlay(OverlayKind::WorkspaceSymbols);
            }
        } else {
            self.overlays.lsp_results.workspace_symbols = Some(WorkspaceSymbolState {
                filtered: (0..parsed.len()).collect(),
                items: parsed,
                query: String::new(),
                selected: 0,
            });
            self.push_overlay_if_idle(OverlayKind::WorkspaceSymbols);
            self.status.text = "workspace symbols".to_owned();
        }
    }
    fn apply_code_action_response(
        &mut self,
        result: &serde_json::Value,
        base: Option<HashMap<PathBuf, u64>>,
    ) {
        if result.is_null() {
            self.status.text = "lsp: no code actions".to_owned();
            self.overlays.lsp_results.code_actions = None;
            self.close_overlay(OverlayKind::CodeActions);
            return;
        }
        let Some(items) = result.as_array() else {
            self.status.text = "E: lsp: invalid code action response".to_owned();
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
                let command = item
                    .get("command")
                    .and_then(|command| command.get("command"))
                    .and_then(serde_json::Value::as_str)
                    .filter(|command| !command.is_empty())
                    .map(|command| CodeActionCommand {
                        command: command.to_owned(),
                        arguments: item
                            .get("command")
                            .and_then(|command| command.get("arguments"))
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!([])),
                    });
                Some(CodeActionItem {
                    title,
                    edit,
                    command,
                    disabled,
                })
            })
            .collect();
        if items.is_empty() {
            self.status.text = "lsp: no code actions".to_owned();
            self.overlays.lsp_results.code_actions = None;
            self.close_overlay(OverlayKind::CodeActions);
        } else {
            self.overlays.lsp_results.code_actions = Some(CodeActionState { items, selected: 0 });
            self.push_overlay_if_idle(OverlayKind::CodeActions);
            self.status.text = "code actions".to_owned();
        }
    }
}

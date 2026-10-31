//! LSP request state, completion, definition, and hover UI behavior.

use super::{CompletionItem, CompletionState, LspRequestKind, Model};
use crate::{lsp, lsp_worker};
use anvil_core::TextBuffer;
use std::path::Path;

pub(crate) struct LspSettings {
    pub(crate) tool: String,
    pub(crate) args: Vec<String>,
    pub(crate) language_id: String,
    pub(crate) root_markers: Vec<&'static str>,
    pub(crate) label: &'static str,
}

pub(crate) fn lsp_settings(path: &Path) -> Option<LspSettings> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let (candidates, args, language_id, root_markers, label, env) = match extension.as_str() {
        "rs" => (
            vec!["rust-analyzer"],
            Vec::new(),
            "rust",
            vec!["Cargo.toml"],
            "rust-analyzer",
            "ANVIL_LSP_RUST",
        ),
        "go" => (
            vec!["gopls"],
            Vec::new(),
            "go",
            vec!["go.work", "go.mod"],
            "gopls",
            "ANVIL_LSP_GO",
        ),
        "py" | "pyi" => (
            vec!["pyright-langserver", "basedpyright-langserver", "pylsp"],
            Vec::new(),
            "python",
            vec![
                "pyproject.toml",
                "setup.py",
                "setup.cfg",
                "requirements.txt",
            ],
            "python language server",
            "ANVIL_LSP_PYTHON",
        ),
        _ => return None,
    };
    let (tool, args) = std::env::var_os(env)
        .or_else(|| std::env::var_os("ANVIL_LSP"))
        .and_then(|path| {
            let path = path.to_string_lossy().into_owned();
            (!path.is_empty()).then_some(path)
        })
        .or_else(|| find_tool(&candidates))
        .map(|tool| {
            let args = if tool.ends_with("pyright-langserver") {
                vec!["--stdio".to_owned()]
            } else {
                args.clone()
            };
            (tool, args)
        })?;
    Some(LspSettings {
        tool,
        args,
        language_id: language_id.to_owned(),
        root_markers,
        label,
    })
}

fn find_tool(candidates: &[&str]) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        for candidate in candidates {
            let path = directory.join(candidate);
            if path.is_file() {
                return Some(path.to_string_lossy().into_owned());
            }
        }
    }
    candidates.first().map(|candidate| (*candidate).to_owned())
}

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

    fn send_lsp_request(
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
        let Some(worker) = self.lsp_worker.as_ref() else {
            self.status = "E: lsp is not running".to_owned();
            self.dirty = true;
            return false;
        };
        let id = self.next_lsp_request;
        self.next_lsp_request += 1;
        let text = self.editor.text().slice(0..self.editor.text().len_bytes());
        if let Err(error) = worker
            .sync(&path, text)
            .and_then(|()| worker.request(id, method, params))
        {
            self.status = format!("E: lsp request: {error}");
            self.dirty = true;
            return false;
        }
        self.pending_lsp.insert(id, kind);
        true
    }

    pub(crate) fn jump_diagnostic(&mut self, forward: bool) {
        self.cancel_completion();
        self.hover = None;
        let Some(path) = self.path.clone() else {
            self.status = "E: no file".to_owned();
            self.dirty = true;
            return;
        };
        let Some(diagnostics) = self.diagnostics.get(&path) else {
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
        let Some(diagnostic) = primary.iter().find(|diagnostic| {
            diagnostic.range.start.line as usize == line
                && diagnostic.range.start.character as usize == character
        }) else {
            return;
        };
        let label = match diagnostic.severity {
            Some(1) => "E",
            Some(2) => "W",
            Some(3) => "I",
            _ => "D",
        };
        self.diagnostic_popup = Some(format!("{label}: {}", diagnostic.message));
        self.cursor = self.offset_at_utf16(line, character);
        let index = positions
            .iter()
            .position(|item| *item == (line, character))
            .unwrap_or(0);
        self.status = format!("diagnostic {}/{}", index + 1, positions.len());
        self.dirty = true;
    }

    pub(crate) fn notify_lsp_save(&mut self, path: &std::path::Path, text: &str) {
        if let Some(worker) = self.lsp_worker.as_ref() {
            let _ = worker.save(path, text.to_owned());
        }
    }

    pub(crate) fn cancel_completion(&mut self) {
        self.pending_lsp
            .retain(|_, kind| *kind != LspRequestKind::Completion);
        self.completion = None;
        self.dirty = true;
    }

    pub(crate) fn auto_completion(&mut self) {
        if self.lsp_worker.is_some() {
            self.request_completion();
        }
    }

    pub(crate) fn request_completion(&mut self) {
        self.cancel_completion();
        self.hover = None;
        self.diagnostic_popup = None;
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

    pub(crate) fn request_definition(&mut self) {
        self.cancel_completion();
        self.diagnostic_popup = None;
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

    pub(crate) fn request_hover(&mut self) {
        self.cancel_completion();
        self.diagnostic_popup = None;
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

    pub(crate) fn handle_lsp_response(
        &mut self,
        id: u64,
        result: serde_json::Value,
        error: Option<String>,
    ) {
        let Some(kind) = self.pending_lsp.remove(&id) else {
            return;
        };
        if let Some(error) = error {
            self.status = format!("E: lsp: {error}");
            self.dirty = true;
            return;
        }
        match kind {
            LspRequestKind::Completion => self.apply_completion_response(id, &result),
            LspRequestKind::Definition => self.apply_definition_response(&result),
            LspRequestKind::Hover => self.apply_hover_response(&result),
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
        }
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
        let insert = item.insert.clone();
        if let Some((start_line, start_char, end_line, end_char)) = item.replace {
            let start = self.offset_at_utf16(start_line, start_char);
            let end = self.offset_at_utf16(end_line, end_char).max(start);
            self.editor.set_selection_bytes(start, end);
            self.editor.delete(start..end);
            self.editor.insert(start, &insert);
            self.cursor = start + insert.len();
            self.modified = true;
        } else {
            self.insert_text(&insert);
        }
        self.status = format!("completed {}", item.label);
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
    }

    pub(crate) fn filter_completion(&mut self) {
        if let Some(completion) = self.completion.as_mut() {
            let query = completion.query.to_lowercase();
            completion.filtered = completion
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.label.to_lowercase().contains(&query))
                .map(|(index, _)| index)
                .collect();
            completion.selected = completion
                .selected
                .min(completion.filtered.len().saturating_sub(1));
            self.dirty = true;
        }
    }
}

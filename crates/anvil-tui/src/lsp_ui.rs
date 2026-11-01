//! LSP request state, completion, definition, and hover UI behavior.

use super::{
    CodeActionItem, CodeActionState, CompletionItem, CompletionState, InputMode, LspRequestKind,
    Model, SignatureHelpState, SignatureItem, SymbolItem, SymbolState, WorkspaceEdit,
    WorkspaceSymbolItem, WorkspaceSymbolState,
};
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
        "ts" | "mts" | "cts" => (
            vec!["typescript-language-server", "vtsls"],
            vec!["--stdio".to_owned()],
            "typescript",
            vec!["tsconfig.json", "jsconfig.json", "package.json"],
            "typescript language server",
            "ANVIL_LSP_TYPESCRIPT",
        ),
        "tsx" => (
            vec!["typescript-language-server", "vtsls"],
            vec!["--stdio".to_owned()],
            "typescriptreact",
            vec!["tsconfig.json", "jsconfig.json", "package.json"],
            "typescript language server",
            "ANVIL_LSP_TYPESCRIPT",
        ),
        "js" | "mjs" | "cjs" => (
            vec!["typescript-language-server", "vtsls"],
            vec!["--stdio".to_owned()],
            "javascript",
            vec!["tsconfig.json", "jsconfig.json", "package.json"],
            "typescript language server",
            "ANVIL_LSP_TYPESCRIPT",
        ),
        "jsx" => (
            vec!["typescript-language-server", "vtsls"],
            vec!["--stdio".to_owned()],
            "javascriptreact",
            vec!["tsconfig.json", "jsconfig.json", "package.json"],
            "typescript language server",
            "ANVIL_LSP_TYPESCRIPT",
        ),
        "c" | "h" => (
            vec!["clangd"],
            Vec::new(),
            "c",
            vec![
                "compile_commands.json",
                "compile_flags.txt",
                ".clangd",
                "CMakeLists.txt",
                "Makefile",
            ],
            "clangd",
            "ANVIL_LSP_CLANGD",
        ),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => (
            vec!["clangd"],
            Vec::new(),
            "cpp",
            vec![
                "compile_commands.json",
                "compile_flags.txt",
                ".clangd",
                "CMakeLists.txt",
                "Makefile",
            ],
            "clangd",
            "ANVIL_LSP_CLANGD",
        ),
        "java" => (
            vec!["jdtls"],
            Vec::new(),
            "java",
            vec![
                "pom.xml",
                "build.gradle",
                "build.gradle.kts",
                "settings.gradle",
                "settings.gradle.kts",
                "gradlew",
            ],
            "jdtls",
            "ANVIL_LSP_JAVA",
        ),
        "lua" => (
            vec!["lua-language-server"],
            Vec::new(),
            "lua",
            vec![".luarc.json", ".luarc.jsonc", ".stylua.toml", "selene.toml"],
            "lua-language-server",
            "ANVIL_LSP_LUA",
        ),
        "json" => (
            vec!["vscode-json-language-server", "json-languageserver"],
            vec!["--stdio".to_owned()],
            "json",
            vec!["package.json", "tsconfig.json", "jsconfig.json"],
            "json language server",
            "ANVIL_LSP_JSON",
        ),
        "jsonc" => (
            vec!["vscode-json-language-server", "json-languageserver"],
            vec!["--stdio".to_owned()],
            "jsonc",
            vec!["package.json", "tsconfig.json", "jsconfig.json"],
            "json language server",
            "ANVIL_LSP_JSON",
        ),
        "yaml" | "yml" => (
            vec!["yaml-language-server"],
            vec!["--stdio".to_owned()],
            "yaml",
            vec![".yamllint", ".yamllint.yaml", ".yamllint.yml", "Chart.yaml"],
            "yaml-language-server",
            "ANVIL_LSP_YAML",
        ),
        "sh" | "bash" | "zsh" => (
            vec!["bash-language-server"],
            vec!["start".to_owned()],
            "shellscript",
            vec![".shellcheckrc", ".bashrc", ".zshrc"],
            "bash-language-server",
            "ANVIL_LSP_SHELL",
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
        let id = self.next_lsp_request;
        self.next_lsp_request += 1;
        let Some(worker) = self.active_lsp_worker() else {
            self.status = "E: lsp is not running".to_owned();
            self.dirty = true;
            return false;
        };
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
        if matches!(
            kind,
            LspRequestKind::Rename
                | LspRequestKind::CodeAction
                | LspRequestKind::Formatting
                | LspRequestKind::SignatureHelp
        ) {
            self.pending_edit_revision
                .insert(id, self.editor.revision());
            self.pending_edit_path.insert(id, path);
        }
        true
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
        if let Some(key) = Self::lsp_key(path)
            && let Some(worker) = self.lsp_workers.get(&key)
        {
            let _ = worker.save(path, text.to_owned());
        }
    }

    pub(crate) fn clear_lsp_requests(&mut self) {
        self.pending_lsp.clear();
        self.pending_edit_revision.clear();
        self.pending_edit_path.clear();
        self.signature_help = None;
        self.workspace_symbols = None;
        self.inlay_hints.clear();
        self.inlay_pending = None;
        self.inlay_checked = None;
        self.dirty = true;
    }

    pub(crate) fn cancel_signature_help(&mut self) {
        self.pending_lsp
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

    fn prune_edit_requests(&mut self) {
        let pending: Vec<u64> = self.pending_lsp.keys().copied().collect();
        self.pending_edit_revision
            .retain(|id, _| pending.contains(id));
        self.pending_edit_path.retain(|id, _| pending.contains(id));
    }

    pub(crate) fn cancel_completion(&mut self) {
        self.pending_lsp
            .retain(|_, kind| *kind != LspRequestKind::Completion);
        self.completion = None;
        self.dirty = true;
    }

    pub(crate) fn cancel_code_actions(&mut self) {
        self.pending_lsp.retain(|_, kind| {
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
        self.diagnostic_popup = None;
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

    pub(crate) fn request_definition(&mut self) {
        self.cancel_completion();
        self.diagnostic_popup = None;
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

    pub(crate) fn request_hover(&mut self) {
        self.cancel_completion();
        self.diagnostic_popup = None;
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
        self.diagnostic_popup = None;
        self.input_mode = InputMode::Rename {
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
        self.diagnostic_popup = None;
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

    pub(crate) fn cancel_document_symbols(&mut self) {
        self.pending_lsp
            .retain(|_, kind| *kind != LspRequestKind::DocumentSymbol);
        self.document_symbols = None;
        self.dirty = true;
    }

    pub(crate) fn cancel_workspace_symbols(&mut self) {
        self.pending_lsp
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
        self.diagnostic_popup = None;
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
        self.diagnostic_popup = None;
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
        self.cursor = self.offset_at_utf16(symbol.line, symbol.character);
        self.status = format!("symbol {}", symbol.name);
        self.dirty = true;
    }

    pub(crate) fn request_formatting(&mut self) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_symbols();
        self.pending_lsp
            .retain(|_, kind| *kind != LspRequestKind::Formatting);
        self.prune_edit_requests();
        self.hover = None;
        self.diagnostic_popup = None;
        let Some(path) = self.path.clone() else {
            return;
        };
        let uri = lsp_worker::file_uri(&path);
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 4, "insertSpaces": true}
        });
        if self.send_lsp_request(
            LspRequestKind::Formatting,
            "textDocument/formatting",
            params,
        ) {
            self.status = "formatting".to_owned();
        }
        self.dirty = true;
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

    fn apply_code_action_response(&mut self, result: &serde_json::Value) {
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
                    Some(edit) => match WorkspaceEdit::from_lsp(edit) {
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
            self.status = format!("E: lsp: {error}");
            self.dirty = true;
            return;
        }
        if matches!(
            kind,
            LspRequestKind::Rename
                | LspRequestKind::CodeAction
                | LspRequestKind::Formatting
                | LspRequestKind::SignatureHelp
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
            LspRequestKind::Definition => self.apply_definition_response(&result),
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

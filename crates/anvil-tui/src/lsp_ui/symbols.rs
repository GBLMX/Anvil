use super::*;

use anvil_runtime::{
    SymbolKind, SymbolOccurrence, SymbolRole, TextPosition, TextRange, WorkspaceSymbol,
};
use std::path::Path;

impl Model {
    pub(crate) fn request_workspace_symbols(&mut self) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_symbols();
        self.cancel_workspace_symbols();
        self.close_hover();
        self.close_diagnostic_peek();
        if self.project.platform.is_none() {
            self.load_project_info();
        }
        let cached = self.open_cached_workspace_symbols();
        let Some(path) = self.session.path.as_ref() else {
            if !cached {
                self.status.text = "E: no workspace symbols".to_owned();
                self.file.dirty = true;
            }
            return;
        };
        let params = serde_json::json!({"query": ""});
        let _ = path;
        if self.send_lsp_request(LspRequestKind::WorkspaceSymbol, "workspace/symbol", params) {
            self.status.text = "workspace symbols".to_owned();
        }
        self.file.dirty = true;
    }

    fn cached_workspace_symbols(&self) -> Vec<WorkspaceSymbolItem> {
        let Some(runtime) = self.project.platform.as_ref() else {
            return Vec::new();
        };
        let root = runtime.workspace_root();
        runtime
            .symbols()
            .symbols("", usize::MAX)
            .into_iter()
            .map(|symbol| WorkspaceSymbolItem {
                detail: symbol.kind.as_str().to_owned(),
                path: root.join(&symbol.path),
                line: symbol.range.start.line as usize,
                character: symbol.range.start.character as usize,
                name: symbol.name,
            })
            .collect()
    }

    pub(crate) fn open_cached_workspace_symbols(&mut self) -> bool {
        let items = self.cached_workspace_symbols();
        if items.is_empty() {
            return false;
        }
        let count = items.len();
        self.overlays.lsp_results.workspace_symbols = Some(WorkspaceSymbolState {
            filtered: (0..count).collect(),
            items,
            query: String::new(),
            selected: 0,
        });
        self.push_overlay_if_idle(OverlayKind::WorkspaceSymbols);
        self.status.text = format!("workspace symbols: {count} cached");
        self.file.dirty = true;
        true
    }

    fn index_symbol(
        &mut self,
        name: &str,
        detail: &str,
        path: &Path,
        line: usize,
        character: usize,
    ) {
        let Some(runtime) = self.project.platform.as_mut() else {
            return;
        };
        let root = runtime.workspace_root().to_path_buf();
        let Ok(relative) = path.strip_prefix(&root) else {
            return;
        };
        let path = relative.to_string_lossy().replace('\\', "/");
        if path.is_empty() || name.is_empty() {
            return;
        }
        let caps = runtime.symbols().caps();
        if name.len() > caps.max_name_bytes || path.len() > caps.max_path_bytes {
            return;
        }
        let start = TextPosition {
            line: line.min(u32::MAX as usize) as u32,
            character: character.min(u32::MAX as usize) as u32,
        };
        let range = TextRange {
            start,
            end: TextPosition {
                line: start.line,
                character: start.character.saturating_add(1),
            },
        };
        let kind = SymbolKind::Other(detail.chars().take(64).collect());
        let _ = runtime.symbols_mut().push_symbol(WorkspaceSymbol {
            name: name.to_owned(),
            kind: kind.clone(),
            path: path.clone(),
            range,
        });
        let _ = runtime.symbols_mut().push_occurrence(SymbolOccurrence {
            symbol: name.to_owned(),
            role: SymbolRole::Definition,
            path,
            range,
        });
    }

    pub(crate) fn index_workspace_symbols(&mut self, items: &[WorkspaceSymbolItem]) {
        for item in items {
            self.index_symbol(
                &item.name,
                &item.detail,
                &item.path,
                item.line,
                item.character,
            );
        }
        self.persist_symbol_cache();
    }

    pub(crate) fn index_document_symbols(&mut self, path: &Path, items: &[SymbolItem]) {
        for item in items {
            self.index_symbol(
                &item.name,
                item.detail.as_deref().unwrap_or_default(),
                path,
                item.line,
                item.character,
            );
        }
        self.persist_symbol_cache();
    }
    pub(crate) fn forget_symbols_for_path(&mut self, path: &Path) {
        if let Some(runtime) = self.project.platform.as_mut() {
            runtime.remove_symbols_under_path(path);
        }
        self.persist_symbol_cache();
    }

    pub(crate) fn persist_symbol_cache(&self) {
        if let Some(runtime) = self.project.platform.as_ref() {
            let cache = self
                .symbol_cache
                .clone()
                .unwrap_or_else(|| runtime.symbol_cache());
            let _ = cache.save(runtime.symbols());
        }
    }
    pub(crate) fn move_workspace_symbol(&mut self, delta: isize) {
        if let Some(symbols) = self.overlays.lsp_results.workspace_symbols.as_mut() {
            symbols.selected = symbols
                .selected
                .saturating_add_signed(delta)
                .min(symbols.filtered.len().saturating_sub(1));
            self.file.dirty = true;
        }
    }
    pub(crate) fn filter_workspace_symbols(&mut self) {
        if let Some(symbols) = self.overlays.lsp_results.workspace_symbols.as_mut() {
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
            self.file.dirty = true;
        }
    }
    pub(crate) fn apply_workspace_symbol(&mut self) {
        let Some(symbols) = self.overlays.lsp_results.workspace_symbols.take() else {
            return;
        };
        self.close_overlay(OverlayKind::WorkspaceSymbols);
        let Some(symbol) = symbols
            .filtered
            .get(symbols.selected)
            .and_then(|index| symbols.items.get(*index))
            .cloned()
        else {
            return;
        };
        self.record_jump();
        if self.session.path.as_ref() != Some(&symbol.path)
            && let Err(error) = self.open_path(&symbol.path)
        {
            self.status.text = format!("E: {error}");
            self.file.dirty = true;
            return;
        }
        self.session.cursor = self.offset_at_utf16(symbol.line, symbol.character);
        self.status.text = format!("symbol {}", symbol.name);
        self.file.dirty = true;
    }
    pub(crate) fn request_document_symbols(&mut self) {
        self.request_document_symbols_mode(false);
    }

    pub(crate) fn request_document_symbols_silent(&mut self) {
        self.request_document_symbols_mode(true);
    }

    pub(crate) fn request_document_symbols_silent_if_runtime(&mut self) {
        if self.project.platform.is_some() || self.ensure_project_platform() {
            self.request_document_symbols_silent();
        }
    }

    fn request_document_symbols_mode(&mut self, silent: bool) {
        self.project.silent_symbol_refresh = silent;
        self.project.document_symbol_path = self.session.path.clone();
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_symbols();
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
        let params = serde_json::json!({"textDocument": {"uri": uri}});
        if self.send_lsp_request(
            LspRequestKind::DocumentSymbol,
            "textDocument/documentSymbol",
            params,
        ) && !silent
        {
            self.status.text = "document symbols".to_owned();
        }
        self.file.dirty = true;
    }
    pub(crate) fn move_document_symbol(&mut self, delta: isize) {
        if let Some(symbols) = self.overlays.lsp_results.document_symbols.as_mut() {
            symbols.selected = symbols
                .selected
                .saturating_add_signed(delta)
                .min(symbols.items.len().saturating_sub(1));
            self.file.dirty = true;
        }
    }
    pub(crate) fn apply_document_symbol(&mut self) {
        let Some(symbols) = self.overlays.lsp_results.document_symbols.take() else {
            return;
        };
        self.close_overlay(OverlayKind::DocumentSymbols);
        let Some(symbol) = symbols.items.get(symbols.selected).cloned() else {
            return;
        };
        self.record_jump();
        self.session.cursor = self.offset_at_utf16(symbol.line, symbol.character);
        self.status.text = format!("symbol {}", symbol.name);
        self.file.dirty = true;
    }
    pub(crate) fn move_reference(&mut self, delta: isize) {
        if let Some(references) = self.overlays.lsp_results.references.as_mut() {
            references.selected = references
                .selected
                .saturating_add_signed(delta)
                .min(references.items.len().saturating_sub(1));
            self.file.dirty = true;
        }
    }
    pub(crate) fn apply_reference(&mut self) {
        let Some(references) = self.overlays.lsp_results.references.take() else {
            return;
        };
        self.close_overlay(OverlayKind::References);
        let Some(reference) = references.items.get(references.selected).cloned() else {
            return;
        };
        self.record_jump();
        if self.session.path.as_ref() != Some(&reference.path)
            && let Err(error) = self.open_path(&reference.path)
        {
            self.status.text = format!("E: {error}");
            self.file.dirty = true;
            return;
        }
        self.session.cursor = self.offset_at_utf16(reference.line, reference.character);
        self.status.text = format!(
            "reference {}:{}",
            reference.path.display(),
            reference.line + 1
        );
        self.file.dirty = true;
    }
    pub(crate) fn move_call_hierarchy(&mut self, delta: isize) {
        if let Some(calls) = self.overlays.lsp_results.call_hierarchy.as_mut() {
            calls.selected = calls
                .selected
                .saturating_add_signed(delta)
                .min(calls.items.len().saturating_sub(1));
            self.file.dirty = true;
        }
    }
    pub(crate) fn apply_call_hierarchy(&mut self) {
        let Some(calls) = self.overlays.lsp_results.call_hierarchy.take() else {
            return;
        };
        self.close_overlay(OverlayKind::CallHierarchy);
        let Some(item) = calls.items.get(calls.selected).cloned() else {
            return;
        };
        self.record_jump();
        if self.session.path.as_ref() != Some(&item.path)
            && let Err(error) = self.open_path(&item.path)
        {
            self.status.text = format!("E: {error}");
            self.file.dirty = true;
            return;
        }
        self.session.cursor = self.offset_at_utf16(item.line, item.character);
        let direction = match calls.direction {
            CallHierarchyDirection::Incoming => "incoming",
            CallHierarchyDirection::Outgoing => "outgoing",
        };
        self.status.text = format!("{direction} call {}:{}", item.name, item.line + 1);
        self.file.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EditorState;

    #[test]
    fn lsp_symbols_refresh_the_runtime_index() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.load_project_info();
        let path = model.workspace.path.join("src/main.rs");
        let response = serde_json::json!([{
            "name": "refreshed_symbol",
            "containerName": "function",
            "location": {
                "uri": format!("file://{}", path.display()),
                "range": {
                    "start": {"line": 2, "character": 4},
                    "end": {"line": 2, "character": 8}
                }
            }
        }]);
        model.apply_workspace_symbols_response(&response);
        let runtime = model.project.platform.as_ref().expect("runtime");
        assert!(
            runtime
                .symbols()
                .symbols("refreshed_symbol", 10)
                .iter()
                .any(|symbol| symbol.name == "refreshed_symbol")
        );
        let panel = model
            .overlays
            .lsp_results
            .workspace_symbols
            .as_ref()
            .expect("workspace symbol panel");
        assert!(
            panel
                .items
                .iter()
                .any(|item| item.name == "refreshed_symbol")
        );
    }

    #[test]
    fn silent_document_symbol_response_updates_index_without_panel() {
        let directory = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(directory.path().join("src")).expect("src");
        std::fs::write(
            directory.path().join("Cargo.toml"),
            "[package]\nname = \"refresh\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .expect("manifest");
        let path = directory.path().join("src/main.rs");
        std::fs::write(&path, "fn main() {}\n").expect("file");
        let mut model = Model::new(EditorState::from_text(""), None);
        model.workspace.path = directory.path().to_path_buf();
        model.load_project_info();
        model.session.path = Some(path);
        model.project.silent_symbol_refresh = true;
        let response = serde_json::json!([{
            "name": "silent_symbol",
            "detail": "function",
            "range": {
                "start": {"line": 0, "character": 3},
                "end": {"line": 0, "character": 7}
            },
            "selectionRange": {
                "start": {"line": 0, "character": 3},
                "end": {"line": 0, "character": 7}
            }
        }]);
        model.apply_document_symbols_response(&response);
        let runtime = model.project.platform.as_ref().expect("runtime");
        assert!(
            runtime
                .symbols()
                .symbols("silent_symbol", 10)
                .iter()
                .any(|symbol| symbol.name == "silent_symbol")
        );
        assert!(model.overlays.lsp_results.document_symbols.is_none());
    }
}

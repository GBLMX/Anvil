use super::*;

impl Model {
    pub(crate) fn request_workspace_symbols(&mut self) {
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_symbols();
        self.cancel_workspace_symbols();
        self.close_hover();
        self.close_diagnostic_peek();
        let Some(path) = self.session.path.as_ref() else {
            return;
        };
        let params = serde_json::json!({"query": ""});
        let _ = path;
        if self.send_lsp_request(LspRequestKind::WorkspaceSymbol, "workspace/symbol", params) {
            self.status.text = "workspace symbols".to_owned();
        }
        self.file.dirty = true;
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
        self.cancel_completion();
        self.cancel_code_actions();
        self.cancel_document_symbols();
        self.close_hover();
        self.close_diagnostic_peek();
        let Some(uri) = self
            .session
            .path
            .as_ref()
            .map(|path| lsp_worker::file_uri(path))
        else {
            return;
        };
        let params = serde_json::json!({"textDocument": {"uri": uri}});
        if self.send_lsp_request(
            LspRequestKind::DocumentSymbol,
            "textDocument/documentSymbol",
            params,
        ) {
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

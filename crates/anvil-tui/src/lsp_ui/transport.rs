use super::*;

impl Model {
    pub(crate) fn try_offset_at_utf16(&self, line: usize, character: usize) -> Option<usize> {
        self.session
            .editor
            .index()
            .offset_at_utf16(self.session.editor.text(), line, character)
    }

    pub(crate) fn offset_at_utf16(&self, line: usize, character: usize) -> usize {
        self.try_offset_at_utf16(line, character)
            .unwrap_or(self.session.editor.text().len_bytes())
    }
    pub(super) fn lsp_position(&self) -> Option<(u64, u64)> {
        let position = self
            .session
            .editor
            .index()
            .position(self.session.editor.text(), self.session.cursor)?;
        let character = self
            .session
            .editor
            .index()
            .utf16_column(self.session.editor.text(), self.session.cursor)?;
        Some((position.line as u64, character as u64))
    }
    pub(crate) fn send_lsp_request(
        &mut self,
        kind: LspRequestKind,
        method: &str,
        params: serde_json::Value,
    ) -> bool {
        let Some(path) = self.session.path.clone() else {
            self.status.text = "E: lsp requires an open file".to_owned();
            self.file.dirty = true;
            return false;
        };
        let id = self.language.lsp_requests.next_request;
        self.language.lsp_requests.next_request += 1;
        if self.active_lsp_worker().is_none() {
            self.status.text = "E: lsp is not running".to_owned();
            self.file.dirty = true;
            return false;
        }
        let text = self
            .session
            .editor
            .text()
            .slice(0..self.session.editor.text().len_bytes());
        if let Err(error) = self.sync_lsp_document(&path, text) {
            self.status.text = format!("E: lsp request: {error}");
            self.file.dirty = true;
            return false;
        }
        let Some(worker) = self.active_lsp_worker() else {
            self.status.text = "E: lsp is not running".to_owned();
            self.file.dirty = true;
            return false;
        };
        if let Err(error) = worker.request(id, method, params) {
            self.status.text = format!("E: lsp request: {error}");
            self.file.dirty = true;
            return false;
        }
        self.language.lsp_requests.pending.insert(id, kind);
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
                .lsp_requests
                .pending_edit_revision
                .insert(id, self.session.editor.revision());
            self.language
                .lsp_requests
                .pending_edit_path
                .insert(id, path);
            self.language
                .lsp_requests
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
    pub(super) fn record_lsp_version(&mut self, path: &Path) {
        if let Some(version) = self.language.lsp_requests.versions.get_mut(path) {
            *version += 1;
        } else {
            self.language
                .lsp_requests
                .versions
                .insert(path.to_path_buf(), 1);
        }
    }
    pub(crate) fn buffer_revisions(&self) -> HashMap<PathBuf, u64> {
        let mut revisions = HashMap::new();
        if let Some(path) = &self.session.path {
            revisions.insert(path.clone(), self.session.editor.revision());
        }
        for buffer in &self.buffers.entries {
            if let Some(path) = &buffer.path {
                revisions
                    .entry(path.clone())
                    .or_insert_with(|| buffer.editor.revision());
            }
        }
        if let Some(pane) = &self.buffers.split
            && let Some(path) = &pane.buffer.path
        {
            revisions
                .entry(path.clone())
                .or_insert_with(|| pane.buffer.editor.revision());
        }
        revisions
    }
}

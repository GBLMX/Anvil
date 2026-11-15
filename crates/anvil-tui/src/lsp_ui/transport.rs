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
        let id = self.language.lsp_requests.reserve_request_id();
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
        let request = PendingRequest::capture(kind, self.session.editor.revision(), path, || {
            self.buffer_revisions()
        });
        self.language.lsp_requests.track(id, request);
        true
    }
    pub(crate) fn sync_lsp_document(&mut self, path: &Path, text: String) -> Result<(), String> {
        let Some(worker) = self.active_lsp_worker() else {
            return Err("lsp is not running".to_owned());
        };
        match worker.sync_recoverable(path, text) {
            Ok(()) => {
                if self
                    .language
                    .lsp
                    .pending_sync
                    .as_ref()
                    .is_some_and(|(pending, _)| pending == path)
                {
                    self.language.lsp.pending_sync = None;
                }
                self.language.lsp_requests.record_version(path);
                Ok(())
            }
            Err((error, text)) => {
                self.language.lsp.pending_sync = Some((path.to_path_buf(), text));
                Err(error)
            }
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

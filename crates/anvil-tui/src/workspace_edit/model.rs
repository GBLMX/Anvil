use super::*;

pub(super) fn remap_path(path: &Path, from: &Path, to: &Path) -> PathBuf {
    if path == from {
        return to.to_path_buf();
    }
    if let Ok(rest) = path.strip_prefix(from) {
        return to.join(rest);
    }
    path.to_path_buf()
}

pub(super) fn path_is_open(model: &Model, path: &Path) -> bool {
    model.session.path.as_deref() == Some(path)
        || model
            .buffers
            .documents
            .values()
            .any(|document| document.path.as_deref() == Some(path))
}

pub(super) fn path_is_modified(model: &Model, path: &Path) -> bool {
    let affected = |candidate: &Path| candidate == path || candidate.starts_with(path);
    let active = model.session.path.as_deref().is_some_and(affected) && model.file.modified;
    let modified = model
        .buffers
        .documents
        .values()
        .any(|document| document.path.as_deref().is_some_and(affected) && document.modified);
    active || modified
}

impl Model {
    pub(super) fn retarget_path(&mut self, from: &Path, to: &Path) {
        if let Some(path) = self.session.path.as_mut() {
            *path = remap_path(path, from, to);
        }
        for document in self.buffers.documents.values_mut() {
            if let Some(path) = document.path.as_mut() {
                *path = remap_path(path, from, to);
                document.syntax = Default::default();
            }
        }
        let background = std::mem::take(&mut self.buffers.background);
        self.buffers.background = background
            .into_iter()
            .map(|(path, id)| (remap_path(&path, from, to), id))
            .collect();
        let diagnostics = std::mem::take(&mut self.language.diagnostics.by_path);
        self.language.diagnostics.by_path = diagnostics
            .into_iter()
            .map(|(path, value)| (remap_path(&path, from, to), value))
            .collect();
        self.language
            .lsp_requests
            .versions
            .retain(|path, _| path != from && !path.starts_with(from));
        self.workspace.git.diff_checked = None;
        self.workspace.git.marks.clear();
        self.clear_lsp_ui();
        self.syntax_cache
            .highlights
            .borrow_mut()
            .switch_path(self.session.path.as_deref(), true);
        self.workspace.tree.stale = true;
        self.sync_watch_paths();
        self.file.dirty = true;
    }

    pub(super) fn close_paths_under(&mut self, path: &Path) {
        let affected = |candidate: Option<&Path>| {
            candidate.is_some_and(|candidate| candidate == path || candidate.starts_with(path))
        };
        let split_document = self
            .buffers
            .split
            .as_ref()
            .map(|pane| pane.buffer.document)
            .filter(|id| {
                self.buffers
                    .document(*id)
                    .is_some_and(|document| affected(document.path.as_deref()))
            });
        if split_document.is_some() {
            self.buffers.split = None;
            self.ui.focus.split = false;
        }
        while let Some(index) = self.buffers.entries.iter().position(|view| {
            self.buffers
                .document(view.document)
                .is_some_and(|document| affected(document.path.as_deref()))
        }) {
            self.close_buffer(index, true);
        }
        let removed_background = self
            .buffers
            .background
            .iter()
            .filter(|(path, _)| affected(Some(path)))
            .map(|(_, id)| *id)
            .collect::<Vec<_>>();
        self.buffers
            .background
            .retain(|path, _| !affected(Some(path)));
        for id in removed_background.into_iter().chain(split_document) {
            self.prune_document(id);
        }
        self.workspace.tree.stale = true;
        self.sync_watch_paths();
        self.file.dirty = true;
    }

    pub(crate) fn lsp_document_version(&self, path: &Path) -> Option<u64> {
        self.language.lsp_requests.versions.get(path).copied()
    }
}

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
            .entries
            .iter()
            .any(|buffer| buffer.path.as_deref() == Some(path))
        || model
            .buffers
            .background
            .values()
            .any(|buffer| buffer.path.as_deref() == Some(path))
        || model
            .buffers
            .split
            .as_ref()
            .is_some_and(|pane| pane.buffer.path.as_deref() == Some(path))
}

pub(super) fn path_is_modified(model: &Model, path: &Path) -> bool {
    let affected = |candidate: &Path| candidate == path || candidate.starts_with(path);
    let active = model.session.path.as_deref().is_some_and(affected) && model.file.modified;
    let buffer = model
        .buffers
        .entries
        .iter()
        .any(|buffer| buffer.path.as_deref().is_some_and(affected) && buffer.modified);
    let background = model
        .buffers
        .background
        .values()
        .any(|buffer| buffer.path.as_deref().is_some_and(affected) && buffer.modified);
    let split = model.buffers.split.as_ref().is_some_and(|pane| {
        pane.buffer.path.as_deref().is_some_and(affected) && pane.buffer.modified
    });
    active || buffer || background || split
}

impl Model {
    pub(super) fn retarget_path(&mut self, from: &Path, to: &Path) {
        if let Some(path) = self.session.path.as_mut() {
            *path = remap_path(path, from, to);
        }
        for buffer in &mut self.buffers.entries {
            if let Some(path) = buffer.path.as_mut() {
                *path = remap_path(path, from, to);
            }
        }
        let background = std::mem::take(&mut self.buffers.background);
        self.buffers.background = background
            .into_iter()
            .map(|(path, mut buffer)| {
                let path = remap_path(&path, from, to);
                buffer.path = Some(path.clone());
                buffer.syntax = Default::default();
                (path, buffer)
            })
            .collect();
        if let Some(pane) = self.buffers.split.as_mut()
            && let Some(path) = pane.buffer.path.as_mut()
        {
            *path = remap_path(path, from, to);
        }
        if let Some(back) = self.pager.back.as_mut()
            && let Some(path) = back.path.as_mut()
        {
            *path = remap_path(path, from, to);
        }
        let diagnostics = std::mem::take(&mut self.language.diagnostics.by_path);
        self.language.diagnostics.by_path = diagnostics
            .into_iter()
            .map(|(path, value)| (remap_path(&path, from, to), value))
            .collect();
        self.language
            .lsp_requests
            .versions
            .retain(|path, _| path != from && !path.starts_with(from));
        self.git.diff_checked = None;
        self.git.marks.clear();
        self.clear_lsp_ui();
        self.syntax_cache
            .highlights
            .borrow_mut()
            .switch_path(self.session.path.as_deref(), true);
        self.tree.stale = true;
        self.sync_watch_paths();
        self.file.dirty = true;
    }

    pub(super) fn close_paths_under(&mut self, path: &Path) {
        let affected = |candidate: Option<&Path>| {
            candidate.is_some_and(|candidate| candidate == path || candidate.starts_with(path))
        };
        if self
            .buffers
            .split
            .as_ref()
            .is_some_and(|pane| affected(pane.buffer.path.as_deref()))
        {
            self.buffers.split = None;
            self.ui.focus.split = false;
        }
        loop {
            let Some(index) = self
                .buffers
                .entries
                .iter()
                .position(|buffer| affected(buffer.path.as_deref()))
            else {
                break;
            };
            self.close_buffer(index, true);
        }
        self.buffers
            .background
            .retain(|path, _| !affected(Some(path)));
        self.tree.stale = true;
        self.sync_watch_paths();
        self.file.dirty = true;
    }

    pub(crate) fn lsp_document_version(&self, path: &Path) -> Option<u64> {
        self.language.lsp_requests.versions.get(path).copied()
    }
}

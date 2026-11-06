//! Fuzzy file finder and preview state.

use super::*;

impl Model {
    pub(crate) fn clear_preview(&mut self) {
        self.preview_generation = self.preview_generation.saturating_add(1);
        self.preview = PreviewState::default();
    }

    fn selected_fuzzy_path(&self) -> Option<String> {
        let fuzzy = self.fuzzy.as_ref()?;
        fuzzy
            .filtered
            .get(fuzzy.selected)
            .map(|index| fuzzy.entries[*index].clone())
    }

    pub(crate) fn request_preview(&mut self) {
        let Some(path) = self.selected_fuzzy_path() else {
            self.clear_preview();
            return;
        };
        if self.preview.path.as_deref() == Some(path.as_str())
            && (self.preview.loading || self.preview.message.is_none())
        {
            return;
        }
        if self.file_worker.is_none() {
            self.preview_generation = self.preview_generation.saturating_add(1);
            self.preview = PreviewState {
                path: Some(path),
                message: Some("preview unavailable: gateway".to_owned()),
                ..PreviewState::default()
            };
            self.status = "E: preview: gateway unavailable".to_owned();
            self.dirty = true;
            return;
        }
        self.preview_generation = self.preview_generation.saturating_add(1);
        let generation = self.preview_generation;
        self.preview = PreviewState {
            path: Some(path.clone()),
            loading: true,
            ..PreviewState::default()
        };
        if let Some(worker) = self.file_worker.as_ref()
            && worker
                .request(FileRequest::Preview { path, generation })
                .is_err()
        {
            self.preview.loading = false;
            self.preview.message = Some("preview worker stopped".to_owned());
            self.status = "E: preview: worker stopped".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn handle_preview(
        &mut self,
        path: String,
        generation: u64,
        result: Result<PreviewData, String>,
    ) {
        if generation != self.preview_generation
            || self.preview.path.as_deref() != Some(path.as_str())
        {
            return;
        }
        self.preview.loading = false;
        match result {
            Ok(preview) => {
                if self.status.starts_with("E: preview:") {
                    self.status = "preview ready".to_owned();
                }
                self.preview.lines = preview.lines;
                self.preview.truncated = preview.truncated;
                self.preview.message = None;
            }
            Err(error) => {
                self.preview.lines.clear();
                self.preview.truncated = false;
                self.preview.message = Some(error.clone());
                self.status = format!("E: preview: {error}");
            }
        }
        self.dirty = true;
    }

    pub(crate) fn start_fuzzy(&mut self) {
        self.cancel_completion();
        self.clear_lsp_requests();
        self.hover = None;
        self.ui.diagnostic_peek = None;
        self.code_actions = None;
        self.document_symbols = None;
        self.clear_preview();
        let workspace = self.workspace.clone();
        if self.ensure_file_worker() {
            if let Some(worker) = self.file_worker.as_ref() {
                let _ = worker.request(FileRequest::Files {
                    hidden: self.show_hidden,
                });
            }
            self.fuzzy = Some(FuzzyState {
                query: String::new(),
                entries: Vec::new(),
                filtered: Vec::new(),
                selected: 0,
            });
            self.status = "loading files".to_owned();
            self.dirty = true;
            return;
        }
        let mut entries = Vec::new();
        if let Err(error) = self.collect_files(&workspace, "", &mut entries) {
            self.status = format!("E: files: {error}");
            self.dirty = true;
            return;
        }
        entries.sort();
        let filtered = (0..entries.len()).collect();
        self.fuzzy = Some(FuzzyState {
            query: String::new(),
            entries,
            filtered,
            selected: 0,
        });
        self.request_preview();
        self.dirty = true;
    }

    pub(crate) fn fuzzy_refresh(&mut self) {
        let Some(fuzzy) = self.fuzzy.as_mut() else {
            return;
        };
        let query = fuzzy.query.to_lowercase();
        fuzzy.filtered = fuzzy
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.to_lowercase().contains(&query))
            .map(|(index, _)| index)
            .collect();
        fuzzy.selected = fuzzy.selected.min(fuzzy.filtered.len().saturating_sub(1));
        self.request_preview();
    }

    pub(crate) fn fuzzy_move(&mut self, delta: isize) {
        let Some(fuzzy) = self.fuzzy.as_mut() else {
            return;
        };
        fuzzy.selected = fuzzy
            .selected
            .saturating_add_signed(delta)
            .min(fuzzy.filtered.len().saturating_sub(1));
        self.request_preview();
        self.dirty = true;
    }

    pub(crate) fn fuzzy_open(&mut self) {
        let Some(fuzzy) = self.fuzzy.take() else {
            return;
        };
        self.clear_preview();
        let Some(index) = fuzzy.filtered.get(fuzzy.selected) else {
            self.dirty = true;
            return;
        };
        let path = self.workspace.join(&fuzzy.entries[*index]);
        match self.open_path(&path) {
            Ok(()) => {}
            Err(error) => self.status = format!("E: {error}"),
        }
        self.dirty = true;
    }
}

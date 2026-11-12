//! Fuzzy file finder and preview state.

use super::*;

impl Model {
    pub(crate) fn clear_preview(&mut self) {
        self.preview_generation = self.preview_generation.saturating_add(1);
        self.preview = PreviewState::default();
    }

    pub(crate) fn fuzzy_display_path(&self, entry: &str) -> String {
        let path = Path::new(entry);
        if let Some(root) = self
            .root_file_workers
            .keys()
            .find(|root| path.starts_with(root))
            && let Ok(relative) = path.strip_prefix(root)
        {
            let name = root
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_else(|| root.to_string_lossy());
            return format!("{name}/{}", relative.display());
        }
        entry.to_owned()
    }

    fn fuzzy_owner_root(&self, path: &Path) -> Option<PathBuf> {
        self.root_file_workers
            .keys()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .cloned()
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
        let owner = self.fuzzy_owner_root(Path::new(&path));
        let request_path = match owner.as_ref() {
            Some(root) => match Path::new(&path).strip_prefix(root) {
                Ok(relative) => relative.to_string_lossy().into_owned(),
                Err(_) => path.clone(),
            },
            None => path.clone(),
        };
        let has_worker = match owner.as_ref() {
            Some(root) => self.root_file_workers.contains_key(root),
            None => self.file_worker.is_some(),
        };
        if !has_worker {
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
            path: Some(path),
            loading: true,
            ..PreviewState::default()
        };
        let request = match owner.as_ref() {
            Some(root) => self.root_file_workers.get(root).map(|worker| {
                worker.request(FileRequest::Preview {
                    path: request_path,
                    generation,
                })
            }),
            None => self.file_worker.as_ref().map(|worker| {
                worker.request(FileRequest::Preview {
                    path: request_path,
                    generation,
                })
            }),
        };
        if !matches!(request, Some(Ok(()))) {
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
        let generation = self.fuzzy_generation.saturating_add(1);
        self.fuzzy_generation = generation;
        if self.ensure_file_worker() {
            if let Some(worker) = self.file_worker.as_ref() {
                let _ = worker.request(FileRequest::Files {
                    hidden: self.show_hidden,
                    generation,
                });
            }
            for worker in self.root_file_workers.values() {
                let _ = worker.request(FileRequest::Files {
                    hidden: self.show_hidden,
                    generation,
                });
            }
            self.fuzzy = Some(FuzzyState {
                generation,
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
            generation,
            query: String::new(),
            entries,
            filtered,
            selected: 0,
        });
        self.request_preview();
        self.dirty = true;
    }

    pub(crate) fn handle_fuzzy_files(
        &mut self,
        root: Option<PathBuf>,
        generation: u64,
        result: Result<Vec<String>, String>,
    ) {
        {
            let Some(fuzzy) = self.fuzzy.as_mut() else {
                return;
            };
            if fuzzy.generation != generation {
                return;
            }
            match result {
                Ok(files) => {
                    if let Some(root) = root {
                        fuzzy.entries.extend(
                            files
                                .into_iter()
                                .map(|relative| root.join(relative).to_string_lossy().into_owned()),
                        );
                        fuzzy.entries.sort();
                        fuzzy.entries.dedup();
                    } else {
                        fuzzy.entries.extend(files);
                    }
                    fuzzy.entries.sort();
                    fuzzy.entries.dedup();
                }
                Err(error) => {
                    self.status = format!("E: files: {error}");
                    self.dirty = true;
                    return;
                }
            }
        }
        self.fuzzy_refresh();
        self.status = "files ready".to_owned();
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
        let entry = &fuzzy.entries[*index];
        let path = if Path::new(entry).is_absolute() {
            PathBuf::from(entry)
        } else {
            self.workspace.join(entry)
        };
        self.record_jump();
        match self.open_path(&path) {
            Ok(()) => {}
            Err(error) => self.status = format!("E: {error}"),
        }
        self.dirty = true;
    }
}

//! Fuzzy file finder and preview state.

use super::*;
use crate::OverlayKind;

impl Model {
    pub(crate) fn clear_preview(&mut self) {
        self.overlays.preview_generation = self.overlays.preview_generation.saturating_add(1);
        self.overlays.preview = PreviewState::default();
    }

    pub(crate) fn fuzzy_display_path(&self, entry: &str) -> String {
        let path = Path::new(entry);
        if let Some(root) = self
            .workspace
            .root_file_workers
            .keys()
            .find(|root| path.starts_with(root))
            && let Ok(relative) = path.strip_prefix(root)
        {
            let name = self.root_label(root);
            return format!("{name}/{}", relative.display());
        }
        entry.to_owned()
    }

    fn fuzzy_owner_root(&self, path: &Path) -> Option<PathBuf> {
        self.workspace
            .root_file_workers
            .keys()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .cloned()
    }

    fn selected_fuzzy_path(&self) -> Option<String> {
        let fuzzy = self.overlays.fuzzy.as_ref()?;
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
        if self.overlays.preview.path.as_deref() == Some(path.as_str())
            && (self.overlays.preview.loading || self.overlays.preview.message.is_none())
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
            Some(root) => self.workspace.root_file_workers.contains_key(root),
            None => self.workspace.file_worker.is_some(),
        };
        if !has_worker {
            self.overlays.preview_generation = self.overlays.preview_generation.saturating_add(1);
            self.overlays.preview = PreviewState {
                path: Some(path),
                message: Some("preview unavailable: gateway".to_owned()),
                ..PreviewState::default()
            };
            self.status.text = "E: preview: gateway unavailable".to_owned();
            self.file.dirty = true;
            return;
        }
        self.overlays.preview_generation = self.overlays.preview_generation.saturating_add(1);
        let generation = self.overlays.preview_generation;
        self.overlays.preview = PreviewState {
            path: Some(path),
            loading: true,
            ..PreviewState::default()
        };
        let request = match owner.as_ref() {
            Some(root) => self.workspace.root_file_workers.get(root).map(|worker| {
                worker.request(FileRequest::Preview {
                    path: request_path,
                    generation,
                })
            }),
            None => self.workspace.file_worker.as_ref().map(|worker| {
                worker.request(FileRequest::Preview {
                    path: request_path,
                    generation,
                })
            }),
        };
        if !matches!(request, Some(Ok(()))) {
            self.overlays.preview.loading = false;
            self.overlays.preview.message = Some("preview worker stopped".to_owned());
            self.status.text = "E: preview: worker stopped".to_owned();
        }
        self.file.dirty = true;
    }

    pub(crate) fn handle_preview(
        &mut self,
        path: String,
        generation: u64,
        result: Result<PreviewData, String>,
    ) {
        if generation != self.overlays.preview_generation
            || self.overlays.preview.path.as_deref() != Some(path.as_str())
        {
            return;
        }
        self.overlays.preview.loading = false;
        match result {
            Ok(preview) => {
                if self.status.text.starts_with("E: preview:") {
                    self.status.text = "preview ready".to_owned();
                }
                self.overlays.preview.lines = preview.lines;
                self.overlays.preview.truncated = preview.truncated;
                self.overlays.preview.message = None;
            }
            Err(error) => {
                self.overlays.preview.lines.clear();
                self.overlays.preview.truncated = false;
                self.overlays.preview.message = Some(error.clone());
                self.status.text = format!("E: preview: {error}");
            }
        }
        self.file.dirty = true;
    }

    pub(crate) fn start_fuzzy(&mut self) {
        self.cancel_completion();
        self.clear_lsp_requests();
        self.overlays.lsp_results.hover = None;
        self.ui.diagnostic_peek = None;
        self.overlays.lsp_results.code_actions = None;
        self.overlays.lsp_results.document_symbols = None;
        self.clear_preview();
        let workspace = self.workspace.path.clone();
        let generation = self.overlays.fuzzy_generation.saturating_add(1);
        self.overlays.fuzzy_generation = generation;
        if self.ensure_file_worker() {
            if let Some(worker) = self.workspace.file_worker.as_ref() {
                let _ = worker.request(FileRequest::Files {
                    hidden: self.tree.show_hidden,
                    generation,
                });
            }
            for worker in self.workspace.root_file_workers.values() {
                let _ = worker.request(FileRequest::Files {
                    hidden: self.tree.show_hidden,
                    generation,
                });
            }
            self.overlays.fuzzy = Some(FuzzyState {
                generation,
                query: String::new(),
                entries: Vec::new(),
                filtered: Vec::new(),
                selected: 0,
            });
            self.push_overlay(OverlayKind::Fuzzy);
            self.status.text = "loading files".to_owned();
            self.file.dirty = true;
            return;
        }
        let mut entries = Vec::new();
        if let Err(error) = self.collect_files(&workspace, "", &mut entries) {
            self.status.text = format!("E: files: {error}");
            self.file.dirty = true;
            return;
        }
        entries.sort();
        let filtered = (0..entries.len()).collect();
        self.overlays.fuzzy = Some(FuzzyState {
            generation,
            query: String::new(),
            entries,
            filtered,
            selected: 0,
        });
        self.push_overlay(OverlayKind::Fuzzy);
        self.request_preview();
        self.file.dirty = true;
    }

    pub(crate) fn handle_fuzzy_files(
        &mut self,
        root: Option<PathBuf>,
        generation: u64,
        result: Result<Vec<String>, String>,
    ) {
        if let Some(root) = root.as_deref()
            && !self
                .workspace
                .roots
                .list()
                .iter()
                .any(|candidate| candidate.path == root && candidate.trusted)
        {
            return;
        }
        {
            let Some(fuzzy) = self.overlays.fuzzy.as_mut() else {
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
                    self.status.text = format!("E: files: {error}");
                    self.file.dirty = true;
                    return;
                }
            }
        }
        self.fuzzy_refresh();
        self.status.text = "files ready".to_owned();
    }

    pub(crate) fn fuzzy_refresh(&mut self) {
        let Some(fuzzy) = self.overlays.fuzzy.as_mut() else {
            return;
        };
        let query = fuzzy.query.to_lowercase();
        let mut matched = fuzzy
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.to_lowercase().contains(&query))
            .map(|(index, entry)| {
                let score = fuzzy_score(entry, &query, self.recent.frecency.get(entry).copied());
                (score, index)
            })
            .collect::<Vec<_>>();
        matched.sort_by(|(left, left_index), (right, right_index)| {
            left.cmp(right).then(left_index.cmp(right_index))
        });
        fuzzy.filtered = matched.into_iter().map(|(_, index)| index).collect();
        fuzzy.selected = fuzzy.selected.min(fuzzy.filtered.len().saturating_sub(1));
        self.request_preview();
    }

    pub(crate) fn fuzzy_move(&mut self, delta: isize) {
        let Some(fuzzy) = self.overlays.fuzzy.as_mut() else {
            return;
        };
        fuzzy.selected = fuzzy
            .selected
            .saturating_add_signed(delta)
            .min(fuzzy.filtered.len().saturating_sub(1));
        self.request_preview();
        self.file.dirty = true;
    }

    pub(crate) fn close_fuzzy(&mut self) {
        self.overlays.fuzzy = None;
        self.close_overlay(OverlayKind::Fuzzy);
        self.clear_preview();
        self.file.dirty = true;
    }

    pub(crate) fn fuzzy_open(&mut self) {
        let Some(fuzzy) = self.overlays.fuzzy.take() else {
            return;
        };
        self.close_overlay(OverlayKind::Fuzzy);
        self.clear_preview();
        let Some(index) = fuzzy.filtered.get(fuzzy.selected) else {
            self.file.dirty = true;
            return;
        };
        let entry = fuzzy.entries[*index].clone();
        let path = if Path::new(&entry).is_absolute() {
            PathBuf::from(&entry)
        } else {
            self.workspace.path.join(&entry)
        };
        let count = self.recent.frecency.entry(entry).or_default();
        *count = count.saturating_add(1);
        self.record_jump();
        match self.open_path(&path) {
            Ok(()) => {}
            Err(error) => self.status.text = format!("E: {error}"),
        }
        self.file.dirty = true;
    }
}

fn fuzzy_score(
    entry: &str,
    query: &str,
    frecency: Option<u32>,
) -> (u8, u8, usize, std::cmp::Reverse<u32>, usize, String) {
    let lower = entry.to_lowercase();
    let basename = Path::new(entry)
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| lower.clone());
    let basename_match = basename.contains(query);
    let basename_prefix = if !query.is_empty() && basename.starts_with(query) {
        0
    } else {
        1
    };
    let position = if basename_match {
        basename.find(query).unwrap_or(usize::MAX)
    } else {
        lower.find(query).unwrap_or(usize::MAX)
    };
    let depth = Path::new(entry).components().count();
    (
        if basename_match { 0 } else { 1 },
        basename_prefix,
        position,
        std::cmp::Reverse(frecency.unwrap_or(0)),
        depth,
        lower,
    )
}

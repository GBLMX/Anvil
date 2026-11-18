//! Fuzzy file finder and preview state.

use crate::file_query::{FILE_PICKER_RESULT_LIMIT, FileMatch, FileQuery};
use anvil_core::TextBuffer;

use super::*;
use crate::OverlayKind;

impl Model {
    pub(crate) fn clear_preview(&mut self) {
        self.overlays.preview_generation = self.overlays.preview_generation.saturating_add(1);
        self.overlays.preview_syntax.invalidate();
        self.overlays.preview = PreviewState::default();
    }

    pub(crate) fn poll_preview_syntax(&mut self) {
        if self.overlays.preview.lines.is_empty() {
            return;
        }
        let lines = self.overlays.preview.lines.clone();
        let path = self.overlays.preview.path.clone();
        let revision = self.overlays.preview_generation;
        let text = lines.join("\n");
        let document = anvil_core::TextDocument::from_text(&text);
        if self
            .overlays
            .preview_syntax
            .poll(&document, revision, path.as_deref().map(Path::new))
        {
            let syntax = self.overlays.preview_syntax.active_cache();
            self.overlays.preview.syntax = syntax;
            self.file.dirty = true;
        }
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
            let name = self.workspace.root_label(root);
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

    fn cached_fuzzy_entries(&self) -> Vec<String> {
        let roots = self.workspace.roots.list();
        let mut entries = Vec::new();
        for ((root, hidden), files) in &self.workspace.file_cache {
            if *hidden != self.workspace.tree.show_hidden {
                continue;
            }
            if !roots
                .iter()
                .any(|candidate| candidate.path == *root && candidate.trusted)
            {
                continue;
            }
            entries.extend(
                files
                    .iter()
                    .map(|relative| root.join(relative).to_string_lossy().into_owned()),
            );
        }
        entries.sort();
        entries.dedup();
        entries
    }

    pub(crate) fn request_preview(&mut self) {
        let Some(path) = self.selected_fuzzy_path() else {
            self.clear_preview();
            return;
        };
        if self.overlays.preview.loading {
            if self.overlays.preview.path.as_deref() != Some(path.as_str()) {
                self.overlays.preview.pending = Some(path);
            }
            return;
        }
        if self.overlays.preview.path.as_deref() == Some(path.as_str())
            && self.overlays.preview.message.is_none()
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

    pub(crate) fn scroll_preview(&mut self, delta: isize) {
        let preview = &mut self.overlays.preview;
        let max = preview.lines.len().saturating_sub(1);
        preview.scroll = preview.scroll.saturating_add_signed(delta).min(max);
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
        self.overlays.preview_syntax.invalidate();
        self.overlays.preview.syntax = Default::default();
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
        let pending = self.overlays.preview.pending.take();
        if pending.is_some()
            && self.selected_fuzzy_path().as_deref() != self.overlays.preview.path.as_deref()
        {
            self.request_preview();
        }
        self.file.dirty = true;
    }

    pub(crate) fn start_fuzzy(&mut self) {
        self.cancel_completion();
        self.clear_lsp_requests();
        self.close_hover();
        self.close_diagnostic_peek();
        self.overlays.lsp_results.code_actions = None;
        self.close_overlay(OverlayKind::CodeActions);
        self.overlays.lsp_results.document_symbols = None;
        self.close_overlay(OverlayKind::DocumentSymbols);
        self.clear_preview();
        if let Some(cancel) = self.workspace.fuzzy_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.workspace.fuzzy_cancel = Some(cancel.clone());
        let workspace = self.workspace.path.clone();
        let generation = self.overlays.fuzzy_generation.saturating_add(1);
        self.overlays.fuzzy_generation = generation;
        let cached = self.cached_fuzzy_entries();
        let cached_empty = cached.is_empty();
        if self.ensure_file_worker() {
            if let Some(worker) = self.workspace.file_worker.as_ref() {
                let _ = worker.request(FileRequest::Files {
                    hidden: self.workspace.tree.show_hidden,
                    generation,
                    cancel: cancel.clone(),
                });
            }
            for worker in self.workspace.root_file_workers.values() {
                let _ = worker.request(FileRequest::Files {
                    hidden: self.workspace.tree.show_hidden,
                    generation,
                    cancel: cancel.clone(),
                });
            }
            self.overlays.fuzzy = Some(FuzzyState {
                generation,
                query: String::new(),
                entries: cached,
                filtered: Vec::new(),
                selected: 0,
                truncated: false,
            });
            self.push_overlay(OverlayKind::Fuzzy);
            if cached_empty {
                self.status.text = "loading files".to_owned();
            } else {
                self.fuzzy_refresh();
                self.status.text = "files ready (cached)".to_owned();
            }
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
            truncated: false,
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
            if let Some(root) = root.as_ref()
                && let Ok(files) = &result
            {
                self.workspace.file_cache.insert(
                    (root.clone(), self.workspace.tree.show_hidden),
                    files.clone(),
                );
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
        let (mut matched, selected) = {
            let Some(fuzzy) = self.overlays.fuzzy.as_ref() else {
                return;
            };
            let query = FileQuery::parse(&fuzzy.query);
            let matched = fuzzy
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    let match_path = self.fuzzy_display_path(entry);
                    query.matches(&match_path).map(|matched| {
                        let score = fuzzy_score(
                            &match_path,
                            &matched,
                            self.workspace.recent.frecency.get(entry).copied(),
                        );
                        (score, index)
                    })
                })
                .collect::<Vec<_>>();
            (matched, fuzzy.selected)
        };
        matched.sort_by(|(left, left_index), (right, right_index)| {
            left.cmp(right).then(left_index.cmp(right_index))
        });
        let truncated = matched.len() > FILE_PICKER_RESULT_LIMIT;
        matched.truncate(FILE_PICKER_RESULT_LIMIT);
        let Some(fuzzy) = self.overlays.fuzzy.as_mut() else {
            return;
        };
        fuzzy.filtered = matched.into_iter().map(|(_, index)| index).collect();
        fuzzy.selected = selected.min(fuzzy.filtered.len().saturating_sub(1));
        fuzzy.truncated = truncated;
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
        if let Some(cancel) = self.workspace.fuzzy_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.overlays.fuzzy = None;
        self.close_overlay(OverlayKind::Fuzzy);
        self.clear_preview();
        self.file.dirty = true;
    }

    pub(crate) fn toggle_fuzzy_hidden(&mut self) {
        let query = self
            .overlays
            .fuzzy
            .as_ref()
            .map(|fuzzy| fuzzy.query.clone())
            .unwrap_or_default();
        self.workspace.tree.show_hidden = !self.workspace.tree.show_hidden;
        self.start_fuzzy();
        if let Some(fuzzy) = self.overlays.fuzzy.as_mut() {
            fuzzy.query = query;
        }
        self.fuzzy_refresh();
    }

    pub(crate) fn fuzzy_open(&mut self) {
        self.fuzzy_open_mode(false);
    }

    pub(crate) fn fuzzy_open_split(&mut self) {
        self.fuzzy_open_mode(true);
    }

    pub(crate) fn fuzzy_copy_path(&mut self) {
        let Some(path) = self.selected_fuzzy_path() else {
            self.status.text = "E: no file selected".to_owned();
            self.file.dirty = true;
            return;
        };
        if self.record_register_text('+', &path) {
            self.status.text = format!("copied {path}");
            self.file.dirty = true;
        }
    }

    fn fuzzy_open_mode(&mut self, split: bool) {
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
        let count = self.workspace.recent.frecency.entry(entry).or_default();
        *count = count.saturating_add(1);
        self.record_jump();
        if split {
            self.split_vertical();
            self.focus_split(true);
        }
        match self.open_path(&path) {
            Ok(()) => {}
            Err(error) => self.status.text = format!("E: {error}"),
        }
        self.file.dirty = true;
    }
}

fn fuzzy_score(
    entry: &str,
    matched: &FileMatch,
    frecency: Option<u32>,
) -> (u8, u8, usize, std::cmp::Reverse<u32>, usize, String) {
    let lower = entry.to_lowercase();
    let depth = Path::new(entry).components().count();
    let basename_start = entry.rfind('/').map(|index| index + 1).unwrap_or(0);
    let position = if matched.basename_match {
        matched.first_position.saturating_sub(basename_start)
    } else {
        matched.first_position
    };
    (
        if matched.basename_match { 0 } else { 1 },
        if matched.basename_prefix { 0 } else { 1 },
        position,
        std::cmp::Reverse(frecency.unwrap_or(0)),
        depth,
        lower,
    )
}

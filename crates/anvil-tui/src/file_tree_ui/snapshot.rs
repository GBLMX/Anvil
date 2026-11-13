use super::*;

impl Model {
    pub(super) fn expanded_tree_paths(&self) -> BTreeSet<PathBuf> {
        self.tree
            .entries
            .iter()
            .filter(|entry| entry.is_dir && entry.expanded)
            .map(|entry| entry.path.clone())
            .collect()
    }

    pub(crate) fn sync_tree_watches(&mut self) {
        let workspace = self.workspace.path.clone();
        let mut directories: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
        directories
            .entry(workspace.clone())
            .or_default()
            .push(workspace.clone());
        for root in self.workspace.roots.list() {
            if root.trusted && root.path != workspace {
                directories
                    .entry(root.path.clone())
                    .or_default()
                    .push(root.path.clone());
            }
        }
        for path in self.expanded_tree_paths() {
            if path.starts_with(&workspace) {
                directories.entry(workspace.clone()).or_default().push(path);
            } else if let Some(root) = self.secondary_root(&path)
                && self
                    .workspace
                    .roots
                    .owning_root(&path)
                    .is_some_and(|root| root.trusted)
            {
                directories.entry(root).or_default().push(path);
            }
        }
        let per_root = (MAX_TREE_WATCH_DIRS / directories.len().max(1)).max(1);
        let mut limited = false;
        for paths in directories.values_mut() {
            paths.sort();
            paths.dedup();
            if paths.len() > per_root {
                paths.truncate(per_root);
                limited = true;
            }
        }
        if limited && !self.tree.watch_limited {
            self.status.text =
                format!("tree watch limit reached ({MAX_TREE_WATCH_DIRS}); auto-refresh limited");
        }
        self.tree.watch_limited = limited;
        if !self.ensure_file_worker() {
            self.tree.stale = true;
            return;
        }
        let mut sent = 0usize;
        let mut expected = 0usize;
        for (root, paths) in directories {
            expected += 1;
            let request = if root == workspace {
                self.workspace
                    .file_worker
                    .as_ref()
                    .map(|worker| worker.request(FileRequest::WatchDirs(paths)))
            } else {
                self.workspace
                    .root_file_workers
                    .get(&root)
                    .map(|worker| worker.request(FileRequest::WatchDirs(paths)))
            };
            if matches!(request, Some(Ok(()))) {
                sent += 1;
            }
        }
        self.tree.stale = sent < expected;
    }

    pub(super) fn request_tree_refresh(&mut self, select: Option<PathBuf>) {
        if select.is_some() {
            self.tree.pending_select = select;
        }
        if self.tree.refresh_in_flight {
            self.tree.refresh_queued = true;
            return;
        }
        let expanded = self
            .expanded_tree_paths()
            .into_iter()
            .filter(|path| path.starts_with(&self.workspace.path))
            .collect();
        if self.ensure_file_worker() {
            self.tree.refresh_generation = self.tree.refresh_generation.wrapping_add(1);
            let generation = self.tree.refresh_generation;
            let request = FileRequest::TreeSnapshot {
                generation,
                root: self.workspace.path.clone(),
                expanded,
            };
            let sent = self
                .workspace
                .file_worker
                .as_ref()
                .is_some_and(|worker| worker.request(request).is_ok());
            self.tree.refresh_in_flight = sent;
            self.tree.stale = !sent;
            return;
        }
        self.rebuild_tree_from_disk();
    }

    pub(super) fn refresh_tree_background(&mut self) {
        self.request_tree_refresh(None);
    }

    pub(super) fn rebuild_tree_from_disk(&mut self) {
        let expanded = self
            .expanded_tree_paths()
            .into_iter()
            .filter(|path| path.starts_with(&self.workspace.path))
            .collect::<BTreeSet<_>>();
        let workspace = self.workspace.path.clone();
        let mut listings = Vec::new();
        listings.push((workspace.clone(), self.tree_entries(&workspace)));
        for path in expanded {
            if let Ok(entries) = self.tree_entries(&path) {
                listings.push((path, Ok(entries)));
            }
        }
        self.tree.refresh_generation = self.tree.refresh_generation.wrapping_add(1);
        let generation = self.tree.refresh_generation;
        self.apply_tree_snapshot(generation, listings);
    }

    pub(super) fn build_tree_entries(
        &self,
        directory: &Path,
        depth: usize,
        listings: &BTreeMap<PathBuf, Result<Vec<DirEntry>, String>>,
        expanded: &BTreeSet<PathBuf>,
        out: &mut Vec<TreeEntry>,
    ) -> bool {
        let Some(Ok(entries)) = listings.get(directory) else {
            return false;
        };
        let mut needs_listing = false;
        for entry in entries {
            if !self.tree_entry_visible(&entry.name, entry.is_dir) {
                continue;
            }
            let path = directory.join(&entry.name);
            let listing = listings.get(&path);
            let is_expanded =
                entry.is_dir && expanded.contains(&path) && !matches!(listing, Some(Err(_)));
            out.push(TreeEntry {
                path: path.clone(),
                depth,
                is_dir: entry.is_dir,
                ino: entry.ino,
                expanded: is_expanded,
            });
            if is_expanded {
                match listing {
                    Some(Ok(_)) => {
                        needs_listing |=
                            self.build_tree_entries(&path, depth + 1, listings, expanded, out);
                    }
                    Some(Err(_)) => {}
                    None => needs_listing = true,
                }
            }
        }
        needs_listing
    }

    pub(super) fn select_tree_path(
        tree: &[TreeEntry],
        target: Option<&Path>,
        fallback_index: Option<usize>,
        identity: Option<u64>,
    ) -> Option<usize> {
        if let Some(path) = target {
            if let Some(index) = tree.iter().position(|entry| entry.path == path) {
                return Some(index);
            }
            if let Some(ino) = identity
                && let Some(index) = tree.iter().position(|entry| entry.ino == ino)
            {
                return Some(index);
            }
            for ancestor in path.ancestors().skip(1) {
                if let Some(index) = tree.iter().position(|entry| entry.path == ancestor) {
                    return Some(index);
                }
            }
        }
        fallback_index
            .map(|index| index.min(tree.len().saturating_sub(1)))
            .filter(|_| !tree.is_empty())
            .or_else(|| (!tree.is_empty()).then_some(0))
    }

    pub(super) fn apply_tree_snapshot(
        &mut self,
        generation: u64,
        listings: Vec<(PathBuf, Result<Vec<DirEntry>, String>)>,
    ) {
        if generation != self.tree.refresh_generation {
            return;
        }
        self.tree.refresh_in_flight = false;
        let previous = self
            .tree
            .selected
            .and_then(|index| self.tree.entries.get(index));
        let previous_path = previous.map(|entry| entry.path.clone());
        let previous_ino = previous.map(|entry| entry.ino).filter(|ino| *ino != 0);
        let previous_index = self.tree.selected;
        let expanded = self.expanded_tree_paths();
        let listings: BTreeMap<_, _> = listings.into_iter().collect();
        match listings.get(&self.workspace.path) {
            None => {
                self.status.text = "E: tree: missing root listing".to_owned();
                self.tree.stale = true;
                self.finish_tree_refresh();
                return;
            }
            Some(Err(error)) => {
                self.status.text = format!("E: tree: {error}");
                self.tree.stale = true;
                self.finish_tree_refresh();
                return;
            }
            Some(Ok(_)) => {}
        }
        let preserved = self
            .tree
            .entries
            .iter()
            .filter(|entry| self.secondary_root(&entry.path).is_some())
            .cloned()
            .collect::<Vec<_>>();
        let mut tree = Vec::new();
        let needs_listing =
            self.build_tree_entries(&self.workspace.path, 0, &listings, &expanded, &mut tree);
        tree.extend(preserved);
        let target = self.tree.pending_select.take().or(previous_path);
        self.tree.entries = tree;
        self.ensure_root_tree_nodes();
        self.tree.selected = Self::select_tree_path(
            &self.tree.entries,
            target.as_deref(),
            previous_index,
            previous_ino,
        );
        self.tree.stale = false;
        if needs_listing {
            self.tree.refresh_queued = true;
        }
        self.sync_tree_watches();
        self.request_git_status();
        self.finish_tree_refresh();
    }

    pub(super) fn finish_tree_refresh(&mut self) {
        if self.tree.refresh_queued {
            self.tree.refresh_queued = false;
            self.refresh_tree_background();
        }
    }

    pub(crate) fn refresh_tree(&mut self, select: Option<PathBuf>) {
        self.tree.visible = true;
        self.ui.focus.tree = true;
        self.refresh_wrap_width();
        let refresh_all = select.is_none();
        let secondary = select.as_deref().and_then(|path| self.secondary_root(path));
        if let Some(root) = secondary {
            self.tree.pending_select = select;
            self.refresh_root_tree(&root);
        } else {
            self.request_tree_refresh(select);
            if refresh_all {
                let roots = self
                    .workspace
                    .roots
                    .list()
                    .iter()
                    .filter(|root| root.trusted && root.path != self.workspace.path)
                    .map(|root| root.path.clone())
                    .collect::<Vec<_>>();
                for root in roots {
                    self.refresh_root_tree(&root);
                }
            }
        }
        self.request_git_status();
        self.file.dirty = true;
    }

    pub(super) fn tree_busy(&mut self) -> bool {
        if self.status.busy.is_some() {
            self.status.text = "E: file operation already in progress".to_owned();
            self.file.dirty = true;
            true
        } else {
            false
        }
    }
}

//! Workspace tree navigation and traversal.

use super::*;

impl Model {
    pub(crate) fn open_tree(&mut self) {
        if self.workspace.tree.visible && self.ui.focus.tree {
            self.ui.focus.tree = false;
            self.workspace.tree.visible = false;
            self.refresh_wrap_width();
            self.file.dirty = true;
            return;
        }
        if self.workspace.tree.entries.is_empty() {
            let workspace = self.workspace.path.clone();
            if self.ensure_file_worker() {
                if let Some(worker) = self.workspace.file_worker.as_ref() {
                    let _ = worker.request(FileRequest::List(workspace));
                }
                self.ensure_root_tree_nodes();
                let roots = self
                    .workspace
                    .root_file_workers
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>();
                for root in roots {
                    if let Some(worker) = self.workspace.root_file_workers.get(&root) {
                        let _ = worker.request(FileRequest::List(root));
                    }
                }
                self.status.text = "loading file tree".to_owned();
            } else {
                match self.tree_entries(&workspace) {
                    Ok(entries) => {
                        self.workspace.tree.entries = entries
                            .into_iter()
                            .filter(|entry| {
                                self.workspace.tree.entry_visible(&entry.name, entry.is_dir)
                            })
                            .map(|entry| TreeEntry {
                                path: workspace.join(entry.name),
                                depth: 0,
                                is_dir: entry.is_dir,
                                ino: entry.ino,
                                expanded: false,
                            })
                            .collect();
                    }
                    Err(error) => self.status.text = format!("E: tree: {error}"),
                }
                self.ensure_root_tree_nodes();
            }
        } else {
            self.ensure_root_tree_nodes();
        }
        self.workspace.tree.visible = true;
        self.ui.focus.tree = true;
        self.refresh_wrap_width();
        self.request_git_status();
        if self.workspace.tree.selected.is_none() {
            self.workspace.tree.selected = (!self.workspace.tree.entries.is_empty()).then_some(0);
        }
        self.sync_tree_watches();
        self.file.dirty = true;
    }

    pub(crate) fn tree_move(&mut self, delta: isize) {
        if self.workspace.tree.entries.is_empty() {
            return;
        }
        let current = self.workspace.tree.selected.unwrap_or(0);
        self.workspace.tree.selected = Some(
            current
                .saturating_add_signed(delta)
                .min(self.workspace.tree.entries.len().saturating_sub(1)),
        );
        self.file.dirty = true;
    }

    pub(crate) fn tree_toggle(&mut self) {
        let Some(index) = self.workspace.tree.selected else {
            return;
        };
        let Some(entry) = self.workspace.tree.entries.get(index).cloned() else {
            return;
        };
        if !entry.is_dir {
            match self.open_path(&entry.path) {
                Ok(()) => {
                    self.ui.focus.tree = false;
                    self.workspace.tree.visible = false;
                    self.refresh_wrap_width();
                }
                Err(error) => self.status.text = format!("E: {error}"),
            }
            self.file.dirty = true;
            return;
        }
        if entry.expanded {
            self.workspace.tree.entries[index].expanded = false;
            while self
                .workspace
                .tree
                .entries
                .get(index + 1)
                .is_some_and(|child| child.depth > entry.depth)
            {
                self.workspace.tree.entries.remove(index + 1);
            }
        } else {
            let owned = self.workspace.roots.owning_root(&entry.path);
            if let Some(root) = owned
                && !root.trusted
            {
                self.status.text = format!("E: untrusted workspace root: {}", root.path.display());
                self.file.dirty = true;
                return;
            }
            let has_worker =
                self.ensure_file_worker() && self.file_worker_for_path(&entry.path).is_some();
            if has_worker {
                self.workspace.tree.entries[index].expanded = true;
                if let Some(worker) = self.file_worker_for_path(&entry.path) {
                    let _ = worker.request(FileRequest::List(entry.path));
                }
                self.status.text = "loading directory".to_owned();
            } else {
                match self.tree_entries(&entry.path) {
                    Ok(children) => {
                        self.workspace.tree.entries[index].expanded = true;
                        let children: Vec<TreeEntry> = children
                            .into_iter()
                            .filter(|child| {
                                self.workspace.tree.entry_visible(&child.name, child.is_dir)
                            })
                            .map(|child| TreeEntry {
                                path: entry.path.join(child.name),
                                depth: entry.depth + 1,
                                is_dir: child.is_dir,
                                ino: child.ino,
                                expanded: false,
                            })
                            .collect();
                        self.workspace
                            .tree
                            .entries
                            .splice(index + 1..index + 1, children);
                    }
                    Err(error) => self.status.text = format!("E: tree: {error}"),
                }
            }
        }
        self.sync_tree_watches();
        self.file.dirty = true;
    }

    pub(crate) fn tree_collapse(&mut self) {
        let Some(index) = self.workspace.tree.selected else {
            return;
        };
        if self
            .workspace
            .tree
            .entries
            .get(index)
            .is_some_and(|entry| entry.expanded)
        {
            self.tree_toggle();
        } else if self
            .workspace
            .tree
            .entries
            .get(index)
            .is_some_and(|entry| entry.depth > 0)
        {
            while self.workspace.tree.selected.unwrap_or(0) > 0
                && self.workspace.tree.entries[self.workspace.tree.selected.unwrap_or(0)].depth > 0
            {
                self.tree_move(-1);
            }
            self.workspace.tree.selected = self
                .workspace
                .tree
                .selected
                .map(|index| index.saturating_sub(1));
            self.file.dirty = true;
        }
    }

    pub(crate) fn collect_files(
        &mut self,
        directory: &Path,
        prefix: &str,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        if out.len() >= 5000 {
            return Ok(());
        }
        let entries = self.tree_entries(directory)?;
        for entry in entries {
            if !self.workspace.tree.entry_visible(&entry.name, entry.is_dir) {
                continue;
            }
            let relative = if prefix.is_empty() {
                entry.name.clone()
            } else {
                format!("{prefix}/{}", entry.name)
            };
            if entry.is_dir {
                self.collect_files(&directory.join(&entry.name), &relative, out)?;
                if out.len() >= 5000 {
                    break;
                }
            } else {
                out.push(relative);
            }
        }
        Ok(())
    }
}

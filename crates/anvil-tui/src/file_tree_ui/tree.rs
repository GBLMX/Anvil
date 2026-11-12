//! Workspace tree navigation and traversal.

use super::*;

impl Model {
    pub(crate) fn open_tree(&mut self) {
        if self.tree_visible && self.ui.focus.tree {
            self.ui.focus.tree = false;
            self.tree_visible = false;
            self.dirty = true;
            return;
        }
        if self.tree.is_empty() {
            let workspace = self.workspace.clone();
            if self.ensure_file_worker() {
                if let Some(worker) = self.file_worker.as_ref() {
                    let _ = worker.request(FileRequest::List(workspace));
                }
                self.ensure_root_tree_nodes();
                let roots = self.root_file_workers.keys().cloned().collect::<Vec<_>>();
                for root in roots {
                    if let Some(worker) = self.root_file_workers.get(&root) {
                        let _ = worker.request(FileRequest::List(root));
                    }
                }
                self.status = "loading file tree".to_owned();
            } else {
                match self.tree_entries(&workspace) {
                    Ok(entries) => {
                        self.tree = entries
                            .into_iter()
                            .filter(|entry| self.tree_entry_visible(&entry.name, entry.is_dir))
                            .map(|entry| TreeEntry {
                                path: workspace.join(entry.name),
                                depth: 0,
                                is_dir: entry.is_dir,
                                ino: entry.ino,
                                expanded: false,
                            })
                            .collect();
                    }
                    Err(error) => self.status = format!("E: tree: {error}"),
                }
                self.ensure_root_tree_nodes();
            }
        } else {
            self.ensure_root_tree_nodes();
        }
        self.tree_visible = true;
        self.ui.focus.tree = true;
        self.request_git_status();
        if self.tree_selected.is_none() {
            self.tree_selected = (!self.tree.is_empty()).then_some(0);
        }
        self.sync_tree_watches();
        self.dirty = true;
    }

    pub(crate) fn tree_move(&mut self, delta: isize) {
        if self.tree.is_empty() {
            return;
        }
        let current = self.tree_selected.unwrap_or(0);
        self.tree_selected = Some(
            current
                .saturating_add_signed(delta)
                .min(self.tree.len().saturating_sub(1)),
        );
        self.dirty = true;
    }

    pub(crate) fn tree_toggle(&mut self) {
        let Some(index) = self.tree_selected else {
            return;
        };
        let Some(entry) = self.tree.get(index).cloned() else {
            return;
        };
        if !entry.is_dir {
            match self.open_path(&entry.path) {
                Ok(()) => {
                    self.ui.focus.tree = false;
                    self.tree_visible = false;
                }
                Err(error) => self.status = format!("E: {error}"),
            }
            self.dirty = true;
            return;
        }
        if entry.expanded {
            self.tree[index].expanded = false;
            while self
                .tree
                .get(index + 1)
                .is_some_and(|child| child.depth > entry.depth)
            {
                self.tree.remove(index + 1);
            }
        } else {
            let owned = self.workspace_roots.owning_root(&entry.path);
            if let Some(root) = owned
                && !root.trusted
            {
                self.status = format!("E: untrusted workspace root: {}", root.path.display());
                self.dirty = true;
                return;
            }
            let has_worker =
                self.ensure_file_worker() && self.file_worker_for_path(&entry.path).is_some();
            if has_worker {
                self.tree[index].expanded = true;
                if let Some(worker) = self.file_worker_for_path(&entry.path) {
                    let _ = worker.request(FileRequest::List(entry.path));
                }
                self.status = "loading directory".to_owned();
            } else {
                match self.tree_entries(&entry.path) {
                    Ok(children) => {
                        self.tree[index].expanded = true;
                        let children: Vec<TreeEntry> = children
                            .into_iter()
                            .filter(|child| self.tree_entry_visible(&child.name, child.is_dir))
                            .map(|child| TreeEntry {
                                path: entry.path.join(child.name),
                                depth: entry.depth + 1,
                                is_dir: child.is_dir,
                                ino: child.ino,
                                expanded: false,
                            })
                            .collect();
                        self.tree.splice(index + 1..index + 1, children);
                    }
                    Err(error) => self.status = format!("E: tree: {error}"),
                }
            }
        }
        self.sync_tree_watches();
        self.dirty = true;
    }

    pub(crate) fn tree_collapse(&mut self) {
        let Some(index) = self.tree_selected else {
            return;
        };
        if self.tree.get(index).is_some_and(|entry| entry.expanded) {
            self.tree_toggle();
        } else if self.tree.get(index).is_some_and(|entry| entry.depth > 0) {
            while self.tree_selected.unwrap_or(0) > 0
                && self.tree[self.tree_selected.unwrap_or(0)].depth > 0
            {
                self.tree_move(-1);
            }
            self.tree_selected = self.tree_selected.map(|index| index.saturating_sub(1));
            self.dirty = true;
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
            if !self.tree_entry_visible(&entry.name, entry.is_dir) {
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

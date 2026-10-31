//! Workspace tree and fuzzy file finder state.

use super::{FuzzyState, Model, TreeEntry};
use crate::file_worker::{FileEvent, FileRequest, FileWorker};
use anvil_ipc::DirEntry;
use std::{fs, path::Path};

impl Model {
    fn tree_entry_visible(&self, name: &str, is_dir: bool) -> bool {
        !is_dir
            || self.show_hidden
            || !name.starts_with('.')
            || matches!(name, ".github" | ".cargo")
    }

    fn ensure_file_worker(&mut self) -> bool {
        if self.file_worker.is_some() {
            return true;
        }
        let Some(session) = self.gateway_session.as_mut() else {
            return false;
        };
        let Ok(client) = session.connect_lsp() else {
            return false;
        };
        self.file_worker = Some(FileWorker::start(client, self.workspace.clone()));
        true
    }

    pub(crate) fn poll_file_worker(&mut self) {
        let events = self
            .file_worker
            .as_ref()
            .map(|worker| worker.poll())
            .unwrap_or_default();
        for event in events {
            match event {
                FileEvent::Listed(path, Ok(entries)) => {
                    let entries: Vec<DirEntry> = entries
                        .into_iter()
                        .filter(|entry| self.tree_entry_visible(&entry.name, entry.is_dir))
                        .collect();
                    if path == self.workspace {
                        self.tree = entries
                            .into_iter()
                            .map(|entry| TreeEntry {
                                path: path.join(entry.name),
                                depth: 0,
                                is_dir: entry.is_dir,
                                expanded: false,
                            })
                            .collect();
                        if self.tree_selected.is_none() && !self.tree.is_empty() {
                            self.tree_selected = Some(0);
                        }
                    } else if let Some(index) =
                        self.tree.iter().position(|entry| entry.path == path)
                    {
                        if self.tree[index].expanded {
                            let depth = self.tree[index].depth + 1;
                            let children: Vec<TreeEntry> = entries
                                .into_iter()
                                .map(|entry| TreeEntry {
                                    path: path.join(entry.name),
                                    depth,
                                    is_dir: entry.is_dir,
                                    expanded: false,
                                })
                                .collect();
                            self.tree.splice(index + 1..index + 1, children);
                        }
                    }
                    self.status = "file tree ready".to_owned();
                }
                FileEvent::Listed(_, Err(error)) => self.status = format!("E: tree: {error}"),
                FileEvent::Files(Ok(files)) => {
                    if let Some(fuzzy) = self.fuzzy.as_mut() {
                        fuzzy.entries = files;
                        self.fuzzy_refresh();
                    }
                    self.status = "files ready".to_owned();
                }
                FileEvent::Files(Err(error)) => self.status = format!("E: files: {error}"),
            }
            self.dirty = true;
        }
    }

    fn tree_entries(&mut self, path: &Path) -> Result<Vec<DirEntry>, String> {
        if let Some(gateway) = self.gateway.as_mut() {
            let relative = if path == self.workspace {
                ".".to_owned()
            } else {
                path.strip_prefix(&self.workspace)
                    .map_err(|_| format!("path is outside workspace {}", path.display()))?
                    .to_string_lossy()
                    .into_owned()
            };
            return gateway
                .read_dir(&relative)
                .map_err(|error| error.to_string());
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".git" || name == "target" {
                continue;
            }
            entries.push(DirEntry {
                is_dir: entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false),
                name,
            });
        }
        entries.sort_by(|left, right| {
            right
                .is_dir
                .cmp(&left.is_dir)
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(entries)
    }

    pub(crate) fn open_tree(&mut self) {
        if self.tree_visible && self.tree_focus {
            self.tree_focus = false;
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
                                expanded: false,
                            })
                            .collect();
                    }
                    Err(error) => self.status = format!("E: tree: {error}"),
                }
            }
        }
        self.tree_visible = true;
        self.tree_focus = true;
        if self.tree_selected.is_none() {
            self.tree_selected = (!self.tree.is_empty()).then_some(0);
        }
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
                    self.tree_focus = false;
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
        } else if self.file_worker.is_some() || self.ensure_file_worker() {
            self.tree[index].expanded = true;
            if let Some(worker) = self.file_worker.as_ref() {
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
                            expanded: false,
                        })
                        .collect();
                    self.tree.splice(index + 1..index + 1, children);
                }
                Err(error) => self.status = format!("E: tree: {error}"),
            }
        }
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

    fn collect_files(
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

    pub(crate) fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        if self.tree_visible {
            self.tree.clear();
            self.tree_selected = None;
            let workspace = self.workspace.clone();
            if self.ensure_file_worker() {
                if let Some(worker) = self.file_worker.as_ref() {
                    let _ = worker.request(FileRequest::List(workspace));
                }
            } else if let Ok(entries) = self.tree_entries(&workspace) {
                self.tree = entries
                    .into_iter()
                    .filter(|entry| self.tree_entry_visible(&entry.name, entry.is_dir))
                    .map(|entry| TreeEntry {
                        path: workspace.join(entry.name),
                        depth: 0,
                        is_dir: entry.is_dir,
                        expanded: false,
                    })
                    .collect();
            }
            self.tree_selected = (!self.tree.is_empty()).then_some(0);
        }
        self.status = if self.show_hidden {
            "hidden files shown".to_owned()
        } else {
            "hidden files hidden".to_owned()
        };
        self.dirty = true;
    }

    pub(crate) fn start_fuzzy(&mut self) {
        self.cancel_completion();
        self.pending_lsp.clear();
        self.hover = None;
        self.diagnostic_popup = None;
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
    }

    pub(crate) fn fuzzy_move(&mut self, delta: isize) {
        let Some(fuzzy) = self.fuzzy.as_mut() else {
            return;
        };
        fuzzy.selected = fuzzy
            .selected
            .saturating_add_signed(delta)
            .min(fuzzy.filtered.len().saturating_sub(1));
        self.dirty = true;
    }

    pub(crate) fn fuzzy_open(&mut self) {
        let Some(fuzzy) = self.fuzzy.take() else {
            return;
        };
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

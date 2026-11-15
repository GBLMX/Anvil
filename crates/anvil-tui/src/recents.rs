//! Recent workspace/file navigation over session persistence.

use super::*;

pub(crate) use anvil_session::recents::{Recents, load, push_recent, save};

impl Model {
    pub(crate) fn install_recents(&mut self, recents: Recents) {
        self.recent.workspaces = recents.workspaces;
        self.recent.files = recents.files;
    }

    pub(crate) fn record_recent_workspace(&mut self, workspace: &Path) {
        push_recent(&mut self.recent.workspaces, workspace);
        self.save_recents();
    }

    pub(crate) fn record_recent_file(&mut self, path: &Path) {
        push_recent(&mut self.recent.files, path);
        self.save_recents();
    }

    fn save_recents(&self) {
        if !self.recent.persist {
            return;
        }
        let recents = Recents::new(self.recent.workspaces.clone(), self.recent.files.clone());
        let _ = save(&recents);
    }

    pub(crate) fn openable_recent_files(&self) -> Vec<PathBuf> {
        self.recent
            .files
            .iter()
            .filter(|path| {
                self.workspace
                    .roots
                    .owning_root(path)
                    .is_some_and(|root| root.trusted)
            })
            .cloned()
            .collect()
    }

    pub(crate) fn open_recent_files(&mut self) {
        let files = self.openable_recent_files();
        if files.is_empty() {
            self.status.text = "no recent files in this workspace".to_owned();
            self.file.dirty = true;
            return;
        }
        self.clear_preview();
        self.overlays.fuzzy_generation = self.overlays.fuzzy_generation.saturating_add(1);
        let entries = files
            .iter()
            .map(|path| {
                let primary = self
                    .workspace
                    .roots
                    .owning_root(path)
                    .is_some_and(|root| root.path == self.workspace.path);
                if primary {
                    path.strip_prefix(&self.workspace.path)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .into_owned()
                } else {
                    path.to_string_lossy().into_owned()
                }
            })
            .collect::<Vec<_>>();
        self.overlays.fuzzy = Some(FuzzyState {
            generation: self.overlays.fuzzy_generation,
            query: String::new(),
            filtered: (0..entries.len()).collect(),
            entries,
            selected: 0,
        });
        self.push_overlay(OverlayKind::Fuzzy);
        self.status.text = "recent files".to_owned();
        self.request_preview();
        self.file.dirty = true;
    }

    pub(crate) fn show_recent_workspaces(&mut self) {
        if self.recent.workspaces.is_empty() {
            self.status.text = "no recent workspaces".to_owned();
        } else {
            self.status.text = format!(
                "recent: {}",
                self.recent
                    .workspaces
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        self.file.dirty = true;
    }
}

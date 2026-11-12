//! Persisted recent workspaces and files.

use super::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const RECENTS_VERSION: u32 = 1;
const MAX_RECENTS: usize = 20;

#[derive(Debug, Default, Deserialize, Serialize)]
pub(crate) struct Recents {
    #[serde(default)]
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) workspaces: Vec<PathBuf>,
    #[serde(default)]
    pub(crate) files: Vec<PathBuf>,
}

impl Recents {
    fn normalize(&mut self) {
        self.version = RECENTS_VERSION;
        self.workspaces.truncate(MAX_RECENTS);
        self.files.truncate(MAX_RECENTS);
    }
}

pub(crate) fn load() -> Recents {
    let Some(path) = state_path() else {
        return Recents::default();
    };
    load_from(&path)
}

fn load_from(path: &Path) -> Recents {
    let Ok(data) = fs::read(path) else {
        return Recents::default();
    };
    let Ok(mut recents) = serde_json::from_slice::<Recents>(&data) else {
        return Recents::default();
    };
    if recents.version != RECENTS_VERSION {
        return Recents::default();
    }
    recents.normalize();
    recents
}

fn save(recents: &Recents) -> Result<(), String> {
    let path = state_path().ok_or_else(|| "home directory unavailable".to_owned())?;
    save_to(&path, recents)
}

fn save_to(path: &Path, recents: &Recents) -> Result<(), String> {
    let directory = path
        .parent()
        .ok_or_else(|| "recent path has no parent".to_owned())?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let data = serde_json::to_vec(recents).map_err(|error| error.to_string())?;
    let temp = directory.join("recent.json.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|error| error.to_string())?;
    file.write_all(&data).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    fs::rename(&temp, path).map_err(|error| error.to_string())
}

fn state_path() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    Some(root.join("anvil/recent.json"))
}

impl Model {
    pub(crate) fn install_recents(&mut self, recents: Recents) {
        self.recent_workspaces = recents.workspaces;
        self.recent_files = recents.files;
    }

    pub(crate) fn record_recent_workspace(&mut self, workspace: &Path) {
        push_recent(&mut self.recent_workspaces, workspace);
        self.save_recents();
    }

    pub(crate) fn record_recent_file(&mut self, path: &Path) {
        push_recent(&mut self.recent_files, path);
        self.save_recents();
    }

    fn save_recents(&self) {
        if !self.persist_recent {
            return;
        }
        let recents = Recents {
            version: RECENTS_VERSION,
            workspaces: self.recent_workspaces.clone(),
            files: self.recent_files.clone(),
        };
        let _ = save(&recents);
    }

    pub(crate) fn openable_recent_files(&self) -> Vec<PathBuf> {
        self.recent_files
            .iter()
            .filter(|path| {
                self.workspace_roots
                    .owning_root(path)
                    .is_some_and(|root| root.trusted)
            })
            .cloned()
            .collect()
    }

    pub(crate) fn open_recent_files(&mut self) {
        let files = self.openable_recent_files();
        if files.is_empty() {
            self.status = "no recent files in this workspace".to_owned();
            self.dirty = true;
            return;
        }
        self.clear_preview();
        self.fuzzy_generation = self.fuzzy_generation.saturating_add(1);
        let entries = files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        self.fuzzy = Some(FuzzyState {
            generation: self.fuzzy_generation,
            query: String::new(),
            filtered: (0..entries.len()).collect(),
            entries,
            selected: 0,
        });
        self.status = "recent files".to_owned();
        self.request_preview();
        self.dirty = true;
    }

    pub(crate) fn show_recent_workspaces(&mut self) {
        if self.recent_workspaces.is_empty() {
            self.status = "no recent workspaces".to_owned();
        } else {
            self.status = format!(
                "recent: {}",
                self.recent_workspaces
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        self.dirty = true;
    }
}

fn push_recent(list: &mut Vec<PathBuf>, path: &Path) {
    list.retain(|entry| entry != path);
    list.insert(0, path.to_path_buf());
    list.truncate(MAX_RECENTS);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_recents_and_rejects_old_versions() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("recent.json");
        let recents = Recents {
            version: RECENTS_VERSION,
            workspaces: vec![PathBuf::from("/ws")],
            files: vec![PathBuf::from("/ws/main.rs")],
        };
        save_to(&path, &recents).expect("save");
        let loaded = load_from(&path);
        assert_eq!(loaded.workspaces, recents.workspaces);
        assert_eq!(loaded.files, recents.files);
        fs::write(&path, br#"{"version":0,"workspaces":["/old"],"files":[]}"#).expect("write old");
        assert!(load_from(&path).workspaces.is_empty());
    }
}

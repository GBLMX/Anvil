//! Persisted recent workspaces and files.

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const RECENTS_VERSION: u32 = 1;
const MAX_RECENTS: usize = 20;

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Recents {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub workspaces: Vec<PathBuf>,
    #[serde(default)]
    pub files: Vec<PathBuf>,
}

impl Recents {
    /// Build a normalized snapshot for persistence.
    pub fn new(workspaces: Vec<PathBuf>, files: Vec<PathBuf>) -> Self {
        let mut recents = Self {
            version: RECENTS_VERSION,
            workspaces,
            files,
        };
        recents.normalize();
        recents
    }

    fn normalize(&mut self) {
        self.version = RECENTS_VERSION;
        self.workspaces.truncate(MAX_RECENTS);
        self.files.truncate(MAX_RECENTS);
    }
}

/// Load the persisted recent lists, or defaults when absent/invalid.
pub fn load() -> Recents {
    let Some(path) = state_path() else {
        return Recents::default();
    };
    load_from(&path)
}

/// Save recent lists to the state directory.
pub fn save(recents: &Recents) -> Result<(), String> {
    let path = state_path().ok_or_else(|| "home directory unavailable".to_owned())?;
    save_to(&path, recents)
}

/// Move a path to the front of a recent list and enforce the cap.
pub fn push_recent(list: &mut Vec<PathBuf>, path: &Path) {
    list.retain(|entry| entry != path);
    list.insert(0, path.to_path_buf());
    list.truncate(MAX_RECENTS);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_recents_and_rejects_old_versions() {
        let directory = std::env::temp_dir().join(format!(
            "anvil-recents-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or_default()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("tempdir");
        let path = directory.join("recent.json");
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
        let _ = fs::remove_dir_all(&directory);
    }
}

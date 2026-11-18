//! Recent workspace/file navigation and Ex command history over session persistence.

use super::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

pub(crate) use anvil_session::recents::{Recents, load};

const COMMAND_HISTORY_VERSION: u32 = 1;
const MAX_COMMANDS: usize = 50;

#[derive(Debug, Default, Deserialize, Serialize)]
struct CommandHistoryFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    commands: Vec<String>,
}

pub(crate) fn push_command(commands: &mut Vec<String>, command: &str) {
    commands.retain(|entry| entry != command);
    commands.insert(0, command.to_owned());
    commands.truncate(MAX_COMMANDS);
}

fn command_history_path() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    Some(root.join("anvil/command-history.json"))
}

fn load_command_history() -> Vec<String> {
    command_history_path()
        .map(|path| load_command_history_from(&path))
        .unwrap_or_default()
}

fn load_command_history_from(path: &Path) -> Vec<String> {
    let Ok(data) = fs::read(path) else {
        return Vec::new();
    };
    let Ok(mut history) = serde_json::from_slice::<CommandHistoryFile>(&data) else {
        return Vec::new();
    };
    if history.version != COMMAND_HISTORY_VERSION {
        return Vec::new();
    }
    history.commands.truncate(MAX_COMMANDS);
    history.commands
}

fn save_command_history(commands: &[String]) -> Result<(), String> {
    let path = command_history_path().ok_or_else(|| "home directory unavailable".to_owned())?;
    save_command_history_to(&path, commands)
}

fn save_command_history_to(path: &Path, commands: &[String]) -> Result<(), String> {
    let directory = path
        .parent()
        .ok_or_else(|| "command history path has no parent".to_owned())?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let data = serde_json::to_vec(&CommandHistoryFile {
        version: COMMAND_HISTORY_VERSION,
        commands: commands.to_vec(),
    })
    .map_err(|error| error.to_string())?;
    let temp = path.with_extension(format!("tmp.{}", std::process::id()));
    let result = (|| {
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
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

impl Model {
    pub(crate) fn install_recents(&mut self, recents: Recents) {
        self.workspace.recent.install(recents);
        self.workspace.recent.commands = load_command_history();
    }

    pub(crate) fn record_recent_workspace(&mut self, workspace: &Path) {
        self.workspace.recent.record_workspace(workspace);
        self.workspace.recent.save();
    }

    pub(crate) fn record_recent_file(&mut self, path: &Path) {
        self.workspace.recent.record_file(path);
        self.workspace.recent.save();
    }

    pub(crate) fn record_ex_command(&mut self, command: &str) {
        let command = command.trim();
        if command.is_empty() {
            return;
        }
        self.workspace.recent.record_command(command);
        if self.workspace.recent.persist {
            let _ = save_command_history(&self.workspace.recent.commands);
        }
    }

    pub(crate) fn openable_recent_files(&self) -> Vec<PathBuf> {
        self.workspace.recent.openable_files(&self.workspace.roots)
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
        if self.workspace.recent.workspaces.is_empty() {
            self.status.text = "no recent workspaces".to_owned();
        } else {
            self.status.text = format!(
                "recent: {}",
                self.workspace
                    .recent
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_history_round_trips_and_bounds() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("command-history.json");
        let commands = vec!["theme anvil-dark".to_owned(), "w".to_owned()];
        save_command_history_to(&path, &commands).expect("save");
        assert_eq!(load_command_history_from(&path), commands);

        let mut bounded = Vec::new();
        for index in 0..MAX_COMMANDS + 5 {
            push_command(&mut bounded, &format!("command.{index}"));
        }
        assert_eq!(bounded.len(), MAX_COMMANDS);
        assert_eq!(bounded.first().map(String::as_str), Some("command.54"));

        fs::write(&path, br#"{"version":0,"commands":["old"]}"#).expect("old history");
        assert!(load_command_history_from(&path).is_empty());
    }
}

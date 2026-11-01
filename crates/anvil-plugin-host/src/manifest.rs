//! Bounded, non-executing plugin manifest validation.

use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_EVENTS: usize = 64;
const MAX_COMMANDS: usize = 128;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    pub id: String,
    pub version: String,
    pub entry: String,
    #[serde(default)]
    pub activation_events: Vec<String>,
    #[serde(default)]
    pub commands: Vec<PluginCommand>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PluginCommand {
    pub id: String,
    pub title: String,
}

impl PluginManifest {
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join("plugin.json");
        let metadata = std::fs::metadata(&path)
            .map_err(|error| format!("manifest {}: {error}", path.display()))?;
        if metadata.len() > MAX_MANIFEST_BYTES {
            return Err("manifest too large".to_owned());
        }
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("manifest {}: {error}", path.display()))?;
        let manifest: Self =
            serde_json::from_slice(&bytes).map_err(|error| format!("manifest: {error}"))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn entry_path(&self, root: &Path) -> Result<PathBuf, String> {
        let path = root.join(&self.entry);
        if !path.is_file() {
            return Err(format!("plugin entry not found: {}", path.display()));
        }
        Ok(path)
    }

    fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.len() > 128
            || !self
                .id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
        {
            return Err("invalid plugin id".to_owned());
        }
        if self.version.is_empty() || self.version.len() > 64 {
            return Err("invalid plugin version".to_owned());
        }
        let entry = Path::new(&self.entry);
        if entry.as_os_str().is_empty()
            || entry.is_absolute()
            || entry
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err("invalid plugin entry".to_owned());
        }
        if self.activation_events.len() > MAX_EVENTS
            || self
                .activation_events
                .iter()
                .any(|event| !valid_event(event))
        {
            return Err("invalid activation events".to_owned());
        }
        if self.commands.len() > MAX_COMMANDS {
            return Err("too many plugin commands".to_owned());
        }
        let mut command_ids = std::collections::BTreeSet::new();
        for command in &self.commands {
            if command.id.is_empty()
                || command.id.len() > 128
                || command.title.len() > 256
                || !command_ids.insert(command.id.as_str())
            {
                return Err("invalid plugin commands".to_owned());
            }
        }
        Ok(())
    }
}

fn valid_event(event: &str) -> bool {
    event == "onStartup"
        || ["onCommand:", "onLanguage:", "onWorkspaceContains:"]
            .iter()
            .any(|prefix| {
                event
                    .strip_prefix(prefix)
                    .is_some_and(|value| !value.is_empty())
            })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(entry: &str) -> PluginManifest {
        PluginManifest {
            id: "demo".to_owned(),
            version: "0.1.0".to_owned(),
            entry: entry.to_owned(),
            activation_events: vec!["onStartup".to_owned()],
            commands: vec![PluginCommand {
                id: "demo.run".to_owned(),
                title: "Run".to_owned(),
            }],
        }
    }

    #[test]
    fn rejects_escaping_entry_and_unknown_event() {
        assert!(manifest("../run.luau").validate().is_err());
        assert!(manifest("/run.luau").validate().is_err());
        let mut invalid = manifest("run.luau");
        invalid.activation_events = vec!["onUnknown".to_owned()];
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_commands() {
        let mut duplicate = manifest("run.luau");
        duplicate.commands.push(duplicate.commands[0].clone());
        assert!(duplicate.validate().is_err());
    }

    #[test]
    fn accepts_bounded_manifest() {
        assert!(manifest("run.luau").validate().is_ok());
    }
}

//! Bounded discovery of local plugin manifests.

use anvil_plugin_host::manifest::PluginManifest;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_PLUGINS: usize = 128;

#[derive(Clone, Debug)]
pub(crate) struct PluginDescriptor {
    pub(crate) root: PathBuf,
    pub(crate) manifest: PluginManifest,
}

pub(crate) fn discover() -> Result<Vec<PluginDescriptor>, String> {
    let Some(root) = std::env::var_os("ANVIL_PLUGIN_DIR") else {
        return Ok(Vec::new());
    };
    discover_from(Path::new(&root))
}

pub(crate) fn discover_from(root: &Path) -> Result<Vec<PluginDescriptor>, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("plugin directory {}: {error}", root.display()))?;
    if !root.is_dir() {
        return Err(format!(
            "plugin directory is not a directory: {}",
            root.display()
        ));
    }
    let mut entries = fs::read_dir(&root)
        .map_err(|error| format!("plugin directory {}: {error}", root.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("plugin directory {}: {error}", root.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut plugins = Vec::new();
    let mut ids = BTreeSet::new();
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("plugin entry {}: {error}", entry.path().display()))?;
        if !file_type.is_dir() {
            continue;
        }
        let plugin_root = entry.path();
        if !plugin_root.join("plugin.json").is_file() {
            continue;
        }
        if plugins.len() >= MAX_PLUGINS {
            return Err(format!("too many plugins; limit is {MAX_PLUGINS}"));
        }
        let manifest = PluginManifest::load(&plugin_root)?;
        if !ids.insert(manifest.id.clone()) {
            return Err(format!("duplicate plugin id: {}", manifest.id));
        }
        plugins.push(PluginDescriptor {
            root: plugin_root,
            manifest,
        });
    }
    Ok(plugins)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_valid_plugin_directories() {
        let directory = tempfile::tempdir().expect("tempdir");
        let plugin = directory.path().join("demo");
        fs::create_dir(&plugin).expect("plugin dir");
        fs::write(
            plugin.join("plugin.json"),
            r#"{"id":"demo","version":"0.1.0","entry":"run.luau","activationEvents":["onStartup"]}"#,
        )
        .expect("manifest");
        let plugins = discover_from(directory.path()).expect("discover");
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].manifest.id, "demo");
        assert_eq!(
            plugins[0].manifest.activation_events,
            vec!["onStartup".to_owned()]
        );
    }

    #[test]
    fn rejects_duplicate_plugin_ids() {
        let directory = tempfile::tempdir().expect("tempdir");
        for name in ["a", "b"] {
            let plugin = directory.path().join(name);
            fs::create_dir(&plugin).expect("plugin dir");
            fs::write(
                plugin.join("plugin.json"),
                r#"{"id":"same","version":"0.1.0","entry":"run.luau"}"#,
            )
            .expect("manifest");
        }
        let error = discover_from(directory.path()).expect_err("duplicate");
        assert!(error.contains("duplicate plugin id"), "{error}");
    }

    #[test]
    fn skips_directories_without_manifests() {
        let directory = tempfile::tempdir().expect("tempdir");
        fs::create_dir(directory.path().join("not-a-plugin")).expect("dir");
        assert!(
            discover_from(directory.path())
                .expect("discover")
                .is_empty()
        );
    }
}

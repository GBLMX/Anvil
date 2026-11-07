//! Bounded discovery of bundled, local, and managed plugin manifests.

use anvil_plugin_host::manifest::{MANIFEST_FILE, PluginManifest};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_PLUGINS: usize = 128;
const OFFICIAL_SOURCE: &str = "official";

#[derive(Clone, Debug)]
pub(crate) struct PluginDescriptor {
    pub(crate) root: PathBuf,
    pub(crate) manifest: PluginManifest,
    pub(crate) enabled: bool,
    pub(crate) digest: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) previous_digest: Option<String>,
}

pub(crate) fn discover() -> Result<Vec<PluginDescriptor>, String> {
    let Some(root) = std::env::var_os("ANVIL_PLUGIN_DIR") else {
        return Ok(Vec::new());
    };
    discover_from(Path::new(&root))
}

/// Locate the release-provided plugin root.
///
/// The environment override exists for tests and development. Release
/// layouts keep official plugins in `../share/anvil/official/plugins` next to
/// `bin/`; the old `../share/anvil/plugins` path remains a fallback, and
/// portable layouts may put them in `plugins/` beside the executable.
pub(crate) fn bundled_root() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("ANVIL_BUNDLED_PLUGIN_DIR")
        && !root.is_empty()
    {
        return Some(PathBuf::from(root));
    }
    let executable = std::env::current_exe().ok()?;
    let directory = executable.parent()?;
    for relative in ["../share/anvil/official/plugins", "../share/anvil/plugins"] {
        let share = directory.join(relative);
        if share.is_dir() {
            return Some(share);
        }
    }
    let portable = directory.join("plugins");
    portable.is_dir().then_some(portable)
}

pub(crate) fn discover_bundled() -> Result<Vec<PluginDescriptor>, String> {
    match bundled_root() {
        Some(root) => discover_official_from(&root),
        None => Ok(Vec::new()),
    }
}

pub(crate) fn discover_official_from(root: &Path) -> Result<Vec<PluginDescriptor>, String> {
    let mut plugins = discover_from(root)?;
    for plugin in &mut plugins {
        plugin.enabled = true;
        plugin.source = Some(OFFICIAL_SOURCE.to_owned());
    }
    Ok(plugins)
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
        if !plugin_root.join(MANIFEST_FILE).is_file() {
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
            enabled: true,
            digest: None,
            source: None,
            previous_digest: None,
        });
    }
    Ok(plugins)
}

/// Merge bundled and local plugins. A local directory may not shadow an
/// official plugin with the same ID.
pub(crate) fn merge_local(
    plugins: &mut Vec<PluginDescriptor>,
    local: Vec<PluginDescriptor>,
) -> Vec<String> {
    let mut ids = plugins
        .iter()
        .map(|plugin| plugin.manifest.id.clone())
        .collect::<BTreeSet<_>>();
    let mut conflicts = Vec::new();
    for plugin in local {
        let id = plugin.manifest.id.clone();
        if !ids.insert(id.clone()) {
            conflicts.push(id);
            continue;
        }
        plugins.push(plugin);
    }
    conflicts
}

pub(crate) fn discover_managed(value: &serde_json::Value) -> Result<Vec<PluginDescriptor>, String> {
    let Some(entries) = value.get("plugins").and_then(|value| value.as_array()) else {
        return Err("pkg.installed result has no plugins array".to_owned());
    };
    if entries.len() > MAX_PLUGINS {
        return Err(format!(
            "too many installed plugins; limit is {MAX_PLUGINS}"
        ));
    }
    let mut plugins = Vec::with_capacity(entries.len());
    for entry in entries {
        let id = entry
            .get("id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "installed plugin has no id".to_owned())?;
        let root = entry
            .get("root")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("installed plugin {id} has no root"))?;
        let root = PathBuf::from(root);
        let manifest = PluginManifest::load(&root)?;
        if manifest.id != id {
            return Err(format!(
                "installed plugin id mismatch: state {id}, manifest {}",
                manifest.id
            ));
        }
        let enabled = entry
            .get("enabled")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let digest = entry
            .get("digest")
            .and_then(|value| value.as_str())
            .map(str::to_owned);
        let source = entry
            .get("source")
            .and_then(|value| value.as_str())
            .map(str::to_owned);
        let previous_digest = entry
            .get("previous_digest")
            .and_then(|value| value.as_str())
            .map(str::to_owned);
        plugins.push(PluginDescriptor {
            root,
            manifest,
            enabled,
            digest,
            source,
            previous_digest,
        });
    }
    Ok(plugins)
}

pub(crate) fn merge_managed(
    plugins: &mut Vec<PluginDescriptor>,
    managed: Vec<PluginDescriptor>,
) -> Vec<String> {
    let mut conflicts = Vec::new();
    let mut ids = BTreeSet::new();
    for plugin in managed {
        let id = plugin.manifest.id.clone();
        if !ids.insert(id.clone()) {
            conflicts.push(id);
            continue;
        }
        if let Some(index) = plugins
            .iter()
            .position(|existing| existing.manifest.id == plugin.manifest.id)
        {
            let existing = &plugins[index];
            let bundled =
                existing.digest.is_none() && existing.source.as_deref() == Some(OFFICIAL_SOURCE);
            plugins.remove(index);
            if !bundled {
                conflicts.push(id.clone());
            }
        }
        plugins.push(plugin);
    }
    conflicts
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

    #[test]
    fn discovers_managed_plugin_roots() {
        let directory = tempfile::tempdir().expect("tempdir");
        let plugin = directory.path().join("demo");
        fs::create_dir(&plugin).expect("plugin dir");
        fs::write(
            plugin.join("plugin.json"),
            r#"{"id":"demo","version":"1.0.0","entry":"run.luau"}"#,
        )
        .expect("manifest");
        let plugins = discover_managed(&serde_json::json!({
            "plugins": [{"id": "demo", "root": plugin}]
        }))
        .expect("managed");
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].manifest.id, "demo");
    }

    #[test]
    fn reports_managed_local_conflicts_and_keeps_managed() {
        fn descriptor(id: &str, root: &str) -> PluginDescriptor {
            PluginDescriptor {
                root: PathBuf::from(root),
                manifest: PluginManifest {
                    id: id.to_owned(),
                    version: "1.0.0".to_owned(),
                    entry: "main.luau".to_owned(),
                    api: "1.0".to_owned(),
                    activation_events: Vec::new(),
                    commands: Vec::new(),
                },
                enabled: true,
                digest: None,
                source: None,
                previous_digest: None,
            }
        }
        let mut local = vec![descriptor("demo", "/local")];
        let conflicts = merge_managed(&mut local, vec![descriptor("demo", "/store")]);
        assert_eq!(conflicts, ["demo"]);
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].root, PathBuf::from("/store"));
    }

    fn descriptor_with_source(
        id: &str,
        root: &str,
        source: Option<&str>,
        digest: Option<&str>,
    ) -> PluginDescriptor {
        PluginDescriptor {
            root: PathBuf::from(root),
            manifest: PluginManifest {
                id: id.to_owned(),
                version: "1.0.0".to_owned(),
                entry: "main.luau".to_owned(),
                api: "1.0".to_owned(),
                activation_events: Vec::new(),
                commands: Vec::new(),
            },
            enabled: true,
            digest: digest.map(str::to_owned),
            source: source.map(str::to_owned),
            previous_digest: None,
        }
    }

    #[test]
    fn discovers_bundled_plugins_as_official() {
        let directory = tempfile::tempdir().expect("tempdir");
        let plugin = directory.path().join("demo");
        fs::create_dir(&plugin).expect("plugin dir");
        fs::write(
            plugin.join("plugin.json"),
            r#"{"id":"demo","version":"1.0.0","entry":"run.luau"}"#,
        )
        .expect("manifest");
        let plugins = discover_official_from(directory.path()).expect("official");
        assert_eq!(plugins.len(), 1);
        assert!(plugins[0].enabled);
        assert_eq!(plugins[0].source.as_deref(), Some(OFFICIAL_SOURCE));
    }

    #[test]
    fn local_directory_cannot_shadow_bundled_plugin() {
        let mut plugins = vec![descriptor_with_source(
            "demo",
            "/bundled",
            Some(OFFICIAL_SOURCE),
            None,
        )];
        let local = vec![descriptor_with_source("demo", "/local", None, None)];
        let conflicts = merge_local(&mut plugins, local);
        assert_eq!(conflicts, ["demo"]);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].root, PathBuf::from("/bundled"));
    }

    #[test]
    fn managed_package_replaces_bundled_plugin_without_conflict() {
        let mut plugins = vec![descriptor_with_source(
            "demo",
            "/bundled",
            Some(OFFICIAL_SOURCE),
            None,
        )];
        let managed = vec![descriptor_with_source(
            "demo",
            "/store",
            Some(OFFICIAL_SOURCE),
            Some("digest"),
        )];
        let conflicts = merge_managed(&mut plugins, managed);
        assert!(conflicts.is_empty());
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].root, PathBuf::from("/store"));
    }
}

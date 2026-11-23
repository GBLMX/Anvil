//! UI-neutral project description and read-only Cargo metadata adapter.

use serde_json::Value;
use std::collections::BTreeSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A project root and the targets discovered under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub roots: Vec<PathBuf>,
    pub targets: Vec<ProjectTarget>,
    pub run_configurations: Vec<RunConfiguration>,
    pub test_configurations: Vec<TestConfiguration>,
}

/// One build target exposed by a project adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTarget {
    pub id: String,
    pub name: String,
    pub kind: TargetKind,
    pub source_roots: Vec<PathBuf>,
    pub dependencies: Vec<String>,
    pub capabilities: Capabilities,
}

/// Target kind normalized across project adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetKind {
    Library,
    Binary,
    Test,
    Bench,
    Example,
    Custom(String),
}

/// Operations a target can support.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub can_build: bool,
    pub can_test: bool,
    pub can_run: bool,
    pub can_debug: bool,
}

/// A named run configuration, once an adapter provides one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunConfiguration {
    pub id: String,
    pub name: String,
    pub target: Option<String>,
}

/// A named test configuration, once an adapter provides one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestConfiguration {
    pub id: String,
    pub name: String,
    pub target: Option<String>,
}

impl Project {
    /// Map `cargo metadata --no-deps --format-version 1` output.
    pub fn from_cargo_metadata(metadata: &Value) -> Result<Self, String> {
        let root = metadata
            .get("workspace_root")
            .and_then(Value::as_str)
            .ok_or_else(|| "cargo metadata: missing workspace_root".to_owned())?;
        let packages = metadata
            .get("packages")
            .and_then(Value::as_array)
            .ok_or_else(|| "cargo metadata: missing packages".to_owned())?;
        let mut targets = Vec::new();
        for package in packages {
            let package_name = package
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "cargo metadata: package without name".to_owned())?;
            let package_root = package
                .get("manifest_path")
                .and_then(Value::as_str)
                .map(Path::new)
                .and_then(Path::parent)
                .ok_or_else(|| format!("cargo metadata: {package_name} without manifest path"))?;
            let dependencies = package
                .get("dependencies")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|dependency| dependency.get("name").and_then(Value::as_str))
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let package_targets = package
                .get("targets")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("cargo metadata: {package_name} without targets"))?;
            for target in package_targets {
                let Some(kind) = target
                    .get("kind")
                    .and_then(Value::as_array)
                    .and_then(|kinds| kinds.first())
                    .and_then(Value::as_str)
                    .and_then(TargetKind::from_cargo)
                else {
                    continue;
                };
                let name = target
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("cargo metadata: {package_name} target without name"))?;
                let src_path = target
                    .get("src_path")
                    .and_then(Value::as_str)
                    .map(PathBuf::from);
                let mut source_roots = BTreeSet::from([package_root.to_path_buf()]);
                if let Some(parent) = src_path.as_deref().and_then(Path::parent) {
                    source_roots.insert(parent.to_path_buf());
                }
                targets.push(ProjectTarget {
                    id: format!("{package_name}:{}:{name}", kind.as_str()),
                    name: name.to_owned(),
                    capabilities: Capabilities {
                        can_build: true,
                        can_test: true,
                        can_run: matches!(kind, TargetKind::Binary | TargetKind::Example),
                        can_debug: false,
                    },
                    kind,
                    source_roots: source_roots.into_iter().collect(),
                    dependencies: dependencies.clone(),
                });
            }
        }
        targets.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(Self {
            roots: vec![PathBuf::from(root)],
            targets,
            run_configurations: Vec::new(),
            test_configurations: Vec::new(),
        })
    }

    /// Discover a project from a directory or manifest path using Cargo.
    pub fn cargo(start: &Path) -> Result<Self, String> {
        CargoAdapter::default().load(start)
    }
}

impl TargetKind {
    fn from_cargo(kind: &str) -> Option<Self> {
        match kind {
            "lib" => Some(Self::Library),
            "bin" => Some(Self::Binary),
            "test" => Some(Self::Test),
            "bench" => Some(Self::Bench),
            "example" => Some(Self::Example),
            "custom-build" => None,
            other => Some(Self::Custom(other.to_owned())),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Library => "lib",
            Self::Binary => "bin",
            Self::Test => "test",
            Self::Bench => "bench",
            Self::Example => "example",
            Self::Custom(kind) => kind,
        }
    }
}

/// Read-only adapter for Cargo workspaces.
#[derive(Debug, Clone)]
pub struct CargoAdapter {
    cargo: PathBuf,
}

impl Default for CargoAdapter {
    fn default() -> Self {
        Self::new(
            env::var_os("CARGO")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("cargo")),
        )
    }
}

impl CargoAdapter {
    pub fn new(cargo: impl Into<PathBuf>) -> Self {
        Self {
            cargo: cargo.into(),
        }
    }

    /// Discover the nearest `Cargo.toml` and load its workspace metadata.
    pub fn load(&self, start: &Path) -> Result<Project, String> {
        let manifest = find_cargo_manifest(start)?;
        self.load_manifest(&manifest)
    }

    /// Load one exact Cargo manifest.
    pub fn load_manifest(&self, manifest: &Path) -> Result<Project, String> {
        let output = Command::new(&self.cargo)
            .arg("metadata")
            .arg("--no-deps")
            .arg("--format-version")
            .arg("1")
            .arg("--manifest-path")
            .arg(manifest)
            .output()
            .map_err(|error| format!("cargo metadata: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.chars().take(4096).collect::<String>();
            return Err(format!("cargo metadata failed: {stderr}"));
        }
        let metadata: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("cargo metadata: {error}"))?;
        Project::from_cargo_metadata(&metadata)
    }
}

/// Walk upward from a file or directory to the nearest `Cargo.toml`.
pub fn find_cargo_manifest(start: &Path) -> Result<PathBuf, String> {
    let mut current = if start.is_file() {
        start.parent().unwrap_or(start)
    } else {
        start
    };
    loop {
        let candidate = current.join("Cargo.toml");
        if candidate.is_file() {
            return Ok(candidate);
        }
        let Some(parent) = current.parent() else {
            return Err(format!("Cargo.toml not found from {}", start.display()));
        };
        current = parent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn metadata() -> Value {
        json!({
            "workspace_root": "/work",
            "packages": [{
                "name": "demo",
                "manifest_path": "/work/demo/Cargo.toml",
                "dependencies": [{"name": "serde"}, {"name": "serde"}],
                "targets": [
                    {"name": "demo", "kind": ["lib"], "src_path": "/work/demo/src/lib.rs"},
                    {"name": "demo-cli", "kind": ["bin"], "src_path": "/work/demo/src/main.rs"},
                    {"name": "build-script-build", "kind": ["custom-build"], "src_path": "/work/demo/build.rs"}
                ]
            }]
        })
    }

    #[test]
    fn maps_cargo_targets_and_capabilities() {
        let project = Project::from_cargo_metadata(&metadata()).expect("metadata");
        assert_eq!(project.roots, [PathBuf::from("/work")]);
        assert_eq!(project.targets.len(), 2);
        let library = project
            .targets
            .iter()
            .find(|target| target.kind == TargetKind::Library)
            .expect("library");
        assert_eq!(library.id, "demo:lib:demo");
        assert!(!library.capabilities.can_run);
        assert_eq!(
            library.source_roots,
            [PathBuf::from("/work/demo"), PathBuf::from("/work/demo/src")]
        );
        assert_eq!(library.dependencies, ["serde", "serde"]);
        let binary = project
            .targets
            .iter()
            .find(|target| target.kind == TargetKind::Binary)
            .expect("binary");
        assert!(binary.capabilities.can_run);
    }

    #[test]
    fn rejects_metadata_without_workspace_root() {
        let error =
            Project::from_cargo_metadata(&json!({"packages": []})).expect_err("missing root");
        assert!(error.contains("workspace_root"));
    }

    #[test]
    fn finds_manifest_in_ancestor() {
        let root = std::env::temp_dir().join(format!(
            "anvil-project-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let nested = root.join("a/b");
        std::fs::create_dir_all(&nested).expect("mkdir");
        std::fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("manifest");
        let found = find_cargo_manifest(&nested).expect("find");
        assert_eq!(found, root.join("Cargo.toml"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn discovers_current_workspace() {
        let project = CargoAdapter::default()
            .load(Path::new(env!("CARGO_MANIFEST_DIR")))
            .expect("cargo metadata");
        assert!(project.roots.iter().any(|root| root.ends_with("Anvil")));
        assert!(
            project
                .targets
                .iter()
                .any(|target| target.name == "anvil_project")
        );
    }
}

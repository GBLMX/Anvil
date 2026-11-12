//! Multi-root workspace path ownership and trust labels.
//!
//! This is the routing core. Per-root gateway sessions are wired separately.

use std::{
    fs,
    path::{Path, PathBuf},
};

pub const MAX_WORKSPACE_ROOTS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRoot {
    pub path: PathBuf,
    pub trusted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootError {
    Invalid(String),
    Duplicate(PathBuf),
    Limit,
    OutsideRoots(PathBuf),
    Untrusted(PathBuf),
    Missing(PathBuf),
}

impl std::fmt::Display for RootError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "{message}"),
            Self::Duplicate(path) => write!(
                formatter,
                "workspace root already added: {}",
                path.display()
            ),
            Self::Limit => write!(formatter, "workspace root limit is {MAX_WORKSPACE_ROOTS}"),
            Self::OutsideRoots(path) => {
                write!(
                    formatter,
                    "path is outside workspace roots: {}",
                    path.display()
                )
            }
            Self::Untrusted(path) => {
                write!(formatter, "workspace root is untrusted: {}", path.display())
            }
            Self::Missing(path) => {
                write!(formatter, "workspace root is missing: {}", path.display())
            }
        }
    }
}

impl std::error::Error for RootError {}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceRoots {
    roots: Vec<WorkspaceRoot>,
}

impl WorkspaceRoots {
    pub fn new(primary: &Path) -> Result<Self, RootError> {
        let mut roots = Self::default();
        roots.add(primary, true)?;
        Ok(roots)
    }

    pub fn list(&self) -> &[WorkspaceRoot] {
        &self.roots
    }

    pub fn primary(&self) -> Option<&WorkspaceRoot> {
        self.roots.first()
    }

    pub fn add(&mut self, path: &Path, trusted: bool) -> Result<(), RootError> {
        if self.roots.len() >= MAX_WORKSPACE_ROOTS {
            return Err(RootError::Limit);
        }
        let path = canonical_directory(path)?;
        if self.roots.iter().any(|root| root.path == path) {
            return Err(RootError::Duplicate(path));
        }
        self.roots.push(WorkspaceRoot { path, trusted });
        Ok(())
    }

    pub fn remove(&mut self, path: &Path) -> Result<(), RootError> {
        let path = canonical_directory(path)?;
        let before = self.roots.len();
        self.roots.retain(|root| root.path != path);
        if self.roots.len() == before {
            return Err(RootError::Missing(path));
        }
        Ok(())
    }

    /// Resolves a path against the primary root and returns its owning root.
    pub fn resolve(&self, path: &Path, require_trusted: bool) -> Result<PathBuf, RootError> {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            let primary = self
                .primary()
                .ok_or_else(|| RootError::Missing(path.to_path_buf()))?;
            primary.path.join(path)
        };
        let candidate = normalize(&candidate);
        let root = self
            .roots
            .iter()
            .filter(|root| candidate.starts_with(&root.path))
            .max_by_key(|root| root.path.components().count())
            .ok_or_else(|| RootError::OutsideRoots(candidate.clone()))?;
        if require_trusted && !root.trusted {
            return Err(RootError::Untrusted(root.path.clone()));
        }
        Ok(candidate)
    }

    pub fn owning_root(&self, path: &Path) -> Option<&WorkspaceRoot> {
        let path = normalize(path);
        self.roots
            .iter()
            .filter(|root| path.starts_with(&root.path))
            .max_by_key(|root| root.path.components().count())
    }

    pub fn relative(&self, path: &Path) -> Option<PathBuf> {
        let root = self.owning_root(path)?;
        normalize(path)
            .strip_prefix(&root.path)
            .ok()
            .map(Path::to_path_buf)
    }
}

fn canonical_directory(path: &Path) -> Result<PathBuf, RootError> {
    if !path.is_absolute() {
        return Err(RootError::Invalid(format!(
            "workspace root must be absolute: {}",
            path.display()
        )));
    }
    let canonical = fs::canonicalize(path).map_err(|_| RootError::Missing(path.to_path_buf()))?;
    if !canonical.is_dir() {
        return Err(RootError::Invalid(format!(
            "workspace root is not a directory: {}",
            path.display()
        )));
    }
    Ok(canonical)
}

fn normalize(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                output.pop();
            }
            component => output.push(component.as_os_str()),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> (tempfile::TempDir, tempfile::TempDir, WorkspaceRoots) {
        let first = tempfile::tempdir().expect("first root");
        let second = tempfile::tempdir().expect("second root");
        let mut roots = WorkspaceRoots::new(first.path()).expect("primary root");
        roots.add(second.path(), false).expect("second root");
        (first, second, roots)
    }

    #[test]
    fn resolves_relative_paths_against_primary() {
        let (first, _second, roots) = roots();
        let path = roots
            .resolve(Path::new("src/main.rs"), true)
            .expect("resolve");
        assert_eq!(path, first.path().join("src/main.rs"));
    }

    #[test]
    fn rejects_paths_outside_roots() {
        let (_first, _second, roots) = roots();
        let error = roots
            .resolve(Path::new("/definitely/not/a/root/file"), false)
            .unwrap_err();
        assert!(matches!(error, RootError::OutsideRoots(_)));
    }

    #[test]
    fn enforces_trust_for_write_resolution() {
        let (_first, second, roots) = roots();
        let path = second.path().join("file.rs");
        let error = roots.resolve(&path, true).unwrap_err();
        assert!(matches!(error, RootError::Untrusted(_)));
        assert_eq!(roots.resolve(&path, false).unwrap(), path);
    }

    #[test]
    fn rejects_duplicate_and_relative_roots() {
        let (first, _second, mut roots) = roots();
        assert!(matches!(
            roots.add(first.path(), true).unwrap_err(),
            RootError::Duplicate(_)
        ));
        assert!(matches!(
            roots.add(Path::new("relative"), true).unwrap_err(),
            RootError::Invalid(_)
        ));
    }

    #[test]
    fn reports_owning_root_and_relative_path() {
        let (_first, second, roots) = roots();
        let path = second.path().join("nested/file.rs");
        assert_eq!(
            roots.owning_root(&path).map(|root| root.path.as_path()),
            Some(second.path())
        );
        assert_eq!(roots.relative(&path), Some(PathBuf::from("nested/file.rs")));
    }
}

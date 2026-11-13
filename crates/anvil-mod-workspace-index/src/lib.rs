//! Bounded in-memory workspace index used by the optional workspace-index module.
//!
//! The index is intentionally UI-neutral. It performs no filesystem I/O; callers
//! provide root-relative paths from the owning gateway.

use std::collections::BTreeSet;

pub const DEFAULT_MAX_FILES: usize = 200_000;
pub const DEFAULT_MAX_PATH_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_MAX_QUERY_BYTES: usize = 256;
pub const DEFAULT_MAX_RESULTS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceIndexCaps {
    pub max_files: usize,
    pub max_path_bytes: usize,
    pub max_query_bytes: usize,
    pub max_results: usize,
}

impl Default for WorkspaceIndexCaps {
    fn default() -> Self {
        Self {
            max_files: DEFAULT_MAX_FILES,
            max_path_bytes: DEFAULT_MAX_PATH_BYTES,
            max_query_bytes: DEFAULT_MAX_QUERY_BYTES,
            max_results: DEFAULT_MAX_RESULTS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushOutcome {
    Inserted,
    Duplicate,
    Truncated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceIndexSnapshot {
    pub generation: u64,
    pub paths: Vec<String>,
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub struct WorkspaceIndex {
    caps: WorkspaceIndexCaps,
    paths: BTreeSet<String>,
    path_bytes: usize,
    truncated: bool,
}

impl WorkspaceIndex {
    pub fn new(caps: WorkspaceIndexCaps) -> Self {
        Self {
            caps,
            paths: BTreeSet::new(),
            path_bytes: 0,
            truncated: false,
        }
    }

    pub fn caps(&self) -> WorkspaceIndexCaps {
        self.caps
    }

    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Inserts one normalized root-relative path.
    ///
    /// Once a cap is reached the index becomes truncated and no further paths
    /// are accepted. An empty or absolute path is rejected as `Truncated` so a
    /// malformed gateway result cannot enter the index.
    pub fn push(&mut self, path: &str) -> PushOutcome {
        if self.truncated {
            return PushOutcome::Truncated;
        }
        if path.is_empty() || std::path::Path::new(path).is_absolute() {
            self.truncated = true;
            return PushOutcome::Truncated;
        }
        if self.paths.contains(path) {
            return PushOutcome::Duplicate;
        }
        let next_bytes = self.path_bytes.saturating_add(path.len());
        if self.paths.len() >= self.caps.max_files || next_bytes > self.caps.max_path_bytes {
            self.truncated = true;
            return PushOutcome::Truncated;
        }
        let path = path.to_owned();
        self.path_bytes = next_bytes;
        self.paths.insert(path);
        PushOutcome::Inserted
    }

    pub fn snapshot(&self, generation: u64) -> WorkspaceIndexSnapshot {
        WorkspaceIndexSnapshot {
            generation,
            paths: self.paths.iter().cloned().collect(),
            truncated: self.truncated,
        }
    }

    /// Returns bounded, case-insensitive substring matches in path order.
    pub fn search(&self, query: &str) -> Vec<String> {
        if query.len() > self.caps.max_query_bytes {
            return Vec::new();
        }
        let query = query.to_lowercase();
        self.paths
            .iter()
            .filter(|path| path.to_lowercase().contains(&query))
            .take(self.caps.max_results)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps() -> WorkspaceIndexCaps {
        WorkspaceIndexCaps {
            max_files: 4,
            max_path_bytes: 32,
            max_query_bytes: 8,
            max_results: 2,
        }
    }

    #[test]
    fn enforces_file_and_byte_caps() {
        let mut index = WorkspaceIndex::new(caps());
        assert_eq!(index.push("a.rs"), PushOutcome::Inserted);
        assert_eq!(index.push("b.rs"), PushOutcome::Inserted);
        assert_eq!(index.push("c.rs"), PushOutcome::Inserted);
        assert_eq!(index.push("d.rs"), PushOutcome::Inserted);
        assert_eq!(index.push("e.rs"), PushOutcome::Truncated);
        assert!(index.is_truncated());
        assert_eq!(index.len(), 4);
        assert_eq!(index.push("a.rs"), PushOutcome::Truncated);

        let mut bytes = WorkspaceIndex::new(caps());
        assert_eq!(
            bytes.push("01234567890123456789012345678901"),
            PushOutcome::Inserted
        );
        assert_eq!(bytes.push("x"), PushOutcome::Truncated);
        assert!(bytes.is_truncated());
    }

    #[test]
    fn rejects_empty_absolute_and_duplicate_paths() {
        let mut index = WorkspaceIndex::new(caps());
        assert_eq!(index.push("src/main.rs"), PushOutcome::Inserted);
        assert_eq!(index.push("src/main.rs"), PushOutcome::Duplicate);
        assert_eq!(index.push("/absolute.rs"), PushOutcome::Truncated);
        assert!(index.is_truncated());

        let mut empty = WorkspaceIndex::new(caps());
        assert_eq!(empty.push(""), PushOutcome::Truncated);
        assert!(empty.is_truncated());
    }

    #[test]
    fn snapshot_is_sorted_and_bounded() {
        let mut index = WorkspaceIndex::new(caps());
        index.push("z.rs");
        index.push("a.rs");
        let snapshot = index.snapshot(7);
        assert_eq!(snapshot.generation, 7);
        assert_eq!(snapshot.paths, vec!["a.rs", "z.rs"]);
        assert!(!snapshot.truncated);
    }

    #[test]
    fn search_caps_query_and_results() {
        let mut index = WorkspaceIndex::new(caps());
        index.push("src/main.rs");
        index.push("src/lib.rs");
        index.push("other/main.rs");
        assert_eq!(index.search("SRC").len(), 2);
        assert_eq!(index.search("src").len(), 2);
        assert!(index.search("123456789").is_empty());
        assert_eq!(index.search("").len(), 2);
    }
}

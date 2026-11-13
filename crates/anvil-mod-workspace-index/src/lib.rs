//! Bounded in-memory workspace index and gateway-backed service.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use anvil_ipc::{DirEntry, GatewayClient};
use anvil_module::workspace_index::{
    WorkspaceIndexError, WorkspaceIndexService, WorkspaceIndexSnapshot,
};

pub const DEFAULT_MAX_FILES: usize = 200_000;
pub const DEFAULT_MAX_PATH_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_MAX_QUERY_BYTES: usize = 256;
pub const DEFAULT_MAX_RESULTS: usize = 512;

pub const MODULE_ID: anvil_module::ModuleId =
    anvil_module::ModuleId::new("official.workspace-index");

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

    pub fn push(&mut self, path: &str) -> PushOutcome {
        if self.truncated {
            return PushOutcome::Truncated;
        }
        if path.is_empty() || Path::new(path).is_absolute() {
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
        self.path_bytes = next_bytes;
        self.paths.insert(path.to_owned());
        PushOutcome::Inserted
    }

    pub fn snapshot(&self, generation: u64) -> WorkspaceIndexSnapshot {
        WorkspaceIndexSnapshot {
            generation,
            paths: self.paths.iter().cloned().collect(),
            truncated: self.truncated,
        }
    }

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

/// Gateway-neutral directory source used by the index walker.
pub trait WorkspaceIndexSource {
    fn read_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, String>;
}

impl WorkspaceIndexSource for GatewayClient {
    fn read_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, String> {
        GatewayClient::read_dir(self, path).map_err(|error| error.to_string())
    }
}

#[derive(Default)]
pub struct GatewayWorkspaceIndex {
    caps: WorkspaceIndexCaps,
    indexes: Mutex<HashMap<PathBuf, (u64, WorkspaceIndex)>>,
    latest: Mutex<HashMap<PathBuf, u64>>,
}

impl GatewayWorkspaceIndex {
    pub fn new(caps: WorkspaceIndexCaps) -> Self {
        Self {
            caps,
            indexes: Mutex::new(HashMap::new()),
            latest: Mutex::new(HashMap::new()),
        }
    }

    fn build_from_source(
        &self,
        source: &mut impl WorkspaceIndexSource,
        root: &Path,
        hidden: bool,
        generation: u64,
        cancel: &AtomicBool,
    ) -> Result<WorkspaceIndexSnapshot, WorkspaceIndexError> {
        let root = root.to_path_buf();
        let mut latest = self
            .latest
            .lock()
            .map_err(|_| WorkspaceIndexError::Io("workspace index lock poisoned".to_owned()))?;
        if generation < *latest.get(&root).unwrap_or(&0) {
            return Err(WorkspaceIndexError::Stale);
        }
        latest.insert(root.clone(), generation);
        drop(latest);

        let mut index = WorkspaceIndex::new(self.caps);
        let mut pending = vec![String::new()];
        while let Some(directory) = pending.pop() {
            if cancel.load(Ordering::Relaxed) {
                return Err(WorkspaceIndexError::Cancelled);
            }
            let entries = source
                .read_dir(&directory)
                .map_err(WorkspaceIndexError::Io)?;
            for entry in entries {
                if entry.name == "." || entry.name == ".." {
                    continue;
                }
                if !hidden && entry.name.starts_with('.') {
                    continue;
                }
                let path = if directory.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{directory}/{}", entry.name)
                };
                if entry.is_dir {
                    pending.push(path);
                } else {
                    index.push(&path);
                    if index.is_truncated() {
                        break;
                    }
                }
            }
            if index.is_truncated() {
                break;
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(WorkspaceIndexError::Cancelled);
        }
        let latest = self
            .latest
            .lock()
            .map_err(|_| WorkspaceIndexError::Io("workspace index lock poisoned".to_owned()))?;
        if latest.get(&root).copied() != Some(generation) {
            return Err(WorkspaceIndexError::Stale);
        }
        self.indexes
            .lock()
            .map_err(|_| WorkspaceIndexError::Io("workspace index lock poisoned".to_owned()))?
            .insert(root, (generation, index.clone()));
        Ok(index.snapshot(generation))
    }
}

impl WorkspaceIndexService for GatewayWorkspaceIndex {
    fn build(
        &self,
        client: &mut GatewayClient,
        root: &Path,
        hidden: bool,
        generation: u64,
        cancel: &AtomicBool,
    ) -> Result<WorkspaceIndexSnapshot, WorkspaceIndexError> {
        self.build_from_source(client, root, hidden, generation, cancel)
    }

    fn search(&self, root: &Path, query: &str, limit: usize) -> Vec<String> {
        let Some(index) = self
            .indexes
            .lock()
            .ok()
            .and_then(|indexes| indexes.get(root).map(|(_, index)| index.clone()))
        else {
            return Vec::new();
        };
        let mut results = index.search(query);
        results.truncate(limit);
        results
    }
}

/// Official workspace-index module.
#[derive(Debug, Default, Clone, Copy)]
pub struct WorkspaceIndexModule;

impl anvil_module::Module for WorkspaceIndexModule {
    fn metadata(&self) -> anvil_module::ModuleMetadata {
        anvil_module::ModuleMetadata {
            id: MODULE_ID,
            name: "Official Workspace Index",
            contract: anvil_module::ContractRange::current(),
            commands: &[],
        }
    }

    fn register(
        &self,
        registry: &mut anvil_module::ModuleRegistry,
    ) -> Result<(), anvil_module::ModuleError> {
        registry.set_workspace_index_service(std::sync::Arc::new(GatewayWorkspaceIndex::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Default)]
    struct FakeSource {
        dirs: HashMap<String, Vec<DirEntry>>,
    }

    impl WorkspaceIndexSource for FakeSource {
        fn read_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, String> {
            self.dirs
                .remove(path)
                .ok_or_else(|| format!("unknown directory {path}"))
        }
    }

    fn entry(name: &str, is_dir: bool) -> DirEntry {
        DirEntry {
            name: name.to_owned(),
            is_dir,
            ino: 0,
        }
    }

    fn fixture_source() -> FakeSource {
        let mut dirs = HashMap::new();
        dirs.insert(
            String::new(),
            vec![
                entry("src", true),
                entry("README.md", false),
                entry(".hidden", false),
            ],
        );
        dirs.insert(
            "src".to_owned(),
            vec![entry("main.rs", false), entry("lib.rs", false)],
        );
        FakeSource { dirs }
    }

    fn caps() -> WorkspaceIndexCaps {
        WorkspaceIndexCaps {
            max_files: 4,
            max_path_bytes: 32,
            max_query_bytes: 8,
            max_results: 2,
        }
    }

    #[test]
    fn registers_the_official_workspace_index_module() {
        let mut registry = anvil_module::ModuleRegistry::new();
        registry.register(&WorkspaceIndexModule).expect("register");
        assert!(registry.contains(MODULE_ID));
        assert!(registry.workspace_index_service().is_some());
    }

    #[test]
    fn enforces_file_and_byte_caps() {
        let mut index = WorkspaceIndex::new(caps());
        for path in ["a.rs", "b.rs", "c.rs", "d.rs"] {
            assert_eq!(index.push(path), PushOutcome::Inserted);
        }
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
        assert_eq!(index.search("123456789").len(), 0);
        assert_eq!(index.search("").len(), 2);
    }

    #[test]
    fn builds_sorted_paths_and_hides_dotfiles() {
        let index = GatewayWorkspaceIndex::new(caps());
        let mut source = fixture_source();
        let snapshot = index
            .build_from_source(
                &mut source,
                Path::new("/ws"),
                false,
                1,
                &AtomicBool::new(false),
            )
            .expect("build");
        assert_eq!(
            snapshot.paths,
            vec!["README.md", "src/lib.rs", "src/main.rs"]
        );
        assert_eq!(snapshot.generation, 1);
    }

    #[test]
    fn cancellation_does_not_publish() {
        let index = GatewayWorkspaceIndex::new(caps());
        let mut source = fixture_source();
        let cancel = AtomicBool::new(true);
        assert_eq!(
            index.build_from_source(&mut source, Path::new("/ws"), false, 2, &cancel),
            Err(WorkspaceIndexError::Cancelled)
        );
    }

    #[test]
    fn search_reads_the_published_snapshot() {
        let index = GatewayWorkspaceIndex::new(caps());
        let mut source = fixture_source();
        index
            .build_from_source(
                &mut source,
                Path::new("/ws"),
                false,
                1,
                &AtomicBool::new(false),
            )
            .expect("build");
        assert_eq!(index.search(Path::new("/ws"), "SRC", 1), vec!["src/lib.rs"]);
        assert!(index.search(Path::new("/ws"), "missing", 10).is_empty());
        assert!(index.search(Path::new("/ws"), "src", 0).is_empty());
    }

    struct CancelAfterFirst {
        source: FakeSource,
        cancel: Arc<AtomicBool>,
    }

    impl WorkspaceIndexSource for CancelAfterFirst {
        fn read_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, String> {
            let result = self.source.read_dir(path);
            self.cancel.store(true, Ordering::Relaxed);
            result
        }
    }

    #[test]
    fn cancellation_during_the_walk_does_not_publish() {
        let index = GatewayWorkspaceIndex::new(caps());
        let cancel = Arc::new(AtomicBool::new(false));
        let mut source = CancelAfterFirst {
            source: fixture_source(),
            cancel: Arc::clone(&cancel),
        };
        assert_eq!(
            index.build_from_source(&mut source, Path::new("/ws"), false, 3, &cancel,),
            Err(WorkspaceIndexError::Cancelled)
        );
        assert!(index.search(Path::new("/ws"), "src", 10).is_empty());
    }

    #[test]
    fn stale_generation_is_rejected() {
        let index = GatewayWorkspaceIndex::new(caps());
        let mut source = fixture_source();
        assert!(
            index
                .build_from_source(
                    &mut source,
                    Path::new("/ws"),
                    false,
                    5,
                    &AtomicBool::new(false)
                )
                .is_ok()
        );
        let mut next_source = fixture_source();
        assert_eq!(
            index.build_from_source(
                &mut next_source,
                Path::new("/ws"),
                false,
                4,
                &AtomicBool::new(false)
            ),
            Err(WorkspaceIndexError::Stale)
        );
    }
}

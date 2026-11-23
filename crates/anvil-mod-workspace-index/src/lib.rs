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

pub const DEFAULT_MAX_SYMBOLS: usize = 200_000;
pub const DEFAULT_MAX_OCCURRENCES: usize = 1_000_000;
pub const DEFAULT_MAX_SYMBOL_NAME_BYTES: usize = 256;
pub const DEFAULT_MAX_SYMBOL_PATH_BYTES: usize = 4096;
pub const DEFAULT_MAX_SYMBOL_QUERY_BYTES: usize = 256;
pub const DEFAULT_MAX_SYMBOL_BYTES: usize = 32 * 1024 * 1024;

/// Bounds for the symbol/occurrence layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymbolIndexCaps {
    pub max_symbols: usize,
    pub max_occurrences: usize,
    pub max_name_bytes: usize,
    pub max_path_bytes: usize,
    pub max_query_bytes: usize,
    pub max_total_bytes: usize,
    pub max_results: usize,
}

impl Default for SymbolIndexCaps {
    fn default() -> Self {
        Self {
            max_symbols: DEFAULT_MAX_SYMBOLS,
            max_occurrences: DEFAULT_MAX_OCCURRENCES,
            max_name_bytes: DEFAULT_MAX_SYMBOL_NAME_BYTES,
            max_path_bytes: DEFAULT_MAX_SYMBOL_PATH_BYTES,
            max_query_bytes: DEFAULT_MAX_SYMBOL_QUERY_BYTES,
            max_total_bytes: DEFAULT_MAX_SYMBOL_BYTES,
            max_results: DEFAULT_MAX_RESULTS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Module,
    Function,
    Struct,
    Enum,
    Trait,
    Method,
    Field,
    Constant,
    Variable,
    Other(String),
}

impl SymbolKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Module => "module",
            Self::Function => "function",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Method => "method",
            Self::Field => "field",
            Self::Constant => "constant",
            Self::Variable => "variable",
            Self::Other(value) => value,
        }
    }

    pub fn from_label(label: &str) -> Self {
        match label {
            "module" => Self::Module,
            "function" => Self::Function,
            "struct" => Self::Struct,
            "enum" => Self::Enum,
            "trait" => Self::Trait,
            "method" => Self::Method,
            "field" => Self::Field,
            "constant" => Self::Constant,
            "variable" => Self::Variable,
            other => Self::Other(other.to_owned()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolRole {
    Definition,
    Reference,
    Implementation,
}

impl SymbolRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Reference => "reference",
            Self::Implementation => "implementation",
        }
    }

    pub fn from_label(label: &str) -> Self {
        match label {
            "reference" => Self::Reference,
            "implementation" => Self::Implementation,
            _ => Self::Definition,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub path: String,
    pub range: TextRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolOccurrence {
    pub symbol: String,
    pub role: SymbolRole,
    pub path: String,
    pub range: TextRange,
}

/// Bounded symbol/occurrence index. It is independent of the path walker so
/// external indexers can feed symbols without rescanning files.
#[derive(Clone, Debug)]
pub struct WorkspaceSymbolIndex {
    caps: SymbolIndexCaps,
    symbols: Vec<WorkspaceSymbol>,
    symbol_keys: BTreeSet<(String, String, u32, u32, u32, u32)>,
    occurrences: Vec<SymbolOccurrence>,
    occurrence_keys: BTreeSet<(String, String, u32, u32, u32, u32, u8)>,
    bytes: usize,
    truncated: bool,
}

impl WorkspaceSymbolIndex {
    pub fn new(caps: SymbolIndexCaps) -> Self {
        Self {
            caps,
            symbols: Vec::new(),
            symbol_keys: BTreeSet::new(),
            occurrences: Vec::new(),
            occurrence_keys: BTreeSet::new(),
            bytes: 0,
            truncated: false,
        }
    }

    pub fn caps(&self) -> SymbolIndexCaps {
        self.caps
    }

    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    pub fn occurrence_len(&self) -> usize {
        self.occurrences.len()
    }

    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty() && self.occurrences.is_empty()
    }

    pub fn push_symbol(&mut self, symbol: WorkspaceSymbol) -> PushOutcome {
        if self.truncated {
            return PushOutcome::Truncated;
        }
        if !valid_symbol_path(&symbol.path)
            || symbol.path.len() > self.caps.max_path_bytes
            || !valid_range(symbol.range)
            || symbol.name.is_empty()
            || symbol.name.len() > self.caps.max_name_bytes
            || symbol_kind_bytes(&symbol.kind) > self.caps.max_name_bytes
        {
            self.truncated = true;
            return PushOutcome::Truncated;
        }
        let bytes = symbol.name.len() + symbol.path.len() + symbol_kind_bytes(&symbol.kind);
        let key = (
            symbol.name.clone(),
            symbol.path.clone(),
            symbol.range.start.line,
            symbol.range.start.character,
            symbol.range.end.line,
            symbol.range.end.character,
        );
        if self.symbol_keys.contains(&key) {
            return PushOutcome::Duplicate;
        }
        if self.symbols.len() >= self.caps.max_symbols
            || self.bytes.saturating_add(bytes) > self.caps.max_total_bytes
        {
            self.truncated = true;
            return PushOutcome::Truncated;
        }
        self.bytes += bytes;
        self.symbol_keys.insert(key);
        self.symbols.push(symbol);
        PushOutcome::Inserted
    }

    pub fn push_occurrence(&mut self, occurrence: SymbolOccurrence) -> PushOutcome {
        if self.truncated {
            return PushOutcome::Truncated;
        }
        if !valid_symbol_path(&occurrence.path)
            || occurrence.path.len() > self.caps.max_path_bytes
            || !valid_range(occurrence.range)
            || occurrence.symbol.is_empty()
            || occurrence.symbol.len() > self.caps.max_name_bytes
        {
            self.truncated = true;
            return PushOutcome::Truncated;
        }
        let bytes = occurrence.symbol.len() + occurrence.path.len();
        let role = match occurrence.role {
            SymbolRole::Definition => 0,
            SymbolRole::Reference => 1,
            SymbolRole::Implementation => 2,
        };
        let key = (
            occurrence.symbol.clone(),
            occurrence.path.clone(),
            occurrence.range.start.line,
            occurrence.range.start.character,
            occurrence.range.end.line,
            occurrence.range.end.character,
            role,
        );
        if self.occurrence_keys.contains(&key) {
            return PushOutcome::Duplicate;
        }
        if self.occurrences.len() >= self.caps.max_occurrences
            || self.bytes.saturating_add(bytes) > self.caps.max_total_bytes
        {
            self.truncated = true;
            return PushOutcome::Truncated;
        }
        self.bytes += bytes;
        self.occurrence_keys.insert(key);
        self.occurrences.push(occurrence);
        PushOutcome::Inserted
    }

    pub fn all_symbols(&self) -> &[WorkspaceSymbol] {
        &self.symbols
    }

    pub fn all_occurrences(&self) -> &[SymbolOccurrence] {
        &self.occurrences
    }

    pub fn symbols(&self, query: &str, limit: usize) -> Vec<WorkspaceSymbol> {
        if query.len() > self.caps.max_query_bytes {
            return Vec::new();
        }
        let query = query.to_lowercase();
        self.symbols
            .iter()
            .filter(|symbol| symbol.name.to_lowercase().contains(query.as_str()))
            .take(limit.min(self.caps.max_results))
            .cloned()
            .collect()
    }

    pub fn occurrences(&self, path: &str, symbol: &str, limit: usize) -> Vec<SymbolOccurrence> {
        if path.len() > self.caps.max_path_bytes || symbol.len() > self.caps.max_name_bytes {
            return Vec::new();
        }
        self.occurrences
            .iter()
            .filter(|occurrence| occurrence.path == path && occurrence.symbol == symbol)
            .take(limit.min(self.caps.max_results))
            .cloned()
            .collect()
    }

    /// Remove symbols and occurrences for a file or directory subtree.
    pub fn remove_paths_under(&mut self, prefix: &str) -> usize {
        if prefix.is_empty() {
            return 0;
        }
        let matches = |path: &str| {
            path == prefix
                || path
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('/'))
        };
        let before = self.symbols.len() + self.occurrences.len();
        self.symbols.retain(|symbol| !matches(&symbol.path));
        self.occurrences
            .retain(|occurrence| !matches(&occurrence.path));
        self.symbol_keys = self
            .symbols
            .iter()
            .map(|symbol| {
                (
                    symbol.name.clone(),
                    symbol.path.clone(),
                    symbol.range.start.line,
                    symbol.range.start.character,
                    symbol.range.end.line,
                    symbol.range.end.character,
                )
            })
            .collect();
        self.occurrence_keys = self
            .occurrences
            .iter()
            .map(|occurrence| {
                let role = match occurrence.role {
                    SymbolRole::Definition => 0,
                    SymbolRole::Reference => 1,
                    SymbolRole::Implementation => 2,
                };
                (
                    occurrence.symbol.clone(),
                    occurrence.path.clone(),
                    occurrence.range.start.line,
                    occurrence.range.start.character,
                    occurrence.range.end.line,
                    occurrence.range.end.character,
                    role,
                )
            })
            .collect();
        self.bytes = self
            .symbols
            .iter()
            .map(|symbol| symbol.name.len() + symbol.path.len() + symbol_kind_bytes(&symbol.kind))
            .sum::<usize>()
            + self
                .occurrences
                .iter()
                .map(|occurrence| occurrence.symbol.len() + occurrence.path.len())
                .sum::<usize>();
        before - (self.symbols.len() + self.occurrences.len())
    }
}

fn symbol_kind_bytes(kind: &SymbolKind) -> usize {
    match kind {
        SymbolKind::Other(value) => value.len(),
        _ => 0,
    }
}

fn valid_symbol_path(path: &str) -> bool {
    !path.is_empty()
        && !Path::new(path).is_absolute()
        && !Path::new(path)
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
}

fn valid_range(range: TextRange) -> bool {
    range.start <= range.end
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

    fn symbol(name: &str, path: &str, line: u32) -> WorkspaceSymbol {
        WorkspaceSymbol {
            name: name.to_owned(),
            kind: SymbolKind::Function,
            path: path.to_owned(),
            range: TextRange {
                start: TextPosition { line, character: 0 },
                end: TextPosition { line, character: 3 },
            },
        }
    }

    fn occurrence(symbol: &str, path: &str, line: u32) -> SymbolOccurrence {
        SymbolOccurrence {
            symbol: symbol.to_owned(),
            role: SymbolRole::Reference,
            path: path.to_owned(),
            range: TextRange {
                start: TextPosition { line, character: 0 },
                end: TextPosition { line, character: 3 },
            },
        }
    }

    #[test]
    fn symbol_index_deduplicates_and_bounds() {
        let mut index = WorkspaceSymbolIndex::new(SymbolIndexCaps {
            max_symbols: 1,
            max_occurrences: 1,
            max_name_bytes: 16,
            max_path_bytes: 64,
            max_query_bytes: 16,
            max_total_bytes: 1024,
            max_results: 1,
        });
        assert_eq!(
            index.push_symbol(symbol("run", "src/lib.rs", 1)),
            PushOutcome::Inserted
        );
        assert_eq!(
            index.push_symbol(symbol("run", "src/lib.rs", 1)),
            PushOutcome::Duplicate
        );
        assert_eq!(
            index.push_symbol(symbol("stop", "src/lib.rs", 2)),
            PushOutcome::Truncated
        );
        assert!(index.is_truncated());
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn symbol_index_rejects_invalid_ranges_and_paths() {
        let mut index = WorkspaceSymbolIndex::new(SymbolIndexCaps::default());
        let mut invalid = symbol("run", "src/lib.rs", 1);
        invalid.range.start.character = 4;
        assert_eq!(index.push_symbol(invalid), PushOutcome::Truncated);
        let absolute = symbol("run", "/src/lib.rs", 1);
        assert_eq!(index.push_symbol(absolute), PushOutcome::Truncated);
        let escape = symbol("run", "../src/lib.rs", 1);
        assert_eq!(index.push_symbol(escape), PushOutcome::Truncated);

        let mut long = WorkspaceSymbolIndex::new(SymbolIndexCaps::default());
        let path = "a".repeat(DEFAULT_MAX_SYMBOL_PATH_BYTES + 1);
        assert_eq!(
            long.push_symbol(symbol("run", &path, 1)),
            PushOutcome::Truncated
        );
    }

    #[test]
    fn symbol_and_occurrence_queries_are_bounded() {
        let mut index = WorkspaceSymbolIndex::new(SymbolIndexCaps {
            max_symbols: 4,
            max_occurrences: 4,
            max_name_bytes: 16,
            max_path_bytes: 64,
            max_query_bytes: 16,
            max_total_bytes: 1024,
            max_results: 2,
        });
        for (name, line) in [("run", 1), ("runner", 2), ("stop", 3)] {
            assert_eq!(
                index.push_symbol(symbol(name, "src/lib.rs", line)),
                PushOutcome::Inserted
            );
        }
        assert_eq!(index.symbols("RUN", 10).len(), 2);
        assert!(index.symbols(&"x".repeat(17), 10).is_empty());
        assert_eq!(
            index.push_occurrence(occurrence("run", "src/lib.rs", 4)),
            PushOutcome::Inserted
        );
        assert_eq!(
            index.push_occurrence(occurrence("run", "src/lib.rs", 4)),
            PushOutcome::Duplicate
        );
        assert_eq!(index.occurrences("src/lib.rs", "run", 10).len(), 1);
        assert_eq!(index.occurrence_len(), 1);
    }

    #[test]
    fn remove_paths_under_drops_files_and_subtrees() {
        let mut index = WorkspaceSymbolIndex::new(SymbolIndexCaps::default());
        for (name, path) in [
            ("run", "src/a.rs"),
            ("nested", "src/a/nested.rs"),
            ("other", "src/b.rs"),
        ] {
            assert_eq!(
                index.push_symbol(symbol(name, path, 1)),
                PushOutcome::Inserted
            );
        }
        assert_eq!(
            index.push_occurrence(occurrence("run", "src/a.rs", 2)),
            PushOutcome::Inserted
        );
        assert_eq!(index.remove_paths_under("src/a.rs"), 2);
        assert_eq!(index.len(), 2);
        assert_eq!(index.occurrence_len(), 0);
        assert_eq!(index.remove_paths_under("src"), 2);
        assert!(index.is_empty());
    }
}

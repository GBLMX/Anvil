use anvil_mod_workspace_index::{
    PushOutcome, SymbolKind, SymbolOccurrence, SymbolRole, TextPosition, TextRange,
    WorkspaceSymbol, WorkspaceSymbolIndex,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const CACHE_VERSION: u32 = 1;
const MAX_CACHE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CACHE_SYMBOLS: usize = 200_000;
const MAX_CACHE_OCCURRENCES: usize = 200_000;

static CACHE_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct SymbolCache {
    path: PathBuf,
}

impl SymbolCache {
    pub fn for_workspace(root: &Path) -> Self {
        let digest = Sha256::digest(root.as_os_str().as_encoded_bytes());
        let key = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Self {
            path: cache_root()
                .join("anvil/symbols")
                .join(format!("{key}.json")),
        }
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_into(&self, index: &mut WorkspaceSymbolIndex) -> Result<usize, String> {
        let metadata = match fs::metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => {
                return Err(format!(
                    "failed to stat symbol cache {}: {error}",
                    self.path.display()
                ));
            }
        };
        if !metadata.is_file() {
            return Err(format!(
                "symbol cache {} is not a regular file",
                self.path.display()
            ));
        }
        if metadata.len() > MAX_CACHE_BYTES {
            return Err(format!(
                "symbol cache {} exceeds {} bytes",
                self.path.display(),
                MAX_CACHE_BYTES
            ));
        }
        let file = File::open(&self.path).map_err(|error| {
            format!(
                "failed to open symbol cache {}: {error}",
                self.path.display()
            )
        })?;
        let cache: CacheFile = serde_json::from_reader(BufReader::new(file)).map_err(|error| {
            format!(
                "failed to read symbol cache {}: {error}",
                self.path.display()
            )
        })?;
        if cache.version != CACHE_VERSION {
            return Err(format!(
                "unsupported symbol cache version {} in {}",
                cache.version,
                self.path.display()
            ));
        }
        let mut inserted = 0usize;
        for symbol in cache.symbols.into_iter().take(MAX_CACHE_SYMBOLS) {
            match index.push_symbol(symbol.into()) {
                PushOutcome::Inserted => inserted += 1,
                PushOutcome::Truncated => break,
                PushOutcome::Duplicate => {}
            }
        }
        for occurrence in cache.occurrences.into_iter().take(MAX_CACHE_OCCURRENCES) {
            if index.push_occurrence(occurrence.into()) == PushOutcome::Truncated {
                break;
            }
        }
        Ok(inserted)
    }

    pub fn save(&self, index: &WorkspaceSymbolIndex) -> Result<(), String> {
        let cache = CacheFile {
            version: CACHE_VERSION,
            symbols: index
                .all_symbols()
                .iter()
                .take(MAX_CACHE_SYMBOLS)
                .map(CachedSymbol::from)
                .collect(),
            occurrences: index
                .all_occurrences()
                .iter()
                .take(MAX_CACHE_OCCURRENCES)
                .map(CachedOccurrence::from)
                .collect(),
        };
        let bytes = serde_json::to_vec(&cache)
            .map_err(|error| format!("failed to encode symbol cache: {error}"))?;
        if bytes.len() as u64 > MAX_CACHE_BYTES {
            return Err(format!("symbol cache exceeds {MAX_CACHE_BYTES} bytes"));
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| format!("symbol cache path {} has no parent", self.path.display()))?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        let temporary = parent.join(format!(
            ".{}.{}.{}.tmp",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("symbols"),
            std::process::id(),
            CACHE_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&temporary)
                .map_err(|error| format!("failed to create {}: {error}", temporary.display()))?;
            file.write_all(&bytes)
                .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
            file.sync_all()
                .map_err(|error| format!("failed to sync {}: {error}", temporary.display()))?;
            replace_file(&temporary, &self.path).map_err(|error| {
                format!(
                    "failed to replace {} with {}: {error}",
                    self.path.display(),
                    temporary.display()
                )
            })
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error)
                if error.kind() == std::io::ErrorKind::AlreadyExists
                    || error.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                fs::remove_file(to)?;
                fs::rename(from, to)
            }
            Err(error) => Err(error),
        }
    }
    #[cfg(not(windows))]
    {
        fs::rename(from, to)
    }
}

fn cache_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME")
        && !dir.is_empty()
    {
        return PathBuf::from(dir);
    }
    if let Some(home) = std::env::var_os("HOME")
        && !home.is_empty()
    {
        return PathBuf::from(home).join(".cache");
    }
    if let Some(dir) = std::env::var_os("LOCALAPPDATA")
        && !dir.is_empty()
    {
        return PathBuf::from(dir);
    }
    std::env::temp_dir()
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    symbols: Vec<CachedSymbol>,
    occurrences: Vec<CachedOccurrence>,
}

#[derive(Serialize, Deserialize)]
struct CachedSymbol {
    name: String,
    kind: String,
    path: String,
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
}

impl From<&WorkspaceSymbol> for CachedSymbol {
    fn from(symbol: &WorkspaceSymbol) -> Self {
        Self {
            name: symbol.name.clone(),
            kind: symbol.kind.as_str().to_owned(),
            path: symbol.path.clone(),
            start_line: symbol.range.start.line,
            start_character: symbol.range.start.character,
            end_line: symbol.range.end.line,
            end_character: symbol.range.end.character,
        }
    }
}

impl From<CachedSymbol> for WorkspaceSymbol {
    fn from(symbol: CachedSymbol) -> Self {
        Self {
            name: symbol.name,
            kind: SymbolKind::from_label(&symbol.kind),
            path: symbol.path,
            range: TextRange {
                start: TextPosition {
                    line: symbol.start_line,
                    character: symbol.start_character,
                },
                end: TextPosition {
                    line: symbol.end_line,
                    character: symbol.end_character,
                },
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CachedOccurrence {
    symbol: String,
    role: String,
    path: String,
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
}

impl From<&SymbolOccurrence> for CachedOccurrence {
    fn from(occurrence: &SymbolOccurrence) -> Self {
        Self {
            symbol: occurrence.symbol.clone(),
            role: occurrence.role.as_str().to_owned(),
            path: occurrence.path.clone(),
            start_line: occurrence.range.start.line,
            start_character: occurrence.range.start.character,
            end_line: occurrence.range.end.line,
            end_character: occurrence.range.end.character,
        }
    }
}

impl From<CachedOccurrence> for SymbolOccurrence {
    fn from(occurrence: CachedOccurrence) -> Self {
        Self {
            symbol: occurrence.symbol,
            role: SymbolRole::from_label(&occurrence.role),
            path: occurrence.path,
            range: TextRange {
                start: TextPosition {
                    line: occurrence.start_line,
                    character: occurrence.start_character,
                },
                end: TextPosition {
                    line: occurrence.end_line,
                    character: occurrence.end_character,
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_mod_workspace_index::SymbolIndexCaps;

    #[test]
    fn round_trips_symbols_and_occurrences() {
        let directory = tempfile::tempdir().expect("tempdir");
        let cache = SymbolCache::at(directory.path().join("symbols.json"));
        let mut index = WorkspaceSymbolIndex::new(SymbolIndexCaps::default());
        index.push_symbol(WorkspaceSymbol {
            name: "Widget".to_owned(),
            kind: SymbolKind::Struct,
            path: "src/lib.rs".to_owned(),
            range: TextRange {
                start: TextPosition {
                    line: 4,
                    character: 2,
                },
                end: TextPosition {
                    line: 4,
                    character: 8,
                },
            },
        });
        index.push_occurrence(SymbolOccurrence {
            symbol: "Widget".to_owned(),
            role: SymbolRole::Reference,
            path: "src/main.rs".to_owned(),
            range: TextRange {
                start: TextPosition {
                    line: 9,
                    character: 1,
                },
                end: TextPosition {
                    line: 9,
                    character: 7,
                },
            },
        });
        cache.save(&index).expect("save");

        let mut loaded = WorkspaceSymbolIndex::new(SymbolIndexCaps::default());
        assert_eq!(cache.load_into(&mut loaded).expect("load"), 1);
        assert_eq!(loaded.all_symbols(), index.all_symbols());
        assert_eq!(loaded.all_occurrences(), index.all_occurrences());
    }

    #[test]
    fn missing_cache_is_empty() {
        let directory = tempfile::tempdir().expect("tempdir");
        let cache = SymbolCache::at(directory.path().join("missing.json"));
        let mut index = WorkspaceSymbolIndex::new(SymbolIndexCaps::default());
        assert_eq!(cache.load_into(&mut index).expect("load"), 0);
    }
}

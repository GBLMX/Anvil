//! Background workspace directory and file enumeration.
//!
//! The UI thread never performs a gateway directory read. One worker owns a
//! dedicated authenticated connection; requests and results use mpsc.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anvil_ipc::{DirEntry, GatewayClient};
use serde_json::Value;

use crate::pipe_locations::PipeLocation;
use sha2::{Digest, Sha256};

mod git;
mod preview;
mod search;
use search::{apply_project_replace, files, project_replace_preview, project_search};

use git::{
    apply_git_hunk, git_blame_text, git_conflict_stage_text, git_conflicts_text, git_diff,
    git_diff_text, git_status, resolve_git_conflict,
};
#[cfg(test)]
pub(crate) use git::{parse_git_diff, parse_git_status};
pub(crate) use preview::PreviewData;
use preview::read_preview;
#[cfg(test)]
pub(crate) use preview::{PREVIEW_MAX_LINES, decode_preview};

const FILE_LIMIT: usize = 5000;
pub(crate) const MAX_TREE_WATCH_DIRS: usize = 256;
const TREE_POLL_INTERVAL: Duration = Duration::from_millis(500);
const TREE_DEBOUNCE: Duration = Duration::from_millis(500);
pub(crate) const MAX_PROJECT_RESULTS: usize = 500;
const MAX_REPLACE_FILES: usize = 100;
const MAX_REPLACE_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_REPLACE_BYTES: usize = 16 * 1024 * 1024;

/// Failure while resolving a conflict, including whether the working file was
/// already replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolveConflictError {
    pub(crate) message: String,
    pub(crate) working_written: bool,
}

impl ResolveConflictError {
    fn before_write(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            working_written: false,
        }
    }

    fn after_write(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            working_written: true,
        }
    }
}

/// One file in a project replace preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectReplaceFile {
    pub(crate) relative: String,
    pub(crate) matches: usize,
    pub(crate) hash: [u8; 32],
}

/// A reviewable project replace plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectReplacePlan {
    pub(crate) old: String,
    pub(crate) new: String,
    pub(crate) files: Vec<ProjectReplaceFile>,
}

/// A background file-tree request.
pub enum FileRequest {
    /// List one directory.
    List(PathBuf),
    /// Enumerate workspace files with `rg --files`, or a bounded fallback.
    Files { hidden: bool, generation: u64 },
    /// Run a bounded ripgrep project search.
    ProjectSearch { query: String, generation: u64 },
    /// Read a bounded preview for one workspace-relative file.
    Preview { path: String, generation: u64 },
    /// Read workspace git status for tree decorations.
    GitStatus { root: PathBuf, generation: u64 },
    /// Compare one workspace file with HEAD for gutter marks.
    GitDiff {
        path: PathBuf,
        relative: String,
        revision: u64,
    },
    /// Read a bounded unified diff for one workspace-relative file.
    GitDiffText {
        relative: String,
        staged: bool,
        generation: u64,
    },
    /// Apply one hunk patch to the index.
    GitApplyHunk {
        patch: String,
        reverse: bool,
        generation: u64,
    },
    /// List unmerged paths in the index.
    GitConflictsText { generation: u64 },
    /// Read one index stage for a conflicted workspace file.
    GitConflictStageText {
        relative: String,
        stage: u8,
        generation: u64,
    },
    /// Resolve a conflicted file to one index stage and stage it.
    GitResolveConflict {
        relative: String,
        stage: u8,
        generation: u64,
    },
    /// Read a bounded blame view for one workspace-relative file.
    GitBlameText { relative: String, generation: u64 },
    /// Move or copy one workspace entry.
    Transfer {
        from: PathBuf,
        to: PathBuf,
        copy: bool,
    },
    /// Delete one workspace entry or directory tree.
    RemoveAll(PathBuf),
    /// Replace the workspace-tree directory watch set.
    WatchDirs(Vec<PathBuf>),
    /// Capture root and expanded directory listings for a tree refresh.
    TreeSnapshot {
        generation: u64,
        root: PathBuf,
        expanded: Vec<PathBuf>,
    },
    /// Watch open buffers for external metadata changes.
    Watch(Vec<PathBuf>),
    /// Build a project replace preview.
    ProjectReplacePreview {
        old: String,
        new: String,
        generation: u64,
    },
    /// Apply a reviewed project replace plan.
    ProjectReplaceApply {
        plan: ProjectReplacePlan,
        generation: u64,
    },
}

/// A background file-tree result.
pub enum FileEvent {
    /// Directory listing for the requested path.
    Listed(PathBuf, Result<Vec<DirEntry>, String>),
    /// Workspace-relative files in sorted order for one request generation.
    Files {
        generation: u64,
        result: Result<Vec<String>, String>,
    },
    /// Bounded project search results.
    ProjectSearch {
        query: String,
        generation: u64,
        result: Result<(Vec<PipeLocation>, bool), String>,
    },
    /// Bounded preview result for one workspace-relative file.
    Preview {
        path: String,
        generation: u64,
        result: Result<PreviewData, String>,
    },
    /// Git gutter marks keyed by zero-based line.
    GitDiff {
        path: PathBuf,
        revision: u64,
        result: Result<Vec<(usize, char)>, String>,
    },
    /// Bounded unified diff text and whether it was truncated.
    GitDiffText {
        relative: String,
        staged: bool,
        generation: u64,
        result: Result<(String, bool), String>,
    },
    /// Result of applying one hunk patch to the index.
    GitApplied {
        generation: u64,
        result: Result<(), String>,
    },
    /// Bounded blame text and whether it was truncated.
    GitBlameText {
        generation: u64,
        result: Result<(String, bool), String>,
    },
    /// Bounded unmerged-path list.
    GitConflictsText {
        generation: u64,
        result: Result<(String, bool), String>,
    },
    /// Bounded index-stage text for a conflicted file.
    GitConflictStageText {
        relative: String,
        stage: u8,
        generation: u64,
        result: Result<(String, bool), String>,
    },
    /// Result of resolving and staging one conflicted file.
    GitResolveConflict {
        relative: String,
        stage: u8,
        generation: u64,
        result: Result<(), ResolveConflictError>,
    },
    /// Project replace preview result.
    ProjectReplacePreview {
        generation: u64,
        result: Result<ProjectReplacePlan, String>,
    },
    /// Project replace apply result.
    ProjectReplaceApplied {
        generation: u64,
        result: Result<(), String>,
    },
    /// A move or copy completed.
    Transferred {
        from: PathBuf,
        to: PathBuf,
        copy: bool,
        result: Result<(), String>,
    },
    /// A recursive delete completed.
    Removed {
        path: PathBuf,
        result: Result<(), String>,
    },
    /// A watched file changed on disk.
    ExternalChanged { path: PathBuf },
    /// One or more watched directories changed; refresh once.
    TreeChanged { paths: Vec<PathBuf> },
    /// Directory watching is unsupported or failed; the tree is stale.
    TreeWatchUnavailable(String),
    /// Listings for a generation-scoped tree refresh.
    TreeSnapshot {
        generation: u64,
        listings: Vec<(PathBuf, Result<Vec<DirEntry>, String>)>,
    },
    /// Workspace git status keyed by absolute path.
    GitStatus {
        generation: u64,
        result: Result<HashMap<PathBuf, char>, String>,
    },
}

/// Owns the background worker and its channels.
pub struct FileWorker {
    requests: Sender<FileRequest>,
    events: Receiver<FileEvent>,
}

impl FileWorker {
    /// Starts the worker with a dedicated gateway connection.
    pub fn start(mut client: GatewayClient, workspace: PathBuf) -> Self {
        let (request_sender, requests) = mpsc::channel();
        let (event_sender, events) = mpsc::channel();
        std::thread::spawn(move || {
            let mut watched: BTreeMap<PathBuf, Option<(u64, i64)>> = BTreeMap::new();
            let mut watched_dirs: BTreeMap<PathBuf, Option<(i64, i64)>> = BTreeMap::new();
            let mut pending: BTreeMap<PathBuf, Instant> = BTreeMap::new();
            let mut pending_dirs: BTreeMap<PathBuf, Instant> = BTreeMap::new();
            let mut next_poll = Instant::now();
            let mut next_tree_poll = Instant::now();
            loop {
                match requests.recv_timeout(Duration::from_millis(50)) {
                    Ok(request) => {
                        let event = match request {
                            FileRequest::Watch(paths) => {
                                watched.clear();
                                for path in paths {
                                    let stat = client
                                        .file_stat(&relative_path(&workspace, &path))
                                        .ok()
                                        .map(|stat| (stat.size, stat.modified_ns));
                                    watched.insert(path, stat);
                                }
                                pending.clear();
                                next_poll = Instant::now() + Duration::from_millis(100);
                                continue;
                            }
                            FileRequest::WatchDirs(paths) => {
                                watched_dirs.clear();
                                let mut available = false;
                                let mut watch_error = None;
                                for path in paths.into_iter().take(MAX_TREE_WATCH_DIRS) {
                                    let result = client
                                        .read_dir_signature(&relative_path(&workspace, &path));
                                    let signature = match result {
                                        Ok(signature) => {
                                            available = true;
                                            Some((signature.modified_ns, signature.changed_ns))
                                        }
                                        Err(error) => {
                                            watch_error.get_or_insert_with(|| error.to_string());
                                            None
                                        }
                                    };
                                    watched_dirs.insert(path, signature);
                                }
                                pending_dirs.clear();
                                next_tree_poll = Instant::now() + TREE_POLL_INTERVAL;
                                if !available
                                    && let Some(error) = watch_error
                                    && event_sender
                                        .send(FileEvent::TreeWatchUnavailable(error))
                                        .is_err()
                                {
                                    break;
                                }
                                continue;
                            }
                            FileRequest::TreeSnapshot {
                                generation,
                                root,
                                expanded,
                            } => {
                                let mut listings = Vec::new();
                                listings.push((
                                    root.clone(),
                                    client
                                        .read_dir(&relative_path(&workspace, &root))
                                        .map_err(|error| error.to_string()),
                                ));
                                for path in expanded {
                                    if path == root {
                                        continue;
                                    }
                                    listings.push((
                                        path.clone(),
                                        client
                                            .read_dir(&relative_path(&workspace, &path))
                                            .map_err(|error| error.to_string()),
                                    ));
                                }
                                FileEvent::TreeSnapshot {
                                    generation,
                                    listings,
                                }
                            }
                            FileRequest::List(path) => FileEvent::Listed(
                                path.clone(),
                                client
                                    .read_dir(&relative_path(&workspace, &path))
                                    .map_err(|error| error.to_string()),
                            ),
                            FileRequest::Files { hidden, generation } => FileEvent::Files {
                                generation,
                                result: files(&mut client, &workspace, hidden),
                            },
                            FileRequest::ProjectSearch { query, generation } => {
                                FileEvent::ProjectSearch {
                                    result: project_search(&mut client, &query),
                                    query,
                                    generation,
                                }
                            }
                            FileRequest::ProjectReplacePreview {
                                old,
                                new,
                                generation,
                            } => FileEvent::ProjectReplacePreview {
                                result: project_replace_preview(&mut client, &old, &new),
                                generation,
                            },
                            FileRequest::ProjectReplaceApply { plan, generation } => {
                                FileEvent::ProjectReplaceApplied {
                                    result: apply_project_replace(&mut client, &plan),
                                    generation,
                                }
                            }
                            FileRequest::Preview { path, generation } => FileEvent::Preview {
                                result: read_preview(&mut client, &path),
                                path,
                                generation,
                            },
                            FileRequest::GitStatus { root, generation } => FileEvent::GitStatus {
                                result: git_status(&mut client, &root),
                                generation,
                            },
                            FileRequest::GitDiff {
                                path,
                                relative,
                                revision,
                            } => FileEvent::GitDiff {
                                path,
                                revision,
                                result: git_diff(&mut client, &relative),
                            },
                            FileRequest::GitDiffText {
                                relative,
                                staged,
                                generation,
                            } => {
                                let result = git_diff_text(&mut client, &relative, staged);
                                FileEvent::GitDiffText {
                                    relative,
                                    staged,
                                    generation,
                                    result,
                                }
                            }
                            FileRequest::GitApplyHunk {
                                patch,
                                reverse,
                                generation,
                            } => FileEvent::GitApplied {
                                generation,
                                result: apply_git_hunk(&mut client, &patch, reverse),
                            },
                            FileRequest::GitBlameText {
                                relative,
                                generation,
                            } => FileEvent::GitBlameText {
                                generation,
                                result: git_blame_text(&mut client, &relative),
                            },
                            FileRequest::GitConflictsText { generation } => {
                                FileEvent::GitConflictsText {
                                    generation,
                                    result: git_conflicts_text(&mut client),
                                }
                            }
                            FileRequest::Transfer { from, to, copy } => {
                                let result = if copy {
                                    client
                                        .copy(
                                            &relative_path(&workspace, &from),
                                            &relative_path(&workspace, &to),
                                        )
                                        .map_err(|error| error.to_string())
                                } else {
                                    client
                                        .rename(
                                            &relative_path(&workspace, &from),
                                            &relative_path(&workspace, &to),
                                        )
                                        .map_err(|error| error.to_string())
                                };
                                FileEvent::Transferred {
                                    from,
                                    to,
                                    copy,
                                    result,
                                }
                            }
                            FileRequest::GitConflictStageText {
                                relative,
                                stage,
                                generation,
                            } => {
                                let result = git_conflict_stage_text(&mut client, &relative, stage);
                                FileEvent::GitConflictStageText {
                                    relative,
                                    stage,
                                    generation,
                                    result,
                                }
                            }
                            FileRequest::GitResolveConflict {
                                relative,
                                stage,
                                generation,
                            } => {
                                let result = resolve_git_conflict(&mut client, &relative, stage);
                                FileEvent::GitResolveConflict {
                                    relative,
                                    stage,
                                    generation,
                                    result,
                                }
                            }
                            FileRequest::RemoveAll(path) => FileEvent::Removed {
                                result: client
                                    .remove_all(&relative_path(&workspace, &path))
                                    .map_err(|error| error.to_string()),
                                path,
                            },
                        };
                        if event_sender.send(event).is_err() {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                let now = Instant::now();
                if now >= next_tree_poll {
                    for path in poll_watched_dirs(&mut client, &workspace, &mut watched_dirs) {
                        pending_dirs.insert(path, now);
                    }
                    let ready: Vec<PathBuf> = pending_dirs
                        .iter()
                        .filter(|(_, changed)| now.duration_since(**changed) >= TREE_DEBOUNCE)
                        .map(|(path, _)| path.clone())
                        .collect();
                    if !ready.is_empty() {
                        for path in &ready {
                            pending_dirs.remove(path);
                        }
                        if event_sender
                            .send(FileEvent::TreeChanged { paths: ready })
                            .is_err()
                        {
                            return;
                        }
                    }
                    next_tree_poll = now + TREE_POLL_INTERVAL;
                }
                if now < next_poll {
                    continue;
                }
                for path in poll_watched(&mut client, &workspace, &mut watched) {
                    pending.insert(path, now);
                }
                let ready: Vec<PathBuf> = pending
                    .iter()
                    .filter(|(_, changed)| now.duration_since(**changed) >= TREE_DEBOUNCE)
                    .map(|(path, _)| path.clone())
                    .collect();
                for path in ready {
                    pending.remove(&path);
                    if event_sender
                        .send(FileEvent::ExternalChanged { path })
                        .is_err()
                    {
                        return;
                    }
                }
                next_poll = now + Duration::from_millis(100);
            }
        });
        Self {
            requests: request_sender,
            events,
        }
    }

    /// Queues one request without blocking the caller.
    pub fn request(&self, request: FileRequest) -> Result<(), String> {
        self.requests
            .send(request)
            .map_err(|_| "file worker stopped".to_owned())
    }

    /// Drains completed events.
    pub fn poll(&self) -> Vec<FileEvent> {
        self.events.try_iter().collect()
    }
}

fn relative_path(workspace: &Path, path: &Path) -> String {
    let relative = path
        .strip_prefix(workspace)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_owned());
    if relative.is_empty() {
        ".".to_owned()
    } else {
        relative
    }
}

fn poll_watched(
    client: &mut GatewayClient,
    workspace: &Path,
    watched: &mut BTreeMap<PathBuf, Option<(u64, i64)>>,
) -> Vec<PathBuf> {
    let mut changed = Vec::new();
    for (path, previous) in watched.iter_mut() {
        let current = client
            .file_stat(&relative_path(workspace, path))
            .ok()
            .map(|stat| (stat.size, stat.modified_ns));
        if current != *previous {
            *previous = current;
            changed.push(path.clone());
        }
    }
    changed
}

fn poll_watched_dirs(
    client: &mut GatewayClient,
    workspace: &Path,
    watched: &mut BTreeMap<PathBuf, Option<(i64, i64)>>,
) -> Vec<PathBuf> {
    let mut changed = Vec::new();
    for (path, previous) in watched.iter_mut() {
        let result = client.read_dir_signature(&relative_path(workspace, path));
        let current = result
            .ok()
            .map(|signature| (signature.modified_ns, signature.changed_ns));
        if current != *previous {
            *previous = current;
            changed.push(path.clone());
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::{
        FileEvent, FileRequest, FileWorker, PREVIEW_MAX_LINES, decode_preview, parse_git_diff,
        parse_git_status,
    };
    use anvil_ipc::GatewayClient;
    use std::path::Path;

    #[test]
    fn parses_git_hunks_into_line_marks() {
        let output = "@@ -1 +1 @@\n-old\n+new\n@@ -0,0 +4,2 @@\n+one\n+two\n@@ -8,2 +9,0 @@\n";
        assert_eq!(
            parse_git_diff(output),
            vec![(0, '~'), (3, '+'), (4, '+'), (8, '-')]
        );
    }

    #[test]
    fn routes_transfer_and_remove_all_through_gateway() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().to_path_buf();
        let socket = workspace.join("gateway.sock");
        let listener = anvil_ipc::Listener::bind(&socket).expect("bind");
        let methods = Arc::new(Mutex::new(Vec::new()));
        let server_methods = Arc::clone(&methods);
        let server = std::thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            for _ in 0..3 {
                let frame = connection.recv().expect("recv").expect("frame");
                let request: serde_json::Value = serde_json::from_slice(&frame).expect("request");
                server_methods
                    .lock()
                    .expect("methods")
                    .push(request["method"].as_str().expect("method").to_owned());
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "result": {"ok": true},
                });
                connection
                    .send(&serde_json::to_vec(&response).expect("response"))
                    .expect("send");
            }
        });

        let client = GatewayClient::connect(&socket, &[7; 32]).expect("client");
        let worker = FileWorker::start(client, workspace.clone());
        worker
            .request(FileRequest::Transfer {
                from: workspace.join("source.txt"),
                to: workspace.join("copy.txt"),
                copy: true,
            })
            .expect("transfer request");
        assert!(matches!(
            next_event(&worker),
            FileEvent::Transferred {
                result: Ok(()),
                copy: true,
                ..
            }
        ));
        worker
            .request(FileRequest::RemoveAll(workspace.join("source.txt")))
            .expect("remove request");
        assert!(matches!(
            next_event(&worker),
            FileEvent::Removed { result: Ok(()), .. }
        ));

        drop(worker);
        server.join().expect("server");
        assert_eq!(
            methods.lock().expect("methods").as_slice(),
            ["session.hello", "sys.copy", "sys.remove_all"]
        );
    }

    #[test]
    fn watch_emits_one_external_change_after_metadata_changes() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().to_path_buf();
        let socket = workspace.join("gateway.sock");
        let listener = anvil_ipc::Listener::bind(&socket).expect("bind");
        let stat_calls = Arc::new(Mutex::new(0usize));
        let server_calls = Arc::clone(&stat_calls);
        let server = std::thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            for _ in 0..3 {
                let frame = connection.recv().expect("recv").expect("frame");
                let request: serde_json::Value = serde_json::from_slice(&frame).expect("request");
                let result = if request["method"] == "sys.stat" {
                    let mut calls = server_calls.lock().expect("calls");
                    *calls += 1;
                    let stamp = *calls as u64;
                    serde_json::json!({"size": stamp, "modified_ns": stamp})
                } else {
                    serde_json::json!({"ok": true})
                };
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "result": result,
                });
                connection
                    .send(&serde_json::to_vec(&response).expect("response"))
                    .expect("send");
            }
        });

        let client = GatewayClient::connect(&socket, &[9; 32]).expect("client");
        let worker = FileWorker::start(client, workspace.clone());
        worker
            .request(FileRequest::Watch(vec![workspace.join("watched.txt")]))
            .expect("watch");
        assert!(matches!(
            next_event(&worker),
            FileEvent::ExternalChanged { .. }
        ));
        drop(worker);
        server.join().expect("server");
    }

    #[test]
    fn tree_watch_coalesces_directory_changes() {
        use std::collections::BTreeMap as TestMap;

        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().to_path_buf();
        let socket = workspace.join("gateway.sock");
        let listener = anvil_ipc::Listener::bind(&socket).expect("bind");
        let server = std::thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            let mut signatures: TestMap<String, i64> = TestMap::new();
            while let Ok(Some(frame)) = connection.recv() {
                let request: serde_json::Value = serde_json::from_slice(&frame).expect("request");
                let result = if request["method"] == "sys.read_dir_signature" {
                    let path = request["params"]["path"].as_str().unwrap_or("").to_owned();
                    let signature = signatures.entry(path).or_insert(0);
                    *signature = (*signature + 1).min(2);
                    serde_json::json!({
                        "modified_ns": *signature,
                        "changed_ns": *signature,
                    })
                } else {
                    serde_json::json!({"ok": true})
                };
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "result": result,
                });
                connection
                    .send(&serde_json::to_vec(&response).expect("response"))
                    .expect("send");
            }
        });

        let client = GatewayClient::connect(&socket, &[11; 32]).expect("client");
        let worker = FileWorker::start(client, workspace.clone());
        worker
            .request(FileRequest::WatchDirs(vec![
                workspace.join("a"),
                workspace.join("b"),
            ]))
            .expect("watch dirs");
        let FileEvent::TreeChanged { paths } = next_event(&worker) else {
            panic!("expected tree change");
        };
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&workspace.join("a")));
        assert!(paths.contains(&workspace.join("b")));
        drop(worker);
        server.join().expect("server");
    }

    #[test]
    fn parses_git_status_porcelain() {
        let workspace = Path::new("/ws");
        let statuses =
            parse_git_status(" M src/main.rs\0?? new.rs\0R  new.rs\0old.rs\0", workspace);
        assert_eq!(statuses.get(&workspace.join("src/main.rs")), Some(&'M'));
        assert_eq!(statuses.get(&workspace.join("new.rs")), Some(&'R'));
        assert!(!statuses.contains_key(&workspace.join("old.rs")));
    }

    #[test]
    fn tree_watch_reports_unavailable_gateway() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().to_path_buf();
        let socket = workspace.join("gateway.sock");
        let listener = anvil_ipc::Listener::bind(&socket).expect("bind");
        let server = std::thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            while let Ok(Some(frame)) = connection.recv() {
                let request: serde_json::Value = serde_json::from_slice(&frame).expect("request");
                let response = if request["method"] == "sys.read_dir_signature" {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "error": {"code": -32601, "message": "method not found"},
                    })
                } else {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "result": {"ok": true},
                    })
                };
                connection
                    .send(&serde_json::to_vec(&response).expect("response"))
                    .expect("send");
            }
        });

        let client = GatewayClient::connect(&socket, &[12; 32]).expect("client");
        let worker = FileWorker::start(client, workspace.clone());
        worker
            .request(FileRequest::WatchDirs(vec![workspace]))
            .expect("watch dirs");
        assert!(matches!(
            next_event(&worker),
            FileEvent::TreeWatchUnavailable(_)
        ));
        drop(worker);
        server.join().expect("server");
    }

    #[test]
    fn preview_decode_bounds_lines_and_rejects_binary() {
        let long = (0..=PREVIEW_MAX_LINES)
            .map(|line| format!("line {line}\r"))
            .collect::<Vec<_>>()
            .join("\n");
        let decoded = decode_preview(long.into_bytes(), false).expect("preview");
        assert_eq!(decoded.lines.len(), PREVIEW_MAX_LINES);
        assert_eq!(decoded.lines[0], "line 0");
        assert!(decoded.truncated);

        assert!(decode_preview(vec![0xff], false).is_err());
        assert!(decode_preview(b"a\0b".to_vec(), false).is_err());

        let partial = decode_preview(b"ok\xe2\x82".to_vec(), true).expect("truncated UTF-8");
        assert_eq!(partial.lines, ["ok"]);
        assert!(partial.truncated);
    }

    #[test]
    fn preview_reads_through_gateway() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().to_path_buf();
        let socket = workspace.join("gateway.sock");
        let listener = anvil_ipc::Listener::bind(&socket).expect("bind");
        let server = std::thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            for _ in 0..3 {
                let frame = connection.recv().expect("recv").expect("frame");
                let request: serde_json::Value = serde_json::from_slice(&frame).expect("request");
                let result = match request["method"].as_str().unwrap_or_default() {
                    "sys.stat" => serde_json::json!({"size": 12, "modified_ns": 0}),
                    "sys.read_file" => serde_json::json!({
                        "data": "aGVsbG8Kd29ybGQK",
                        "next_offset": 12,
                        "eof": true,
                    }),
                    _ => serde_json::json!({"ok": true}),
                };
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "result": result,
                });
                connection
                    .send(&serde_json::to_vec(&response).expect("response"))
                    .expect("send");
            }
        });

        let client = GatewayClient::connect(&socket, &[13; 32]).expect("client");
        let worker = FileWorker::start(client, workspace);
        worker
            .request(FileRequest::Preview {
                path: "src/main.rs".to_owned(),
                generation: 7,
            })
            .expect("preview");
        let FileEvent::Preview {
            path,
            generation,
            result,
        } = next_event(&worker)
        else {
            panic!("expected preview");
        };
        assert_eq!(path, "src/main.rs");
        assert_eq!(generation, 7);
        let preview = result.expect("preview result");
        assert_eq!(preview.lines, ["hello", "world", ""]);
        assert!(!preview.truncated);
        drop(worker);
        server.join().expect("server");
    }

    fn next_event(worker: &FileWorker) -> FileEvent {
        for _ in 0..1000 {
            if let Some(event) = worker.poll().into_iter().next() {
                return event;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("worker event timed out");
    }
}

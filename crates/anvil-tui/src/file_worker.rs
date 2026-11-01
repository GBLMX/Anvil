//! Background workspace directory and file enumeration.
//!
//! The UI thread never performs a gateway directory read. One worker owns a
//! dedicated authenticated connection; requests and results use mpsc.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anvil_ipc::{DirEntry, GatewayClient};

const FILE_LIMIT: usize = 5000;
pub(crate) const MAX_TREE_WATCH_DIRS: usize = 256;
const TREE_POLL_INTERVAL: Duration = Duration::from_millis(500);
const TREE_DEBOUNCE: Duration = Duration::from_millis(500);

/// A background file-tree request.
pub enum FileRequest {
    /// List one directory.
    List(PathBuf),
    /// Enumerate workspace files with `rg --files`, or a bounded fallback.
    Files { hidden: bool },
    /// Compare one workspace file with HEAD for gutter marks.
    GitDiff {
        path: PathBuf,
        relative: String,
        revision: u64,
    },
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
}

/// A background file-tree result.
pub enum FileEvent {
    /// Directory listing for the requested path.
    Listed(PathBuf, Result<Vec<DirEntry>, String>),
    /// Workspace-relative files in sorted order.
    Files(Result<Vec<String>, String>),
    /// Git gutter marks keyed by zero-based line.
    GitDiff {
        path: PathBuf,
        revision: u64,
        result: Result<Vec<(usize, char)>, String>,
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
                            FileRequest::Files { hidden } => {
                                FileEvent::Files(files(&mut client, &workspace, hidden))
                            }
                            FileRequest::GitDiff {
                                path,
                                relative,
                                revision,
                            } => FileEvent::GitDiff {
                                path,
                                revision,
                                result: git_diff(&mut client, &relative),
                            },
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

fn files(
    client: &mut GatewayClient,
    workspace: &Path,
    hidden: bool,
) -> Result<Vec<String>, String> {
    if let Some(tool) = rg_tool() {
        let mut args = vec!["--files".to_owned()];
        if hidden {
            args.push("--hidden".to_owned());
        }
        for excluded in [".git", "target", ".cache", ".cargo", ".rustup"] {
            args.push("--glob".to_owned());
            args.push(format!("!{excluded}/**"));
        }
        if let Ok(result) = client.exec(&tool, &args, 3000)
            && result.exit_code == 0
            && !result.timed_out
        {
            let mut files: Vec<String> = result
                .stdout
                .lines()
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .filter(|line| {
                    ![".git/", "target/", ".cache/", ".cargo/", ".rustup/"]
                        .iter()
                        .any(|prefix| line.starts_with(prefix))
                })
                .collect();
            files.sort();
            files.dedup();
            if files.len() <= FILE_LIMIT {
                return Ok(files);
            }
        }
    }
    let mut files = Vec::new();
    collect_fallback(client, workspace, workspace, "", hidden, &mut files)?;
    files.sort();
    files.dedup();
    Ok(files)
}

fn rg_tool() -> Option<String> {
    if let Some(path) = std::env::var_os("ANVIL_RG") {
        return Some(path.to_string_lossy().into_owned());
    }
    ["/usr/bin/rg", "/bin/rg"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
        .map(ToOwned::to_owned)
}

fn collect_fallback(
    client: &mut GatewayClient,
    root: &Path,
    directory: &Path,
    prefix: &str,
    hidden: bool,
    out: &mut Vec<String>,
) -> Result<(), String> {
    if out.len() >= FILE_LIMIT {
        return Ok(());
    }
    let relative = directory
        .strip_prefix(root)
        .unwrap_or(directory)
        .to_string_lossy()
        .into_owned();
    let relative = if relative.is_empty() {
        ".".to_owned()
    } else {
        relative
    };
    let entries = client
        .read_dir(&relative)
        .map_err(|error| error.to_string())?;
    for entry in entries {
        if [".git", "target", ".cache", ".cargo", ".rustup"].contains(&entry.name.as_str()) {
            continue;
        }
        if !hidden && entry.name.starts_with('.') {
            continue;
        }
        let child = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{prefix}/{}", entry.name)
        };
        if entry.is_dir {
            collect_fallback(
                client,
                root,
                &directory.join(&entry.name),
                &child,
                hidden,
                out,
            )?;
        } else {
            out.push(child);
        }
        if out.len() >= FILE_LIMIT {
            break;
        }
    }
    Ok(())
}

fn git_tool() -> String {
    std::env::var_os("ANVIL_GIT")
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/usr/bin/git".to_owned())
}

fn git_diff(client: &mut GatewayClient, relative: &str) -> Result<Vec<(usize, char)>, String> {
    let result = client
        .exec(
            &git_tool(),
            &[
                "--no-pager".to_owned(),
                "diff".to_owned(),
                "--no-ext-diff".to_owned(),
                "--no-textconv".to_owned(),
                "--no-color".to_owned(),
                "--unified=0".to_owned(),
                "HEAD".to_owned(),
                "--".to_owned(),
                relative.to_owned(),
            ],
            3000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("git diff timed out".to_owned());
    }
    if result.exit_code != 0 {
        return Err(result.stderr);
    }
    Ok(parse_git_diff(&result.stdout))
}

fn parse_git_diff(output: &str) -> Vec<(usize, char)> {
    let mut marks = Vec::new();
    for line in output.lines() {
        if !line.starts_with("@@ ") {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(old_range) = fields.nth(1) else {
            continue;
        };
        let Some(new_range) = fields.next() else {
            continue;
        };
        let Some(old_range) = old_range.strip_prefix('-') else {
            continue;
        };
        let Some(new_range) = new_range.strip_prefix('+') else {
            continue;
        };
        let mut old_parts = old_range.split(',');
        let _old_start = old_parts.next();
        let old_count = old_parts
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
        let mut new_parts = new_range.split(',');
        let Some(start) = new_parts
            .next()
            .and_then(|value| value.parse::<usize>().ok())
        else {
            continue;
        };
        let count = new_parts
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
        if count == 0 {
            marks.push((start.saturating_sub(1), '-'));
        } else {
            let marker = if old_count == 0 { '+' } else { '~' };
            for offset in 0..count {
                marks.push((start.saturating_sub(1) + offset, marker));
            }
        }
    }
    marks.sort_unstable();
    marks.dedup();
    marks
}

#[cfg(test)]
mod tests {
    use super::{FileEvent, FileRequest, FileWorker, parse_git_diff};
    use anvil_ipc::GatewayClient;

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

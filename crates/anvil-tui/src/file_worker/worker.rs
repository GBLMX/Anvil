//! File-worker loop, watchers, and request dispatch.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    mpsc::{self, Receiver, Sender},
};
use std::time::{Duration, Instant};

use anvil_ipc::GatewayClient;

use super::preview::read_preview;
use super::search::{apply_project_replace, files, project_replace_preview, project_search};
use super::{FileEvent, FileRequest, MAX_TREE_WATCH_DIRS, TREE_DEBOUNCE, TREE_POLL_INTERVAL};
use anvil_module::git::GitService;
use anvil_module::workspace_index::WorkspaceIndexService;

const GIT_UNAVAILABLE: &str = "git module is not installed";

fn git_status(
    git: Option<&dyn GitService>,
    client: &mut GatewayClient,
    root: &Path,
) -> Result<std::collections::HashMap<PathBuf, char>, String> {
    git.ok_or_else(|| GIT_UNAVAILABLE.to_owned())?
        .git_status(client, root)
}

fn git_diff(
    git: Option<&dyn GitService>,
    client: &mut GatewayClient,
    relative: &str,
) -> Result<Vec<(usize, char)>, String> {
    git.ok_or_else(|| GIT_UNAVAILABLE.to_owned())?
        .git_diff(client, relative)
}

fn git_diff_text(
    git: Option<&dyn GitService>,
    client: &mut GatewayClient,
    relative: &str,
    staged: bool,
) -> Result<(String, bool), String> {
    git.ok_or_else(|| GIT_UNAVAILABLE.to_owned())?
        .git_diff_text(client, relative, staged)
}

fn git_blame_text(
    git: Option<&dyn GitService>,
    client: &mut GatewayClient,
    relative: &str,
) -> Result<(String, bool), String> {
    git.ok_or_else(|| GIT_UNAVAILABLE.to_owned())?
        .git_blame_text(client, relative)
}

fn git_conflicts_text(
    git: Option<&dyn GitService>,
    client: &mut GatewayClient,
) -> Result<(String, bool), String> {
    git.ok_or_else(|| GIT_UNAVAILABLE.to_owned())?
        .git_conflicts_text(client)
}

fn git_conflict_stage_text(
    git: Option<&dyn GitService>,
    client: &mut GatewayClient,
    relative: &str,
    stage: u8,
) -> Result<(String, bool), String> {
    git.ok_or_else(|| GIT_UNAVAILABLE.to_owned())?
        .git_conflict_stage_text(client, relative, stage)
}

fn resolve_git_conflict(
    git: Option<&dyn GitService>,
    client: &mut GatewayClient,
    relative: &str,
    stage: u8,
) -> Result<(), anvil_module::git::ResolveConflictError> {
    git.ok_or_else(|| anvil_module::git::ResolveConflictError::before_write(GIT_UNAVAILABLE))?
        .resolve_git_conflict(client, relative, stage)
}

fn apply_git_hunk(
    git: Option<&dyn GitService>,
    client: &mut GatewayClient,
    patch: &str,
    reverse: bool,
) -> Result<(), String> {
    git.ok_or_else(|| GIT_UNAVAILABLE.to_owned())?
        .apply_git_hunk(client, patch, reverse)
}

/// Owns the background worker and its channels.
pub struct FileWorker {
    requests: Sender<FileRequest>,
    events: Receiver<FileEvent>,
}

impl FileWorker {
    /// Starts the worker with a dedicated gateway connection.
    #[cfg(test)]
    pub fn start(client: GatewayClient, workspace: PathBuf) -> Self {
        #[cfg(feature = "official-git")]
        {
            Self::start_with_services(
                client,
                workspace,
                Some(Arc::new(anvil_mod_git::SystemGitService)),
                None,
            )
        }
        #[cfg(not(feature = "official-git"))]
        {
            Self::start_with_services(client, workspace, None, None)
        }
    }

    #[cfg(test)]
    pub(crate) fn start_with_git_service(
        client: GatewayClient,
        workspace: PathBuf,
        git: Option<Arc<dyn GitService>>,
    ) -> Self {
        Self::start_with_services(client, workspace, git, None)
    }

    pub(crate) fn start_with_services(
        mut client: GatewayClient,
        workspace: PathBuf,
        git: Option<Arc<dyn GitService>>,
        index: Option<Arc<dyn WorkspaceIndexService>>,
    ) -> Self {
        let worker_git = git.clone();
        let worker_index = index.clone();
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
                            FileRequest::Files {
                                hidden,
                                generation,
                                cancel,
                            } => FileEvent::Files {
                                generation,
                                result: if let Some(index) = worker_index.as_deref() {
                                    index
                                        .build(&mut client, &workspace, hidden, generation, &cancel)
                                        .map(|snapshot| snapshot.paths)
                                        .map_err(|error| error.to_string())
                                } else {
                                    files(&mut client, &workspace, hidden)
                                },
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
                                result: git_status(worker_git.as_deref(), &mut client, &root),
                                generation,
                            },
                            FileRequest::GitDiff {
                                path,
                                relative,
                                revision,
                            } => FileEvent::GitDiff {
                                path,
                                revision,
                                result: git_diff(worker_git.as_deref(), &mut client, &relative),
                            },
                            FileRequest::GitDiffText {
                                relative,
                                staged,
                                generation,
                            } => {
                                let result = git_diff_text(
                                    worker_git.as_deref(),
                                    &mut client,
                                    &relative,
                                    staged,
                                );
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
                                result: apply_git_hunk(
                                    worker_git.as_deref(),
                                    &mut client,
                                    &patch,
                                    reverse,
                                ),
                            },
                            FileRequest::GitBlameText {
                                relative,
                                generation,
                            } => FileEvent::GitBlameText {
                                generation,
                                result: git_blame_text(
                                    worker_git.as_deref(),
                                    &mut client,
                                    &relative,
                                ),
                            },
                            FileRequest::GitConflictsText { root, generation } => {
                                FileEvent::GitConflictsText {
                                    root,
                                    generation,
                                    result: git_conflicts_text(worker_git.as_deref(), &mut client),
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
                                let result = git_conflict_stage_text(
                                    worker_git.as_deref(),
                                    &mut client,
                                    &relative,
                                    stage,
                                );
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
                                let result = resolve_git_conflict(
                                    worker_git.as_deref(),
                                    &mut client,
                                    &relative,
                                    stage,
                                );
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

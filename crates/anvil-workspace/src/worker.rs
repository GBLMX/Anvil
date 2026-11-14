//! File-worker loop, watchers, and request dispatch.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    mpsc::{self, SyncSender},
};
use std::time::{Duration, Instant};

use anvil_ipc::{DirEntry, GatewayClient};
use anvil_task::{DataError, TaskContext, TaskEvent as TaskGroupEvent, TaskGroup, TaskGroupConfig};

use super::preview::{PreviewData, read_preview};
use super::search::{apply_project_replace, files, project_replace_preview, project_search};
use super::{
    FileEvent, FileRequest, MAX_TREE_WATCH_DIRS, ProjectReplacePlan, TREE_DEBOUNCE,
    TREE_POLL_INTERVAL,
};
use crate::PipeLocation;
use anvil_module::git::GitService;
use anvil_module::workspace_index::WorkspaceIndexService;

const GIT_UNAVAILABLE: &str = "git module is not installed";
const FILE_WORKER_REQUEST_CAPACITY: usize = 256;
const FILE_WORKER_EVENT_CAPACITY: usize = 1024;
const FILE_WORKER_POLL_EVENTS: usize = 256;
const FILE_WORKER_SHUTDOWN: Duration = Duration::from_millis(250);

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

/// Owns the background worker and its bounded channels.
pub struct FileWorker {
    requests: SyncSender<FileRequest>,
    group: Option<Mutex<TaskGroup<FileEvent>>>,
    interrupt: Option<std::os::unix::net::UnixStream>,
}

impl FileWorker {
    /// Starts the worker with a dedicated gateway connection.
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

    pub fn start_with_services(
        mut client: GatewayClient,
        workspace: PathBuf,
        git: Option<Arc<dyn GitService>>,
        index: Option<Arc<dyn WorkspaceIndexService>>,
    ) -> Self {
        let worker_git = git.clone();
        let worker_index = index.clone();
        let (request_sender, requests) = mpsc::sync_channel(FILE_WORKER_REQUEST_CAPACITY);
        let config = TaskGroupConfig {
            global_data_event_capacity: FILE_WORKER_EVENT_CAPACITY,
            per_task_capacity: FILE_WORKER_EVENT_CAPACITY,
            max_events_per_poll: FILE_WORKER_POLL_EVENTS,
            ..TaskGroupConfig::default()
        };
        let interrupt = client.try_clone_stream().ok();
        let mut group = TaskGroup::with_config(config);
        let spawn = group.spawn("file worker", move |context| {
            let mut watched: BTreeMap<PathBuf, Option<(u64, i64)>> = BTreeMap::new();
            let mut watched_dirs: BTreeMap<PathBuf, Option<(i64, i64)>> = BTreeMap::new();
            let mut pending: BTreeMap<PathBuf, Instant> = BTreeMap::new();
            let mut pending_dirs: BTreeMap<PathBuf, Instant> = BTreeMap::new();
            let mut next_poll = Instant::now();
            let mut next_tree_poll = Instant::now();
            loop {
                if context.is_cancelled() {
                    break;
                }
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
                                    && !send_event(&context, FileEvent::TreeWatchUnavailable(error))
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
                            FileRequest::ListCompletion { path, generation } => {
                                FileEvent::CompletionListed {
                                    generation,
                                    result: client
                                        .read_dir(&relative_path(&workspace, &path))
                                        .map_err(|error| error.to_string()),
                                }
                            }
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
                            FileRequest::Format {
                                path,
                                text,
                                language_id,
                                revision,
                                generation,
                            } => FileEvent::Format {
                                result: super::format::format_text(
                                    &mut client,
                                    &language_id,
                                    &text,
                                ),
                                path,
                                revision,
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
                        if !send_event(&context, event) {
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
                        if !send_event(&context, FileEvent::TreeChanged { paths: ready }) {
                            break;
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
                    if !send_event(&context, FileEvent::ExternalChanged { path }) {
                        break;
                    }
                }
                next_poll = now + Duration::from_millis(100);
            }
            Ok(())
        });
        let group = match spawn {
            Ok(_) => Some(Mutex::new(group)),
            Err(_) => None,
        };
        Self {
            requests: request_sender,
            group,
            interrupt,
        }
    }

    /// Queues one request without blocking the caller.
    pub fn request(&self, request: FileRequest) -> Result<(), String> {
        match self.requests.try_send(request) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Err("file worker busy".to_owned()),
            Err(mpsc::TrySendError::Disconnected(_)) => Err("file worker stopped".to_owned()),
        }
    }

    /// Drains completed events.
    pub fn poll(&self) -> Vec<FileEvent> {
        let Some(group) = self.group.as_ref() else {
            return Vec::new();
        };
        let mut group = group
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        group
            .poll()
            .events
            .into_iter()
            .filter_map(|event| match event {
                TaskGroupEvent::Data { data, .. } => Some(data),
                _ => None,
            })
            .collect()
    }
}

impl Drop for FileWorker {
    fn drop(&mut self) {
        if let Some(interrupt) = &self.interrupt {
            let _ = interrupt.shutdown(std::net::Shutdown::Both);
        }
        let Some(group) = self.group.as_mut() else {
            return;
        };
        let group = group
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = group.shutdown(FILE_WORKER_SHUTDOWN);
    }
}

fn file_event_cost(event: &FileEvent) -> usize {
    let mut bytes = std::mem::size_of_val(event);
    match event {
        FileEvent::Listed(path, result) => {
            add_bytes(&mut bytes, path.as_os_str().len());
            add_bytes(
                &mut bytes,
                result_bytes(result, |entries| dir_entries_bytes(entries)),
            );
        }
        FileEvent::CompletionListed { result, .. } => {
            add_bytes(
                &mut bytes,
                result_bytes(result, |entries| dir_entries_bytes(entries)),
            );
        }
        FileEvent::Files { result, .. } => {
            add_bytes(
                &mut bytes,
                result_bytes(result, |values| string_vec_bytes(values)),
            );
        }
        FileEvent::ProjectSearch { query, result, .. } => {
            add_bytes(&mut bytes, query.len());
            add_bytes(
                &mut bytes,
                result_bytes(result, |(locations, _)| {
                    locations
                        .iter()
                        .map(|location| {
                            location.path.len()
                                + location.message.len()
                                + std::mem::size_of::<PipeLocation>()
                        })
                        .sum()
                }),
            );
        }
        FileEvent::Preview { path, result, .. } => {
            add_bytes(&mut bytes, path.len());
            add_bytes(&mut bytes, result_bytes(result, preview_bytes));
        }
        FileEvent::Format { path, result, .. } => {
            add_bytes(&mut bytes, path.as_os_str().len());
            add_bytes(
                &mut bytes,
                result_bytes(result, |text| text.as_ref().map_or(0, String::len)),
            );
        }
        FileEvent::GitDiff { path, result, .. } => {
            add_bytes(&mut bytes, path.as_os_str().len());
            add_bytes(
                &mut bytes,
                result_bytes(result, |marks| {
                    marks.len() * std::mem::size_of::<(usize, char)>()
                }),
            );
        }
        FileEvent::GitDiffText {
            relative, result, ..
        } => {
            add_bytes(&mut bytes, relative.len());
            add_bytes(&mut bytes, result_bytes(result, text_pair_bytes));
        }
        FileEvent::GitApplied { result, .. } | FileEvent::ProjectReplaceApplied { result, .. } => {
            add_bytes(&mut bytes, error_result_bytes(result));
        }
        FileEvent::GitBlameText { result, .. } => {
            add_bytes(&mut bytes, result_bytes(result, text_pair_bytes));
        }
        FileEvent::GitConflictsText { root, result, .. } => {
            add_bytes(&mut bytes, root.as_os_str().len());
            add_bytes(&mut bytes, result_bytes(result, text_pair_bytes));
        }
        FileEvent::GitConflictStageText {
            relative, result, ..
        } => {
            add_bytes(&mut bytes, relative.len());
            add_bytes(&mut bytes, result_bytes(result, text_pair_bytes));
        }
        FileEvent::GitResolveConflict {
            relative, result, ..
        } => {
            add_bytes(&mut bytes, relative.len());
            add_bytes(&mut bytes, std::mem::size_of_val(result));
        }
        FileEvent::ProjectReplacePreview { result, .. } => {
            add_bytes(&mut bytes, result_bytes(result, plan_bytes));
        }
        FileEvent::Transferred {
            from, to, result, ..
        } => {
            add_bytes(&mut bytes, from.as_os_str().len() + to.as_os_str().len());
            add_bytes(&mut bytes, error_result_bytes(result));
        }
        FileEvent::Removed { path, result } => {
            add_bytes(&mut bytes, path.as_os_str().len());
            add_bytes(&mut bytes, error_result_bytes(result));
        }
        FileEvent::ExternalChanged { path } => {
            add_bytes(&mut bytes, path.as_os_str().len());
        }
        FileEvent::TreeChanged { paths } => {
            add_bytes(
                &mut bytes,
                paths
                    .iter()
                    .map(|path| path.as_os_str().len() + std::mem::size_of::<PathBuf>())
                    .sum(),
            );
        }
        FileEvent::TreeWatchUnavailable(message) | FileEvent::Oversized { message } => {
            add_bytes(&mut bytes, message.len());
        }
        FileEvent::TreeSnapshot { listings, .. } => {
            add_bytes(
                &mut bytes,
                listings
                    .iter()
                    .map(|(path, result)| {
                        path.as_os_str().len()
                            + std::mem::size_of::<(PathBuf, Result<Vec<DirEntry>, String>)>()
                            + result_bytes(result, |entries| dir_entries_bytes(entries))
                    })
                    .sum(),
            );
        }
        FileEvent::GitStatus { result, .. } => {
            add_bytes(
                &mut bytes,
                result_bytes(result, |statuses| {
                    statuses
                        .keys()
                        .map(|path| path.as_os_str().len() + std::mem::size_of::<(PathBuf, char)>())
                        .sum()
                }),
            );
        }
    }
    bytes
}

fn add_bytes(total: &mut usize, extra: usize) {
    *total = total.saturating_add(extra);
}

fn result_bytes<T>(result: &Result<T, String>, used: impl FnOnce(&T) -> usize) -> usize {
    match result {
        Ok(value) => used(value),
        Err(error) => error.len(),
    }
}

fn error_result_bytes(result: &Result<(), String>) -> usize {
    result.as_ref().err().map_or(0, String::len)
}

fn text_pair_bytes(value: &(String, bool)) -> usize {
    value.0.len()
}

fn string_vec_bytes(values: &[String]) -> usize {
    values
        .iter()
        .map(|value| value.len() + std::mem::size_of::<String>())
        .sum()
}

fn dir_entries_bytes(entries: &[DirEntry]) -> usize {
    entries
        .iter()
        .map(|entry| entry.name.len() + std::mem::size_of::<DirEntry>())
        .sum()
}

fn preview_bytes(preview: &PreviewData) -> usize {
    preview
        .lines
        .iter()
        .map(|line| line.len() + std::mem::size_of::<String>())
        .sum()
}

fn plan_bytes(plan: &ProjectReplacePlan) -> usize {
    plan.old.len()
        + plan.new.len()
        + plan
            .files
            .iter()
            .map(|file| {
                file.relative.len()
                    + file.hash.len()
                    + std::mem::size_of::<super::ProjectReplaceFile>()
            })
            .sum::<usize>()
}
fn send_event(context: &TaskContext<FileEvent>, event: FileEvent) -> bool {
    let bytes = file_event_cost(&event);
    if bytes > context.data_byte_capacity() {
        let message = format!(
            "file worker event exceeded {} byte budget",
            context.data_byte_capacity()
        );
        return send_event_inner(context, FileEvent::Oversized { message }, None);
    }
    send_event_inner(context, event, Some(bytes))
}

fn send_event_inner(
    context: &TaskContext<FileEvent>,
    mut event: FileEvent,
    cost: Option<usize>,
) -> bool {
    loop {
        if context.is_cancelled() {
            return false;
        }
        let result = match cost {
            Some(bytes) => context.send_data_with_cost(event, bytes),
            None => context.send_data(event),
        };
        match result {
            Ok(()) => return true,
            Err(DataError::Closed(_)) => return false,
            Err(DataError::TaskFull(returned))
            | Err(DataError::GlobalFull(returned))
            | Err(DataError::GlobalBytesFull(returned)) => {
                event = returned;
                std::thread::sleep(Duration::from_millis(1));
            }
        }
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
mod cost_tests {
    use super::*;

    #[test]
    fn event_cost_includes_heap_payload() {
        let short = FileEvent::ExternalChanged {
            path: PathBuf::from("a"),
        };
        let long = FileEvent::TreeChanged {
            paths: vec![PathBuf::from("x".repeat(4096))],
        };
        assert!(file_event_cost(&long) > file_event_cost(&short));
        assert!(
            file_event_cost(&FileEvent::GitApplied {
                generation: 0,
                result: Err("x".repeat(4096)),
            }) > std::mem::size_of::<FileEvent>()
        );
    }

    #[test]
    fn oversized_event_is_reported_without_stopping() {
        let config = TaskGroupConfig {
            global_data_event_capacity: 8,
            global_data_byte_capacity: 1,
            per_task_capacity: 8,
            ..TaskGroupConfig::default()
        };
        let mut group = TaskGroup::<FileEvent>::with_config(config);
        group
            .spawn("overflow", |context| {
                if !send_event(
                    &context,
                    FileEvent::ExternalChanged {
                        path: PathBuf::from("x"),
                    },
                ) {
                    return Err("oversized event was not reported".to_owned());
                }
                Ok(())
            })
            .expect("spawn");
        let mut saw_oversized = false;
        for _ in 0..100 {
            let batch = group.poll();
            saw_oversized = batch.events.iter().any(|event| {
                matches!(
                    event,
                    TaskGroupEvent::Data {
                        data: FileEvent::Oversized { .. },
                        ..
                    }
                )
            });
            if saw_oversized {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(saw_oversized);
    }
}

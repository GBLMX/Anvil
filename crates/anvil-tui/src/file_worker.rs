//! Background workspace directory and file enumeration.
//!
//! The UI thread never performs a gateway directory read. One worker owns a
//! dedicated authenticated connection; requests and results use mpsc.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use anvil_ipc::{DirEntry, GatewayClient};

const FILE_LIMIT: usize = 5000;

/// A background file-tree request.
pub enum FileRequest {
    /// List one directory.
    List(PathBuf),
    /// Enumerate workspace files with `rg --files`, or a bounded fallback.
    Files,
}

/// A background file-tree result.
pub enum FileEvent {
    /// Directory listing for the requested path.
    Listed(PathBuf, Result<Vec<DirEntry>, String>),
    /// Workspace-relative files in sorted order.
    Files(Result<Vec<String>, String>),
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
            while let Ok(request) = requests.recv() {
                let event = match request {
                    FileRequest::List(path) => {
                        let relative = path
                            .strip_prefix(&workspace)
                            .map(|path| path.to_string_lossy().into_owned())
                            .unwrap_or_else(|_| ".".to_owned());
                        let relative = if relative.is_empty() {
                            ".".to_owned()
                        } else {
                            relative
                        };
                        FileEvent::Listed(
                            path,
                            client
                                .read_dir(&relative)
                                .map_err(|error| error.to_string()),
                        )
                    }
                    FileRequest::Files => FileEvent::Files(files(&mut client, &workspace)),
                };
                if event_sender.send(event).is_err() {
                    break;
                }
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

fn files(client: &mut GatewayClient, workspace: &Path) -> Result<Vec<String>, String> {
    if let Some(tool) = rg_tool() {
        let args = [
            "--files".to_owned(),
            "--hidden".to_owned(),
            "--glob".to_owned(),
            "!.git/**".to_owned(),
            "--glob".to_owned(),
            "!target/**".to_owned(),
        ];
        if let Ok(result) = client.exec(&tool, &args, 3000)
            && result.exit_code == 0
            && !result.timed_out
        {
            let mut files: Vec<String> = result
                .stdout
                .lines()
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .filter(|line| !line.starts_with(".git/") && !line.starts_with("target/"))
                .collect();
            files.sort();
            files.dedup();
            if files.len() <= FILE_LIMIT {
                return Ok(files);
            }
        }
    }
    let mut files = Vec::new();
    collect_fallback(client, workspace, workspace, "", &mut files)?;
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
        let child = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{prefix}/{}", entry.name)
        };
        if entry.is_dir {
            collect_fallback(client, root, &directory.join(&entry.name), &child, out)?;
        } else {
            out.push(child);
        }
        if out.len() >= FILE_LIMIT {
            break;
        }
    }
    Ok(())
}

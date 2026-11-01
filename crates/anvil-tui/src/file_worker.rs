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
    use super::parse_git_diff;

    #[test]
    fn parses_git_hunks_into_line_marks() {
        let output = "@@ -1 +1 @@\n-old\n+new\n@@ -0,0 +4,2 @@\n+one\n+two\n@@ -8,2 +9,0 @@\n";
        assert_eq!(
            parse_git_diff(output),
            vec![(0, '~'), (3, '+'), (4, '+'), (8, '-')]
        );
    }
}

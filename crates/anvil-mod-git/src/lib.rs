//! Trusted Git status, diff, blame, conflict, and hunk helpers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anvil_ipc::GatewayClient;

pub use anvil_module::git::{GitService, ResolveConflictError};

/// Gateway-backed Git service used by the standard edition.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemGitService;

impl GitService for SystemGitService {
    fn git_status(
        &self,
        client: &mut GatewayClient,
        root: &Path,
    ) -> Result<HashMap<PathBuf, char>, String> {
        git_status(client, root)
    }

    fn git_diff(
        &self,
        client: &mut GatewayClient,
        relative: &str,
    ) -> Result<Vec<(usize, char)>, String> {
        git_diff(client, relative)
    }

    fn git_diff_text(
        &self,
        client: &mut GatewayClient,
        relative: &str,
        staged: bool,
    ) -> Result<(String, bool), String> {
        git_diff_text(client, relative, staged)
    }

    fn git_blame_text(
        &self,
        client: &mut GatewayClient,
        relative: &str,
    ) -> Result<(String, bool), String> {
        git_blame_text(client, relative)
    }

    fn git_conflicts_text(&self, client: &mut GatewayClient) -> Result<(String, bool), String> {
        git_conflicts_text(client)
    }

    fn git_conflict_stage_text(
        &self,
        client: &mut GatewayClient,
        relative: &str,
        stage: u8,
    ) -> Result<(String, bool), String> {
        git_conflict_stage_text(client, relative, stage)
    }

    fn resolve_git_conflict(
        &self,
        client: &mut GatewayClient,
        relative: &str,
        stage: u8,
    ) -> Result<(), ResolveConflictError> {
        resolve_git_conflict(client, relative, stage)
    }

    fn apply_git_hunk(
        &self,
        client: &mut GatewayClient,
        patch: &str,
        reverse: bool,
    ) -> Result<(), String> {
        apply_git_hunk(client, patch, reverse)
    }
}

pub fn git_status(
    client: &mut GatewayClient,
    workspace: &Path,
) -> Result<HashMap<PathBuf, char>, String> {
    let result = client
        .exec(
            &git_tool(),
            &[
                "--no-pager".to_owned(),
                "status".to_owned(),
                "--porcelain=v1".to_owned(),
                "--relative".to_owned(),
                "-z".to_owned(),
            ],
            3000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("git status timed out".to_owned());
    }
    if result.exit_code > 1 {
        return Err(result.stderr);
    }
    Ok(parse_git_status(&result.stdout, workspace))
}

pub fn parse_git_status(output: &str, workspace: &Path) -> HashMap<PathBuf, char> {
    let mut statuses = HashMap::new();
    let mut entries = output
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .peekable();
    while let Some(entry) = entries.next() {
        if entry.len() < 3 {
            continue;
        }
        let code = &entry[..2];
        let path = &entry[3..];
        if path.is_empty() {
            continue;
        }
        let marker = if code.contains('?') || code.contains('A') {
            'A'
        } else if code.contains('D') {
            'D'
        } else if code.contains('R') {
            'R'
        } else if code.contains('C') {
            'C'
        } else {
            'M'
        };
        statuses.insert(workspace.join(path), marker);
        if code.contains('R') || code.contains('C') {
            let _ = entries.next();
        }
    }
    statuses
}

fn git_tool() -> String {
    std::env::var_os("ANVIL_GIT")
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/usr/bin/git".to_owned())
}

pub fn git_diff(client: &mut GatewayClient, relative: &str) -> Result<Vec<(usize, char)>, String> {
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

const MAX_GIT_DIFF_BYTES: usize = 512 * 1024;

pub fn git_diff_text(
    client: &mut GatewayClient,
    relative: &str,
    staged: bool,
) -> Result<(String, bool), String> {
    let mut args = vec!["--no-pager".to_owned(), "diff".to_owned()];
    if staged {
        args.push("--cached".to_owned());
    }
    args.extend([
        "--no-ext-diff".to_owned(),
        "--no-textconv".to_owned(),
        "--no-color".to_owned(),
        "--unified=3".to_owned(),
        "HEAD".to_owned(),
        "--".to_owned(),
        relative.to_owned(),
    ]);
    let result = client
        .exec(&git_tool(), &args, 5000)
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("git diff timed out".to_owned());
    }
    if result.exit_code != 0 {
        return Err(result.stderr);
    }
    let mut text = result.stdout;
    let mut truncated = result.output_truncated;
    if text.len() > MAX_GIT_DIFF_BYTES {
        let mut end = MAX_GIT_DIFF_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        truncated = true;
    }
    if text.is_empty() {
        text.push_str("no changes relative to HEAD\n");
    }
    Ok((text, truncated))
}

pub fn git_blame_text(
    client: &mut GatewayClient,
    relative: &str,
) -> Result<(String, bool), String> {
    let result = client
        .exec(
            &git_tool(),
            &[
                "--no-pager".to_owned(),
                "blame".to_owned(),
                "--date=short".to_owned(),
                "--".to_owned(),
                relative.to_owned(),
            ],
            5000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("git blame timed out".to_owned());
    }
    if result.exit_code != 0 {
        return Err(result.stderr);
    }
    let mut text = result.stdout;
    let mut truncated = result.output_truncated;
    if text.len() > MAX_GIT_DIFF_BYTES {
        let mut end = MAX_GIT_DIFF_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        truncated = true;
    }
    if text.is_empty() {
        text.push_str("no blame data\n");
    }
    Ok((text, truncated))
}

pub fn git_conflicts_text(client: &mut GatewayClient) -> Result<(String, bool), String> {
    let result = client
        .exec(
            &git_tool(),
            &[
                "--no-pager".to_owned(),
                "diff".to_owned(),
                "--name-status".to_owned(),
                "--diff-filter=U".to_owned(),
            ],
            3000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("git conflicts timed out".to_owned());
    }
    if result.exit_code != 0 {
        return Err(result.stderr);
    }
    let mut text = result.stdout;
    let mut truncated = result.output_truncated;
    if text.len() > MAX_GIT_DIFF_BYTES {
        let mut end = MAX_GIT_DIFF_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        truncated = true;
    }
    if text.is_empty() {
        text.push_str("no conflicts\n");
    }
    Ok((text, truncated))
}

pub fn git_conflict_stage_text(
    client: &mut GatewayClient,
    relative: &str,
    stage: u8,
) -> Result<(String, bool), String> {
    if !(1..=3).contains(&stage) {
        return Err("invalid conflict stage".to_owned());
    }
    let spec = format!(":{stage}:{relative}");
    let result = client
        .exec(
            &git_tool(),
            &["--no-pager".to_owned(), "show".to_owned(), spec],
            5000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("git show timed out".to_owned());
    }
    if result.exit_code != 0 {
        return Err(result.stderr);
    }
    let mut text = result.stdout;
    let mut truncated = result.output_truncated;
    if text.len() > MAX_GIT_DIFF_BYTES {
        let mut end = MAX_GIT_DIFF_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        truncated = true;
    }
    if text.is_empty() {
        text.push_str("stage is empty\n");
    }
    Ok((text, truncated))
}

pub fn resolve_git_conflict(
    client: &mut GatewayClient,
    relative: &str,
    stage: u8,
) -> Result<(), ResolveConflictError> {
    if !(2..=3).contains(&stage) {
        return Err(ResolveConflictError::before_write(
            "conflict resolution requires stage 2 or 3",
        ));
    }
    let spec = format!(":{stage}:{relative}");
    let show = client
        .exec(
            &git_tool(),
            &["--no-pager".to_owned(), "show".to_owned(), spec],
            5000,
        )
        .map_err(|error| ResolveConflictError::before_write(error.to_string()))?;
    if show.timed_out {
        return Err(ResolveConflictError::before_write("git show timed out"));
    }
    if show.exit_code != 0 {
        return Err(ResolveConflictError::before_write(show.stderr));
    }
    if show.output_truncated {
        return Err(ResolveConflictError::before_write(
            "conflict stage is too large to resolve automatically",
        ));
    }
    client
        .write_all(relative, show.stdout.as_bytes())
        .map_err(|error| ResolveConflictError::after_write(error.to_string()))?;
    let add = client
        .exec(
            &git_tool(),
            &[
                "--no-pager".to_owned(),
                "add".to_owned(),
                "--".to_owned(),
                relative.to_owned(),
            ],
            5000,
        )
        .map_err(|error| ResolveConflictError::after_write(error.to_string()))?;
    if add.timed_out {
        return Err(ResolveConflictError::after_write("git add timed out"));
    }
    if add.exit_code != 0 {
        return Err(ResolveConflictError::after_write(add.stderr));
    }
    Ok(())
}

const GIT_HUNK_PATCH: &str = ".anvil-tmp/git-hunk.patch";

pub fn apply_git_hunk(
    client: &mut GatewayClient,
    patch: &str,
    reverse: bool,
) -> Result<(), String> {
    client
        .write_all(GIT_HUNK_PATCH, patch.as_bytes())
        .map_err(|error| error.to_string())?;
    let mut args = vec![
        "--no-pager".to_owned(),
        "apply".to_owned(),
        "--cached".to_owned(),
        "--whitespace=nowarn".to_owned(),
    ];
    if reverse {
        args.push("-R".to_owned());
    }
    args.extend(["--".to_owned(), GIT_HUNK_PATCH.to_owned()]);
    let result = client
        .exec(&git_tool(), &args, 5000)
        .map_err(|error| error.to_string());
    let _ = client.remove(GIT_HUNK_PATCH);
    let result = result?;
    if result.timed_out {
        return Err("git apply timed out".to_owned());
    }
    if result.exit_code != 0 {
        return Err(result.stderr);
    }
    Ok(())
}

pub fn parse_git_diff(output: &str) -> Vec<(usize, char)> {
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
    use super::resolve_git_conflict;
    use anvil_ipc::GatewayClient;
    use std::sync::{Arc, Mutex};

    fn resolve_through_mock(
        stage: u8,
        stdout: &str,
        expected_path: &str,
        expected_data: &str,
        expected_len: u64,
    ) {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().to_path_buf();
        let socket = workspace.join("gateway.sock");
        let listener = anvil_ipc::Listener::bind(&socket).expect("bind");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let stdout = stdout.to_owned();
        let expected_path = expected_path.to_owned();
        let expected_data = expected_data.to_owned();
        let server = std::thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            for _ in 0..4 {
                let frame = connection.recv().expect("recv").expect("frame");
                let request: serde_json::Value = serde_json::from_slice(&frame).expect("request");
                let method = request["method"].as_str().expect("method").to_owned();
                let params = request["params"].clone();
                let response = match method.as_str() {
                    "session.hello" => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "result": {"ok": true},
                    }),
                    "sys.exec" => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "result": {
                            "stdout": stdout,
                            "stderr": "",
                            "exit_code": 0,
                            "timed_out": false,
                            "output_truncated": false,
                        },
                    }),
                    "sys.write_file" => {
                        assert_eq!(params["path"], expected_path);
                        assert_eq!(params["offset"], 0);
                        assert_eq!(params["data"], expected_data);
                        assert_eq!(params["eof"], true);
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": request["id"],
                            "result": {"next_offset": expected_len},
                        })
                    }
                    _ => panic!("unexpected method {method}"),
                };
                if method != "session.hello" {
                    server_requests
                        .lock()
                        .expect("requests")
                        .push((method, params));
                }
                connection
                    .send(&serde_json::to_vec(&response).expect("response"))
                    .expect("send");
            }
        });

        let mut client = GatewayClient::connect(&socket, &[15; 32]).expect("client");
        resolve_git_conflict(&mut client, "conflict.txt", stage).expect("resolve");
        drop(client);
        server.join().expect("server");
        let requests = requests.lock().expect("requests");
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].0, "sys.exec");
        assert_eq!(requests[0].1["args"][1], "show");
        assert_eq!(requests[0].1["args"][2], format!(":{stage}:conflict.txt"));
        assert_eq!(requests[1].0, "sys.write_file");
        assert_eq!(requests[2].0, "sys.exec");
        assert_eq!(requests[2].1["args"][1], "add");
        assert_eq!(requests[2].1["args"][3], "conflict.txt");
    }

    #[test]
    fn resolves_ours_stage_by_writing_and_staging() {
        resolve_through_mock(2, "ours\n", "conflict.txt", "b3Vycwo=", 5);
    }

    #[test]
    fn resolves_theirs_stage_by_writing_and_staging() {
        resolve_through_mock(3, "theirs\n", "conflict.txt", "dGhlaXJzCg==", 7);
    }
    #[test]
    fn missing_stage_does_not_write_the_working_tree() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().to_path_buf();
        let socket = workspace.join("gateway.sock");
        let listener = anvil_ipc::Listener::bind(&socket).expect("bind");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let server = std::thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            for _ in 0..2 {
                let frame = connection.recv().expect("recv").expect("frame");
                let request: serde_json::Value = serde_json::from_slice(&frame).expect("request");
                let method = request["method"].as_str().expect("method").to_owned();
                let response = if method == "session.hello" {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "result": {"ok": true},
                    })
                } else {
                    assert_eq!(method, "sys.exec");
                    server_requests.lock().expect("requests").push(method);
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "result": {
                            "stdout": "",
                            "stderr": "missing stage",
                            "exit_code": 1,
                            "timed_out": false,
                            "output_truncated": false,
                        },
                    })
                };
                connection
                    .send(&serde_json::to_vec(&response).expect("response"))
                    .expect("send");
            }
        });

        let mut client = GatewayClient::connect(&socket, &[16; 32]).expect("client");
        let error =
            resolve_git_conflict(&mut client, "conflict.txt", 2).expect_err("missing stage");
        assert!(
            error.message.contains("missing stage"),
            "{:?}",
            error.message
        );
        drop(client);
        server.join().expect("server");
        assert_eq!(&*requests.lock().expect("requests"), &["sys.exec"]);
    }
}

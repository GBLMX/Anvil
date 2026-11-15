//! File enumeration, project search, and project replace helpers.

use super::*;

pub(crate) fn files(
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

pub(crate) fn project_search(
    client: &mut GatewayClient,
    query: &str,
) -> Result<(Vec<PipeLocation>, bool), String> {
    let tool = rg_tool().ok_or_else(|| "rg is unavailable".to_owned())?;
    let result = client
        .exec(
            &tool,
            &[
                "--json".to_owned(),
                "--line-number".to_owned(),
                "--smart-case".to_owned(),
                "--".to_owned(),
                query.to_owned(),
            ],
            5000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("project search timed out".to_owned());
    }
    if result.exit_code > 1 {
        return Err(result.stderr);
    }
    let mut truncated = result.output_truncated;
    let mut locations = Vec::new();
    for line in result.stdout.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if event.get("type").and_then(Value::as_str) != Some("match") {
            continue;
        }
        let path = event["data"]["path"]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let line_number = event["data"]["line_number"].as_u64().unwrap_or(1) as usize;
        let column = event["data"]["submatches"][0]["start"]
            .as_u64()
            .unwrap_or(0) as usize
            + 1;
        let message = event["data"]["lines"]["text"]
            .as_str()
            .unwrap_or_default()
            .trim_end()
            .to_owned();
        if path.is_empty() {
            continue;
        }
        if locations.len() >= MAX_PROJECT_RESULTS {
            truncated = true;
            break;
        }
        locations.push(PipeLocation {
            path,
            line: line_number,
            column,
            message,
        });
    }
    Ok((locations, truncated))
}

pub(crate) fn project_replace_preview(
    client: &mut GatewayClient,
    old: &str,
    new: &str,
) -> Result<ProjectReplacePlan, String> {
    if old.is_empty() {
        return Err("replace search text is empty".to_owned());
    }
    if old == new {
        return Err("replacement is identical to search text".to_owned());
    }
    let tool = rg_tool().ok_or_else(|| "rg is unavailable".to_owned())?;
    let result = client
        .exec(
            &tool,
            &[
                "--files-with-matches".to_owned(),
                "--fixed-strings".to_owned(),
                "--".to_owned(),
                old.to_owned(),
            ],
            5000,
        )
        .map_err(|error| error.to_string())?;
    if result.timed_out {
        return Err("replace preview timed out".to_owned());
    }
    if result.exit_code > 1 {
        return Err(result.stderr);
    }
    if result.output_truncated {
        return Err("too many files for project replace".to_owned());
    }
    let mut files = Vec::new();
    let mut total = 0usize;
    for (index, relative) in result.stdout.lines().enumerate() {
        let relative = relative.trim();
        if relative.is_empty() {
            continue;
        }
        if index >= MAX_REPLACE_FILES {
            return Err(format!(
                "project replace is limited to {MAX_REPLACE_FILES} files"
            ));
        }
        let bytes = client
            .read_all_binary(relative, MAX_REPLACE_FILE_BYTES)
            .map_err(|error| format!("{relative}: {error}"))?;
        total = total.saturating_add(bytes.len());
        if total > MAX_REPLACE_BYTES {
            return Err(format!(
                "project replace is limited to {MAX_REPLACE_BYTES} bytes"
            ));
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let matches = text.matches(old).count();
        if matches == 0 {
            continue;
        }
        files.push(ProjectReplaceFile {
            relative: relative.to_owned(),
            matches,
            hash: Sha256::digest(&bytes).into(),
        });
    }
    Ok(ProjectReplacePlan {
        old: old.to_owned(),
        new: new.to_owned(),
        files,
    })
}

pub(crate) fn apply_project_replace(
    client: &mut GatewayClient,
    plan: &ProjectReplacePlan,
) -> Result<(), String> {
    let mut originals = Vec::with_capacity(plan.files.len());
    let mut writes = Vec::with_capacity(plan.files.len());
    for file in &plan.files {
        let bytes = client
            .read_all_binary(&file.relative, MAX_REPLACE_FILE_BYTES)
            .map_err(|error| format!("{}: {error}", file.relative))?;
        let hash: [u8; 32] = Sha256::digest(&bytes).into();
        if hash != file.hash {
            return Err(format!(
                "{} changed since the preview; rerun the preview",
                file.relative
            ));
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| format!("{} is not valid UTF-8", file.relative))?;
        let replaced = text.replace(&plan.old, &plan.new);
        originals.push((file.relative.clone(), bytes));
        writes.push((file.relative.clone(), replaced.into_bytes()));
    }
    for (index, (relative, data)) in writes.iter().enumerate() {
        if let Err(error) = client.write_all(relative, data) {
            let mut rollback_error = None;
            for (original_path, original) in originals.iter().take(index + 1).rev() {
                if let Err(rollback) = client.write_all(original_path, original) {
                    rollback_error = Some(format!("{original_path}: {rollback}"));
                }
            }
            return Err(match rollback_error {
                Some(rollback) => {
                    format!("{relative}: {error}; rollback failed: {rollback}")
                }
                None => format!("{relative}: {error}; rollback complete"),
            });
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_ipc::Listener;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::sync::{Arc, Mutex};
    use std::thread;

    fn plan(files: &[(&str, &[u8])]) -> ProjectReplacePlan {
        ProjectReplacePlan {
            old: "old".to_owned(),
            new: "new".to_owned(),
            files: files
                .iter()
                .map(|(relative, bytes)| ProjectReplaceFile {
                    relative: (*relative).to_owned(),
                    matches: 1,
                    hash: Sha256::digest(bytes).into(),
                })
                .collect(),
        }
    }

    fn send_response(connection: &mut anvil_ipc::Connection, request: &Value, result: Value) {
        let response = json!({
            "jsonrpc": "2.0",
            "id": request["id"],
            "result": result,
        });
        connection
            .send(&serde_json::to_vec(&response).expect("response"))
            .expect("send response");
    }

    fn send_error(connection: &mut anvil_ipc::Connection, request: &Value) {
        let response = json!({
            "jsonrpc": "2.0",
            "id": request["id"],
            "error": {"code": -32000, "message": "write failed"},
        });
        connection
            .send(&serde_json::to_vec(&response).expect("response"))
            .expect("send error");
    }

    #[test]
    fn rejects_a_preview_hash_change_before_writing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("gateway.sock");
        let listener = Listener::bind(&socket).expect("bind");
        let server = thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            let hello: Value =
                serde_json::from_slice(&connection.recv().expect("recv").expect("frame"))
                    .expect("hello");
            assert_eq!(hello["method"], "session.hello");
            send_response(&mut connection, &hello, json!({"session_id": "test"}));
            let request: Value =
                serde_json::from_slice(&connection.recv().expect("recv").expect("frame"))
                    .expect("request");
            assert_eq!(request["method"], "sys.read_file");
            let mut frame = vec![1];
            frame.extend_from_slice(&0u64.to_be_bytes());
            frame.extend_from_slice(&0u64.to_be_bytes());
            frame.extend_from_slice(b"changed");
            connection.send(&frame).expect("binary response");
        });
        let mut client = GatewayClient::connect(&socket, &[7; 32]).expect("client");
        let plan = plan(&[("a.txt", b"original")]);
        let error = apply_project_replace(&mut client, &plan).expect_err("hash mismatch");
        assert!(error.contains("changed since the preview"), "{error}");
        drop(client);
        server.join().expect("server");
    }

    #[test]
    fn write_failure_rolls_back_completed_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("gateway.sock");
        let listener = Listener::bind(&socket).expect("bind");
        let writes = Arc::new(Mutex::new(Vec::new()));
        let server_writes = Arc::clone(&writes);
        let server = thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            let hello: Value =
                serde_json::from_slice(&connection.recv().expect("recv").expect("frame"))
                    .expect("hello");
            assert_eq!(hello["method"], "session.hello");
            send_response(&mut connection, &hello, json!({"session_id": "test"}));
            let mut failed = false;
            while let Some(frame) = connection.recv().expect("recv") {
                let request: Value = serde_json::from_slice(&frame).expect("request");
                let method = request["method"].as_str().expect("method");
                match method {
                    "sys.read_file" => {
                        let path = request["params"]["path"].as_str().expect("path");
                        let data = if path == "a.txt" { b"old1" } else { b"old2" };
                        let mut response = vec![1];
                        response.extend_from_slice(&0u64.to_be_bytes());
                        response.extend_from_slice(&0u64.to_be_bytes());
                        response.extend_from_slice(data);
                        connection.send(&response).expect("binary response");
                    }
                    "sys.write_file" => {
                        let path = request["params"]["path"].as_str().expect("path");
                        server_writes.lock().expect("writes").push(path.to_owned());
                        if path == "b.txt" && !failed {
                            failed = true;
                            send_error(&mut connection, &request);
                        } else {
                            send_response(&mut connection, &request, json!({"next_offset": 4}));
                        }
                    }
                    other => panic!("unexpected method {other}"),
                }
            }
        });
        let mut client = GatewayClient::connect(&socket, &[7; 32]).expect("client");
        let plan = plan(&[("a.txt", b"old1"), ("b.txt", b"old2")]);
        let error = apply_project_replace(&mut client, &plan).expect_err("write failure");
        assert!(error.contains("rollback complete"), "{error}");
        drop(client);
        server.join().expect("server");
        assert_eq!(
            writes.lock().expect("writes").as_slice(),
            ["a.txt", "b.txt", "b.txt", "a.txt"]
        );
    }
}

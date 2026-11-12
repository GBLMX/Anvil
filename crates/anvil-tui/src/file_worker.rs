//! Background workspace directory and file enumeration.
//!
//! The UI thread never performs a gateway directory read. One worker owns a
//! dedicated authenticated connection; requests and results use mpsc.

use std::path::Path;
use std::time::Duration;

use anvil_ipc::GatewayClient;
use serde_json::Value;

use crate::pipe_locations::PipeLocation;
use sha2::{Digest, Sha256};

mod git;
mod preview;
mod protocol;
mod search;
mod worker;
#[cfg(test)]
pub(crate) use git::{parse_git_diff, parse_git_status};
pub(crate) use preview::PreviewData;
#[cfg(test)]
pub(crate) use preview::{PREVIEW_MAX_LINES, decode_preview};
pub use protocol::{FileEvent, FileRequest};
pub(crate) use protocol::{ProjectReplaceFile, ProjectReplacePlan, ResolveConflictError};
pub use worker::FileWorker;

const FILE_LIMIT: usize = 5000;
pub(crate) const MAX_TREE_WATCH_DIRS: usize = 256;
const TREE_POLL_INTERVAL: Duration = Duration::from_millis(500);
const TREE_DEBOUNCE: Duration = Duration::from_millis(500);
pub(crate) const MAX_PROJECT_RESULTS: usize = 500;
const MAX_REPLACE_FILES: usize = 100;
const MAX_REPLACE_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_REPLACE_BYTES: usize = 16 * 1024 * 1024;

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

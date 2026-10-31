use crate::lsp::{Diagnostic, publish_diagnostics};
use anvil_ipc::GatewayClient;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const INITIALIZE_ID: u64 = 1;

#[derive(Debug)]
pub enum LspEvent {
    Diagnostics(PathBuf, Vec<Diagnostic>),
    Response {
        id: u64,
        result: Value,
        error: Option<String>,
    },
    Crashed(String),
    Error(String),
    Log(String),
}

/// Commands sent from the UI to the language-server thread.
pub enum LspCommand {
    Sync {
        path: PathBuf,
        text: String,
    },
    Save {
        path: PathBuf,
        text: String,
    },
    Request {
        id: u64,
        method: String,
        params: Value,
    },
}

/// One language-server launch profile.
pub struct LspConfig {
    pub tool: String,
    pub args: Vec<String>,
    pub language_id: String,
    pub root_markers: Vec<String>,
}

pub struct LspWorker {
    events: Receiver<LspEvent>,
    commands: Sender<LspCommand>,
}

impl LspWorker {
    pub fn start(
        mut client: GatewayClient,
        config: LspConfig,
        workspace: PathBuf,
        path: PathBuf,
        text: String,
    ) -> Result<Self, String> {
        let (sender, events) = mpsc::channel();
        let (command_sender, commands) = mpsc::channel();
        let LspConfig {
            tool,
            args,
            language_id,
            root_markers,
        } = config;
        std::thread::spawn(move || {
            if let Err(error) = client.lsp_start(&tool, &args) {
                let _ = sender.send(LspEvent::Error(error.to_string()));
                return;
            }
            let uri = file_uri(&path);
            let root = project_root(&workspace, &path, &root_markers);
            let initialize = serde_json::json!({
                "jsonrpc": "2.0",
                "id": INITIALIZE_ID,
                "method": "initialize",
                "params": {
                    "processId": null,
                    "rootUri": file_uri(&root),
                    "capabilities": {
                        "textDocument": {
                            "publishDiagnostics": {"relatedInformation": true},
                            "synchronization": {"didSave": true}
                        },
                        "workspace": {"workspaceFolders": true},
                        "window": {"workDoneProgress": true}
                    }
                }
            });
            if let Err(error) = send(&mut client, &initialize) {
                let _ = sender.send(LspEvent::Crashed(error));
                return;
            }
            if let Err(error) = wait_initialize(&mut client) {
                let _ = sender.send(LspEvent::Crashed(error));
                return;
            }
            let initialized = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "initialized",
                "params": {}
            });
            let _ = send(&mut client, &initialized);
            let did_open = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "languageId": language_id,
                        "version": 1,
                        "text": text
                    }
                }
            });
            if let Err(error) = send(&mut client, &did_open) {
                let _ = sender.send(LspEvent::Crashed(error));
                return;
            }
            let did_save = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didSave",
                "params": {"textDocument": {"uri": uri}}
            });
            let _ = send(&mut client, &did_save);
            let mut documents = HashMap::new();
            documents.insert(path.clone(), 1_u64);
            loop {
                while let Ok(command) = commands.try_recv() {
                    let result = match command {
                        LspCommand::Sync { path, text } => {
                            sync_document(&mut client, &mut documents, &language_id, &path, &text)
                        }
                        LspCommand::Save { path, text } => {
                            sync_document(&mut client, &mut documents, &language_id, &path, &text)
                                .and_then(|()| {
                                    send(
                                        &mut client,
                                        &serde_json::json!({
                                            "jsonrpc": "2.0",
                                            "method": "textDocument/didSave",
                                            "params": {"textDocument": {"uri": file_uri(&path)}}
                                        }),
                                    )
                                })
                        }
                        LspCommand::Request { id, method, params } => send(
                            &mut client,
                            &serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "method": method,
                                "params": params
                            }),
                        ),
                    };
                    if let Err(error) = result {
                        let _ = sender.send(LspEvent::Crashed(error));
                        return;
                    }
                }
                match client.wait_readable(POLL_INTERVAL) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(error) => {
                        let _ = sender.send(LspEvent::Crashed(error.to_string()));
                        return;
                    }
                }
                let message = match client.recv_notification() {
                    Ok(Some(message)) => message,
                    Ok(None) => {
                        let _ = sender.send(LspEvent::Crashed("gateway closed".to_owned()));
                        return;
                    }
                    Err(error) => {
                        let _ = sender.send(LspEvent::Crashed(error.to_string()));
                        return;
                    }
                };
                match message.method.as_str() {
                    "lsp.crashed" => {
                        let _ = sender.send(LspEvent::Crashed(message.params.to_string()));
                        return;
                    }
                    "lsp.stderr" => {
                        let line = message
                            .params
                            .get("message")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default();
                        let _ = sender.send(LspEvent::Log(line.to_owned()));
                    }
                    "lsp.message" => {
                        let Some(data) = message.params.get("data") else {
                            continue;
                        };
                        if data.get("method").is_none()
                            && let Some(id) = data.get("id").and_then(Value::as_u64)
                        {
                            let error = data
                                .get("error")
                                .and_then(|error| error.get("message"))
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned);
                            let result = data.get("result").cloned().unwrap_or(Value::Null);
                            let _ = sender.send(LspEvent::Response { id, result, error });
                            continue;
                        }
                        match answer_server_request(&mut client, data) {
                            Ok(true) => continue,
                            Ok(false) => {}
                            Err(error) => {
                                let _ = sender.send(LspEvent::Crashed(error));
                                return;
                            }
                        }
                        if let Some((path, diagnostics)) = publish_diagnostics(data) {
                            let _ = sender.send(LspEvent::Diagnostics(path, diagnostics));
                        }
                    }
                    _ => {}
                }
            }
        });
        Ok(Self {
            events,
            commands: command_sender,
        })
    }

    pub fn sync(&self, path: &Path, text: String) -> Result<(), String> {
        self.commands
            .send(LspCommand::Sync {
                path: path.to_path_buf(),
                text,
            })
            .map_err(|_| "lsp worker stopped".to_owned())
    }

    pub fn save(&self, path: &Path, text: String) -> Result<(), String> {
        self.commands
            .send(LspCommand::Save {
                path: path.to_path_buf(),
                text,
            })
            .map_err(|_| "lsp worker stopped".to_owned())
    }

    pub fn request(&self, id: u64, method: &str, params: Value) -> Result<(), String> {
        self.commands
            .send(LspCommand::Request {
                id,
                method: method.to_owned(),
                params,
            })
            .map_err(|_| "lsp worker stopped".to_owned())
    }

    pub fn poll(&self) -> Vec<LspEvent> {
        self.events.try_iter().collect()
    }
}

fn answer_server_request(
    client: &mut GatewayClient,
    message: &serde_json::Value,
) -> Result<bool, String> {
    let Some(id) = message.get("id") else {
        return Ok(false);
    };
    if message.get("method").is_none() {
        return Ok(false);
    }
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": null
    });
    send(client, &response)?;
    Ok(true)
}

fn sync_document(
    client: &mut GatewayClient,
    documents: &mut HashMap<PathBuf, u64>,
    language_id: &str,
    path: &Path,
    text: &str,
) -> Result<(), String> {
    let uri = file_uri(path);
    if let Some(version) = documents.get_mut(path) {
        *version += 1;
        send(
            client,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": {"uri": uri, "version": *version},
                    "contentChanges": [{"text": text}]
                }
            }),
        )
    } else {
        documents.insert(path.to_path_buf(), 1);
        send(
            client,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "languageId": language_id,
                        "version": 1,
                        "text": text
                    }
                }
            }),
        )
    }
}

fn project_root(
    workspace: &std::path::Path,
    path: &std::path::Path,
    markers: &[String],
) -> std::path::PathBuf {
    let mut root = workspace.to_path_buf();
    if let Some(parent) = path.parent() {
        for ancestor in parent.ancestors() {
            if !ancestor.starts_with(workspace) {
                break;
            }
            if markers.iter().any(|marker| ancestor.join(marker).is_file()) {
                root = ancestor.to_path_buf();
            }
            if ancestor == workspace {
                break;
            }
        }
    }
    root
}

pub(crate) fn file_uri(path: &std::path::Path) -> String {
    let mut absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if absolute.is_relative() {
        if let Ok(current) = std::env::current_dir() {
            absolute = current.join(absolute);
        }
    }
    let mut uri = String::from("file://");
    for byte in absolute.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'_' | b'.' | b'~') {
            uri.push(byte as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

fn send(client: &mut GatewayClient, message: &serde_json::Value) -> Result<(), String> {
    let body = serde_json::to_vec(message).map_err(|error| error.to_string())?;
    client.lsp_send(&body).map_err(|error| error.to_string())
}

fn wait_initialize(client: &mut GatewayClient) -> Result<(), String> {
    loop {
        let Some(message) = client
            .recv_notification()
            .map_err(|error| error.to_string())?
        else {
            return Err("gateway closed during initialize".to_owned());
        };
        if message.method == "lsp.crashed" {
            return Err(message.params.to_string());
        }
        if message.method != "lsp.message" {
            continue;
        }
        let Some(data) = message.params.get("data") else {
            continue;
        };
        if answer_server_request(client, data)? {
            continue;
        }
        if data.get("id") == Some(&serde_json::json!(INITIALIZE_ID)) {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_ipc::{Connection, JSONRPC_VERSION, Listener, Request, Response};
    use serde_json::json;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn receive(connection: &mut Connection) -> Request {
        let frame = connection.recv().expect("frame").expect("body");
        serde_json::from_slice(&frame).expect("request")
    }

    fn respond(connection: &mut Connection, id: &str) {
        let response = Response {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: Some(id.to_owned()),
            result: Some(json!({"ok": true})),
            error: None,
        };
        connection
            .send(&serde_json::to_vec(&response).expect("response"))
            .expect("send");
    }

    #[test]
    fn file_uri_is_absolute_and_encoded() {
        let path = std::env::temp_dir().join("a b#c.rs");
        let uri = file_uri(&path);
        assert!(uri.starts_with("file:///"), "{uri}");
        assert!(uri.ends_with("a%20b%23c.rs"), "{uri}");
    }

    #[test]
    fn project_root_prefers_highest_manifest() {
        let dir = std::env::temp_dir().join(format!("anvil-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("crate/src")).expect("dirs");
        std::fs::write(dir.join("Cargo.toml"), "").expect("root manifest");
        std::fs::write(dir.join("crate/Cargo.toml"), "").expect("crate manifest");
        let path = dir.join("crate/src/main.rs");
        std::fs::write(&path, "").expect("file");
        assert_eq!(project_root(&dir, &path, &["Cargo.toml".to_owned()]), dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn worker_parses_publish_diagnostics() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let socket = std::env::temp_dir().join(format!(
            "anvil-lsp-worker-{}-{nanos}.sock",
            std::process::id()
        ));
        let listener = Listener::bind(&socket).expect("bind");
        let _server = std::thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            let hello = receive(&mut connection);
            respond(&mut connection, hello.id.as_deref().expect("id"));
            for index in 0..5 {
                let request = receive(&mut connection);
                respond(&mut connection, request.id.as_deref().expect("id"));
                if index == 1 {
                    let ready = json!({
                        "jsonrpc": "2.0",
                        "method": "lsp.message",
                        "params": {"data": {
                            "jsonrpc": "2.0",
                            "id": 1,
                            "result": {}
                        }}
                    });
                    connection
                        .send(&serde_json::to_vec(&ready).expect("initialize response"))
                        .expect("send initialize response");
                }
                if index == 4 {
                    let notification = json!({
                        "jsonrpc": "2.0",
                        "method": "lsp.message",
                        "params": {"data": {
                            "jsonrpc": "2.0",
                            "method": "textDocument/publishDiagnostics",
                            "params": {
                                "uri": "file:///tmp/a.rs",
                                "diagnostics": [{
                                    "range": {
                                        "start": {"line": 1, "character": 0},
                                        "end": {"line": 1, "character": 1}
                                    },
                                    "severity": 1,
                                    "message": "bad"
                                }]
                            }
                        }}
                    });
                    connection
                        .send(&serde_json::to_vec(&notification).expect("notification"))
                        .expect("send notification");
                }
            }
        });
        let client = anvil_ipc::GatewayClient::connect(&socket, &[1; 32]).expect("client");
        let worker = LspWorker::start(
            client,
            LspConfig {
                tool: "rust-analyzer".to_owned(),
                args: Vec::new(),
                language_id: "rust".to_owned(),
                root_markers: vec!["Cargo.toml".to_owned()],
            },
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp/a.rs"),
            "fn main() {}".to_owned(),
        )
        .expect("worker");
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            for event in worker.poll() {
                if let LspEvent::Diagnostics(path, diagnostics) = event {
                    assert_eq!(path, PathBuf::from("/tmp/a.rs"));
                    assert_eq!(diagnostics.len(), 1);
                    let _ = std::fs::remove_file(&socket);
                    return;
                }
            }
            assert!(Instant::now() < deadline, "no diagnostics event");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

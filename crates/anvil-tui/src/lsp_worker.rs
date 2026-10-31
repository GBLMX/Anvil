use crate::lsp::{Diagnostic, publish_diagnostics};
use anvil_ipc::GatewayClient;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

#[derive(Debug)]
pub enum LspEvent {
    Diagnostics(PathBuf, Vec<Diagnostic>),
    Crashed(String),
    Error(String),
    Log(String),
}

pub struct LspWorker {
    events: Receiver<LspEvent>,
}

impl LspWorker {
    pub fn start(
        mut client: GatewayClient,
        tool: &str,
        args: Vec<String>,
        workspace: PathBuf,
        path: PathBuf,
        text: String,
    ) -> Result<Self, String> {
        let (sender, events) = mpsc::channel();
        let tool = tool.to_owned();
        std::thread::spawn(move || {
            if let Err(error) = client.lsp_start(&tool, &args) {
                let _ = sender.send(LspEvent::Error(error.to_string()));
                return;
            }
            let uri = format!("file://{}", path.display());
            let initialize = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": null,
                    "rootUri": format!("file://{}", workspace.display()),
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
                let _ = sender.send(LspEvent::Error(error));
                return;
            }
            if let Err(error) = wait_initialize(&mut client) {
                let _ = sender.send(LspEvent::Error(error));
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
                        "languageId": "rust",
                        "version": 1,
                        "text": text
                    }
                }
            });
            if let Err(error) = send(&mut client, &did_open) {
                let _ = sender.send(LspEvent::Error(error));
                return;
            }
            let did_save = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didSave",
                "params": {"textDocument": {"uri": uri}}
            });
            let _ = send(&mut client, &did_save);
            loop {
                let message = match client.recv_notification() {
                    Ok(Some(message)) => message,
                    Ok(None) => {
                        let _ = sender.send(LspEvent::Crashed("gateway closed".to_owned()));
                        return;
                    }
                    Err(error) => {
                        let _ = sender.send(LspEvent::Error(error.to_string()));
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
                        match answer_server_request(&mut client, data) {
                            Ok(true) => continue,
                            Ok(false) => {}
                            Err(error) => {
                                let _ = sender.send(LspEvent::Error(error));
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
        Ok(Self { events })
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
        if data.get("id") == Some(&serde_json::json!(1)) {
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
            "rust-analyzer",
            Vec::new(),
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

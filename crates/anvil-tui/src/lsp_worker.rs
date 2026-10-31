use crate::lsp::{Diagnostic, publish_diagnostics};
use anvil_ipc::GatewayClient;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

#[derive(Debug)]
pub enum LspEvent {
    Diagnostics(PathBuf, Vec<Diagnostic>),
    Crashed(String),
    Error(String),
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
                    "capabilities": {}
                }
            });
            if let Err(error) = send(&mut client, &initialize) {
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
                    "lsp.message" => {
                        let Some(data) = message.params.get("data") else {
                            continue;
                        };
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

fn send(client: &mut GatewayClient, message: &serde_json::Value) -> Result<(), String> {
    let body = serde_json::to_vec(message).map_err(|error| error.to_string())?;
    client.lsp_send(&body).map_err(|error| error.to_string())
}

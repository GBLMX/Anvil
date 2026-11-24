use crate::lsp::{Diagnostic, publish_diagnostics};
use anvil_ipc::{ClientError, GatewayClient};
use anvil_task::{DataError, TaskContext, TaskEvent as TaskGroupEvent, TaskGroup, TaskGroupConfig};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    mpsc::{self, SyncSender},
};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const INITIALIZE_ID: u64 = 1;
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const LSP_COMMAND_CAPACITY: usize = 1024;
const LSP_EVENT_CAPACITY: usize = 1024;
const LSP_SHUTDOWN: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub enum LspEvent {
    Diagnostics(PathBuf, Vec<Diagnostic>),
    Response {
        id: u64,
        result: Value,
        error: Option<String>,
    },
    Capabilities {
        inlay_hints: bool,
        semantic_token_types: Vec<String>,
        server_info: Option<String>,
    },
    Crashed(String),
    StartupError(String),
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
    Close {
        path: PathBuf,
    },
    Request {
        id: u64,
        method: String,
        params: Value,
    },
    Configuration {
        settings: Value,
    },
}

/// One language-server launch profile.
pub struct LspConfig {
    pub tool: String,
    pub args: Vec<String>,
    pub language_id: String,
    pub root_markers: Vec<String>,
    pub initialization_options: Option<serde_json::Value>,
    pub env: BTreeMap<String, String>,
    pub settings: Value,
}

pub struct LspWorker {
    events: Mutex<TaskGroup<LspEvent>>,
    commands: SyncSender<LspCommand>,
    diagnostics: Arc<Mutex<HashMap<PathBuf, Vec<Diagnostic>>>>,
    overflow_responses: Arc<Mutex<HashMap<u64, String>>>,
    root: PathBuf,
    settings: Value,
}

impl LspWorker {
    /// Returns the project root used for the initialize handshake.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn settings(&self) -> Value {
        self.settings.clone()
    }

    pub fn start(
        mut client: GatewayClient,
        config: LspConfig,
        workspace: PathBuf,
        path: PathBuf,
        text: String,
    ) -> Result<Self, String> {
        let (command_sender, commands) = mpsc::sync_channel(LSP_COMMAND_CAPACITY);
        let LspConfig {
            tool,
            args,
            language_id,
            root_markers,
            initialization_options,
            env,
            settings,
        } = config;
        let worker_settings = settings.clone();
        let root = project_root(&workspace, &path, &root_markers);
        let worker_root = root.clone();
        let group_config = TaskGroupConfig {
            global_data_event_capacity: LSP_EVENT_CAPACITY,
            per_task_capacity: LSP_EVENT_CAPACITY,
            max_events_per_poll: 256,
            ..TaskGroupConfig::default()
        };
        let diagnostics = Arc::new(Mutex::new(HashMap::new()));
        let worker_diagnostics = Arc::clone(&diagnostics);
        let overflow_responses = Arc::new(Mutex::new(HashMap::new()));
        let worker_overflow = Arc::clone(&overflow_responses);
        let mut group = TaskGroup::with_config(group_config);
        let spawn = group.spawn("lsp", move |context| {
            let relative_root = root
                .strip_prefix(&workspace)
                .ok()
                .filter(|relative| !relative.as_os_str().is_empty())
                .map(|relative| relative.to_string_lossy().into_owned());
            let mut current_settings = settings;
            if let Err(error) = client.lsp_start(&tool, &args, relative_root.as_deref(), &env) {
                let _ = send_lsp_event(
                    &context,
                    &worker_overflow,
                    LspEvent::Error(error.to_string()),
                );
                return Ok(());
            }
            let uri = file_uri(&path);
            let mut initialize = serde_json::json!({
                "jsonrpc": "2.0",
                "id": INITIALIZE_ID,
                "method": "initialize",
                "params": {
                    "processId": null,
                    "rootUri": file_uri(&root),
                    "workspaceFolders": [workspace_folder(&root)],
                    "capabilities": {
                        "textDocument": {
                            "publishDiagnostics": {"relatedInformation": true},
                            "synchronization": {"didSave": true},
                            "inlayHint": {
                                "dynamicRegistration": false
                            },
                            "codeAction": {
                                "codeActionLiteralSupport": {
                                    "codeActionKind": {
                                        "valueSet": ["", "quickfix", "refactor", "source"]
                                    }
                                },
                                "isPreferredSupport": true,
                                "disabledSupport": true,
                                "dataSupport": true
                            },
                            "callHierarchy": {
                                "dynamicRegistration": false
                            },
                            "foldingRange": {
                                "dynamicRegistration": false
                            },
                            "completion": {
                                "completionItem": {
                                    "snippetSupport": true,
                                    "documentationFormat": ["markdown", "plaintext"],
                                    "resolveSupport": {
                                        "properties": ["documentation", "detail"]
                                    }
                                }
                            },
                            "semanticTokens": {
                                "dynamicRegistration": false,
                                "requests": {"full": true},
                                "tokenTypes": [],
                                "tokenModifiers": [],
                                "formats": ["relative"],
                                "multilineTokenSupport": false
                            }
                        },
                        "workspace": {"workspaceFolders": true},
                        "window": {"workDoneProgress": true}
                    }
                }
            });
            if let Some(options) = initialization_options {
                initialize["params"]["initializationOptions"] = options;
            }
            if let Err(error) = send(&mut client, &initialize) {
                let _ = send_lsp_event(&context, &worker_overflow, LspEvent::Crashed(error));
                return Ok(());
            }
            let (inlay_hints, semantic_token_types, server_info) =
                match wait_initialize(&mut client, &root, &current_settings) {
                    Ok(capabilities) => capabilities,
                    Err(error) => {
                        let _ = send_lsp_event(
                            &context,
                            &worker_overflow,
                            LspEvent::StartupError(error),
                        );
                        return Ok(());
                    }
                };
            let _ = send_lsp_event(
                &context,
                &worker_overflow,
                LspEvent::Capabilities {
                    inlay_hints,
                    semantic_token_types,
                    server_info,
                },
            );
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
                let _ = send_lsp_event(&context, &worker_overflow, LspEvent::Crashed(error));
                return Ok(());
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
                if context.is_cancelled() {
                    let _ = client.lsp_stop();
                    return Ok(());
                }
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
                        LspCommand::Close { path } => {
                            close_document(&mut client, &mut documents, &path)
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
                        LspCommand::Configuration { settings } => {
                            current_settings = settings.clone();
                            send(
                                &mut client,
                                &serde_json::json!({
                                    "jsonrpc": "2.0",
                                    "method": "workspace/didChangeConfiguration",
                                    "params": {"settings": settings}
                                }),
                            )
                        }
                    };
                    if let Err(error) = result {
                        let _ =
                            send_lsp_event(&context, &worker_overflow, LspEvent::Crashed(error));
                        return Ok(());
                    }
                }
                match client.wait_readable(POLL_INTERVAL) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(error) => {
                        let _ = send_lsp_event(
                            &context,
                            &worker_overflow,
                            LspEvent::Crashed(error.to_string()),
                        );
                        return Ok(());
                    }
                }
                let message = match client.recv_notification() {
                    Ok(Some(message)) => message,
                    Ok(None) => {
                        let _ = send_lsp_event(
                            &context,
                            &worker_overflow,
                            LspEvent::Crashed("gateway closed".to_owned()),
                        );
                        return Ok(());
                    }
                    Err(error) => {
                        let _ = send_lsp_event(
                            &context,
                            &worker_overflow,
                            LspEvent::Crashed(error.to_string()),
                        );
                        return Ok(());
                    }
                };
                match message.method.as_str() {
                    "lsp.crashed" => {
                        let _ = send_lsp_event(
                            &context,
                            &worker_overflow,
                            LspEvent::Crashed(message.params.to_string()),
                        );
                        return Ok(());
                    }
                    "lsp.stderr" => {
                        let line = message
                            .params
                            .get("message")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default();
                        let _ = send_lsp_event(
                            &context,
                            &worker_overflow,
                            LspEvent::Log(line.to_owned()),
                        );
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
                            let _ = send_lsp_event(
                                &context,
                                &worker_overflow,
                                LspEvent::Response { id, result, error },
                            );
                            continue;
                        }
                        match answer_server_request(&mut client, data, &root, &current_settings) {
                            Ok(true) => continue,
                            Ok(false) => {}
                            Err(error) => {
                                let _ = send_lsp_event(
                                    &context,
                                    &worker_overflow,
                                    LspEvent::Crashed(error),
                                );
                                return Ok(());
                            }
                        }
                        if let Some((path, diagnostics)) = publish_diagnostics(data)
                            && let Ok(mut latest) = worker_diagnostics.lock()
                        {
                            latest.insert(path, diagnostics);
                        }
                    }
                    _ => {}
                }
            }
        });
        match spawn {
            Ok(_) => Ok(Self {
                events: Mutex::new(group),
                commands: command_sender,
                diagnostics,
                overflow_responses,
                root: worker_root,
                settings: worker_settings,
            }),
            Err(error) => Err(format!("lsp thread failed: {error}")),
        }
    }

    pub fn sync(&self, path: &Path, text: String) -> Result<(), String> {
        self.sync_recoverable(path, text)
            .map_err(|(error, _)| error)
    }

    pub fn did_change_configuration(&self, settings: Value) -> Result<(), String> {
        self.commands
            .try_send(LspCommand::Configuration { settings })
            .map_err(|_| "lsp worker busy or stopped".to_owned())
    }

    /// Sync, returning the unsent text when the command queue is full.
    pub fn sync_recoverable(&self, path: &Path, text: String) -> Result<(), (String, String)> {
        match self.commands.try_send(LspCommand::Sync {
            path: path.to_path_buf(),
            text,
        }) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(LspCommand::Sync { text, .. })) => {
                Err(("lsp worker busy".to_owned(), text))
            }
            Err(mpsc::TrySendError::Disconnected(LspCommand::Sync { text, .. })) => {
                Err(("lsp worker stopped".to_owned(), text))
            }
            Err(_) => Err(("lsp worker stopped".to_owned(), String::new())),
        }
    }

    pub fn save(&self, path: &Path, text: String) -> Result<(), String> {
        self.send_command(LspCommand::Save {
            path: path.to_path_buf(),
            text,
        })
    }

    pub fn close(&self, path: &Path) -> Result<(), String> {
        self.send_command(LspCommand::Close {
            path: path.to_path_buf(),
        })
    }

    pub fn request(&self, id: u64, method: &str, params: Value) -> Result<(), String> {
        self.send_command(LspCommand::Request {
            id,
            method: method.to_owned(),
            params,
        })
    }

    pub fn poll(&self) -> Vec<LspEvent> {
        let mut events = Vec::new();
        if let Ok(mut diagnostics) = self.diagnostics.lock() {
            events.extend(
                diagnostics
                    .drain()
                    .map(|(path, diagnostics)| LspEvent::Diagnostics(path, diagnostics)),
            );
        }
        if let Ok(mut overflow) = self.overflow_responses.lock() {
            events.extend(overflow.drain().map(|(id, error)| LspEvent::Response {
                id,
                result: Value::Null,
                error: Some(error),
            }));
        }
        if let Ok(mut group) = self.events.lock() {
            events.extend(
                group
                    .poll()
                    .events
                    .into_iter()
                    .filter_map(|event| match event {
                        TaskGroupEvent::Data { data, .. } => Some(data),
                        _ => None,
                    }),
            );
        }
        events
    }

    fn send_command(&self, command: LspCommand) -> Result<(), String> {
        match self.commands.try_send(command) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Err("lsp worker busy".to_owned()),
            Err(mpsc::TrySendError::Disconnected(_)) => Err("lsp worker stopped".to_owned()),
        }
    }
}

impl Drop for LspWorker {
    fn drop(&mut self) {
        let group = self
            .events
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = group.shutdown(LSP_SHUTDOWN);
    }
}

fn send_lsp_event(
    context: &TaskContext<LspEvent>,
    overflow: &Mutex<HashMap<u64, String>>,
    mut event: LspEvent,
) -> bool {
    loop {
        if context.is_cancelled() {
            return false;
        }
        match context.send_data(event) {
            Ok(()) => return true,
            Err(DataError::Closed(_)) => return false,
            Err(DataError::TaskFull(returned))
            | Err(DataError::GlobalFull(returned))
            | Err(DataError::GlobalBytesFull(returned)) => {
                if let LspEvent::Response { id, .. } = &returned {
                    if let Ok(mut overflow) = overflow.lock() {
                        overflow.insert(*id, "lsp event queue full".to_owned());
                        return true;
                    }
                    return false;
                }
                event = returned;
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

fn workspace_folder(root: &Path) -> Value {
    let name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string());
    serde_json::json!({"uri": file_uri(root), "name": name})
}

fn answer_server_request(
    client: &mut GatewayClient,
    message: &serde_json::Value,
    root: &Path,
    settings: &Value,
) -> Result<bool, String> {
    let Some(id) = message.get("id") else {
        return Ok(false);
    };
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Ok(false);
    };
    let result = server_request_result(method, message, root, settings);
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    });
    send(client, &response)?;
    Ok(true)
}

fn configuration_section(settings: &Value, section: &str) -> Value {
    section
        .split('.')
        .filter(|part| !part.is_empty())
        .try_fold(settings, |value, part| value.get(part))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}))
}

fn server_request_result(method: &str, message: &Value, root: &Path, settings: &Value) -> Value {
    match method {
        "workspace/configuration" => {
            let items = message
                .pointer("/params/items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            Value::Array(
                items
                    .iter()
                    .map(|item| {
                        configuration_section(
                            settings,
                            item.get("section").and_then(Value::as_str).unwrap_or(""),
                        )
                    })
                    .collect(),
            )
        }
        "workspace/workspaceFolders" => Value::Array(vec![workspace_folder(root)]),
        "workspace/applyEdit" => serde_json::json!({"applied": false}),
        _ => Value::Null,
    }
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

fn close_document(
    client: &mut GatewayClient,
    documents: &mut HashMap<PathBuf, u64>,
    path: &Path,
) -> Result<(), String> {
    if documents.remove(path).is_none() {
        return Ok(());
    }
    send(
        client,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": {"textDocument": {"uri": file_uri(path)}}
        }),
    )
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

pub fn file_uri(path: &std::path::Path) -> String {
    let mut absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if absolute.is_relative()
        && let Ok(current) = std::env::current_dir()
    {
        absolute = current.join(absolute);
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

fn wait_initialize(
    client: &mut GatewayClient,
    root: &Path,
    settings: &Value,
) -> Result<(bool, Vec<String>, Option<String>), String> {
    let deadline = Instant::now() + INITIALIZE_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            return Err("LSP initialize timed out".to_owned());
        }
        let Some(message) =
            client
                .recv_notification_deadline(deadline)
                .map_err(|error| match error {
                    ClientError::Io(std::io::ErrorKind::TimedOut) => {
                        "LSP initialize timed out".to_owned()
                    }
                    error => error.to_string(),
                })?
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
        if answer_server_request(client, data, root, settings)? {
            continue;
        }
        if data.get("id") == Some(&serde_json::json!(INITIALIZE_ID)) {
            if let Some(error) = data
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(serde_json::Value::as_str)
            {
                return Err(format!("initialize failed: {error}"));
            }
            let capabilities = data
                .get("result")
                .and_then(|result| result.get("capabilities"));
            let provider =
                capabilities.and_then(|capabilities| capabilities.get("inlayHintProvider"));
            let inlay_hints = provider
                .and_then(Value::as_bool)
                .unwrap_or_else(|| provider.is_some());
            let semantic_token_types = capabilities
                .and_then(|capabilities| {
                    capabilities.pointer("/semanticTokensProvider/legend/tokenTypes")
                })
                .and_then(Value::as_array)
                .map(|types| {
                    types
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let server_info = data.pointer("/result/serverInfo").map(|info| {
                let name = info
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("language server");
                let version = info
                    .get("version")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                format!("{name} {version}")
            });
            return Ok((inlay_hints, semantic_token_types, server_info));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_ipc::{Connection, JSONRPC_VERSION, Listener, Request, Response};
    use serde_json::json;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn sync_recoverable_returns_latest_text_when_queue_is_full() {
        let (commands, _receiver) = mpsc::sync_channel(1);
        commands
            .try_send(LspCommand::Sync {
                path: PathBuf::from("/tmp/queued.rs"),
                text: "queued".to_owned(),
            })
            .expect("fill queue");
        let worker = LspWorker {
            events: Mutex::new(TaskGroup::new()),
            commands,
            diagnostics: Arc::new(Mutex::new(HashMap::new())),
            overflow_responses: Arc::new(Mutex::new(HashMap::new())),
            root: PathBuf::from("/tmp"),
            settings: Value::Null,
        };
        let (error, text) = worker
            .sync_recoverable(Path::new("/tmp/a.rs"), "latest".to_owned())
            .expect_err("full queue");
        assert_eq!(error, "lsp worker busy");
        assert_eq!(text, "latest");
    }

    #[test]
    fn response_overflow_is_explicit() {
        let config = TaskGroupConfig {
            global_data_event_capacity: 1,
            per_task_capacity: 1,
            ..TaskGroupConfig::default()
        };
        let mut group = TaskGroup::<LspEvent>::with_config(config);
        let overflow = Arc::new(Mutex::new(HashMap::new()));
        let task_overflow = Arc::clone(&overflow);
        let _ = group
            .spawn("overflow", move |context| {
                let _ = send_lsp_event(
                    &context,
                    &task_overflow,
                    LspEvent::Response {
                        id: 1,
                        result: Value::Null,
                        error: None,
                    },
                );
                let _ = send_lsp_event(
                    &context,
                    &task_overflow,
                    LspEvent::Response {
                        id: 2,
                        result: Value::Null,
                        error: None,
                    },
                );
                Ok(())
            })
            .expect("spawn");
        let deadline = Instant::now() + Duration::from_secs(1);
        while !overflow.lock().expect("overflow").contains_key(&2) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(overflow.lock().expect("overflow").contains_key(&2));
        group.shutdown(Duration::from_millis(100));
    }

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
    fn server_requests_get_protocol_results() {
        let root = Path::new("/ws");
        let settings = json!({
            "rust-analyzer": {"cargo": {"allFeatures": true}},
            "lsp": {"inlay_hints": true},
        });
        let message = json!({"params": {"items": [
            {"section": "rust-analyzer"},
            {"section": "lsp.inlay_hints"},
            {"section": "missing"},
        ]}});
        assert_eq!(
            server_request_result("workspace/configuration", &message, root, &settings),
            json!([
                {"cargo": {"allFeatures": true}},
                true,
                {},
            ])
        );
        assert_eq!(
            server_request_result("workspace/workspaceFolders", &message, root, &settings),
            json!([{"uri": "file:///ws", "name": "ws"}])
        );
        assert_eq!(
            server_request_result("workspace/applyEdit", &message, root, &settings),
            json!({"applied": false})
        );
        assert_eq!(
            server_request_result("client/registerCapability", &message, root, &settings),
            Value::Null
        );
    }

    #[test]
    fn close_document_removes_only_the_requested_document() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("gateway.sock");
        let listener = Listener::bind(&socket).expect("bind");
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let mut connection = listener.accept().expect("accept");
                let hello = receive(&mut connection);
                respond(&mut connection, hello.id.as_deref().expect("id"));
                let request = receive(&mut connection);
                assert_eq!(request.method, "lsp.send");
                let message = &request.params["data"];
                assert_eq!(message["method"], "textDocument/didClose");
                assert!(
                    message["params"]["textDocument"]["uri"]
                        .as_str()
                        .is_some_and(|uri| uri.ends_with("a.rs"))
                );
                respond(&mut connection, request.id.as_deref().expect("id"));
                while let Ok(Some(_)) = connection.recv() {}
            });
            let mut client = GatewayClient::connect(&socket, &[1; 32]).expect("client");
            let path = PathBuf::from("/tmp/a.rs");
            let background = PathBuf::from("/tmp/b.rs");
            let mut documents = HashMap::from([(path.clone(), 1_u64), (background.clone(), 2)]);
            close_document(&mut client, &mut documents, &path).expect("close");
            assert!(!documents.contains_key(&path));
            assert_eq!(documents.get(&background), Some(&2));
            close_document(&mut client, &mut documents, &path).expect("already closed");
            drop(client);
        });
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
    fn fake_gateway_worker_stops_and_parses_diagnostics() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let socket = std::env::temp_dir().join(format!(
            "anvil-lsp-worker-{}-{nanos}.sock",
            std::process::id()
        ));
        let listener = Listener::bind(&socket).expect("bind");
        let stopped =
            std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let server_stopped = std::sync::Arc::clone(&stopped);
        let server = std::thread::spawn(move || {
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
            let stop = receive(&mut connection);
            assert_eq!(stop.method, "lsp.stop");
            respond(&mut connection, stop.id.as_deref().expect("id"));
            let (lock, ready) = &*server_stopped;
            *lock.lock().expect("stop state") = true;
            ready.notify_one();
        });
        let client = anvil_ipc::GatewayClient::connect(&socket, &[1; 32]).expect("client");
        let worker = LspWorker::start(
            client,
            LspConfig {
                tool: "rust-analyzer".to_owned(),
                args: Vec::new(),
                language_id: "rust".to_owned(),
                initialization_options: None,
                root_markers: vec!["Cargo.toml".to_owned()],
                env: BTreeMap::new(),
                settings: Value::Null,
            },
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp/a.rs"),
            "fn main() {}".to_owned(),
        )
        .expect("worker");
        assert_eq!(worker.root(), Path::new("/tmp"));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            for event in worker.poll() {
                if let LspEvent::Diagnostics(path, diagnostics) = event {
                    assert_eq!(path, PathBuf::from("/tmp/a.rs"));
                    assert_eq!(diagnostics.len(), 1);
                    let started = Instant::now();
                    drop(worker);
                    assert!(
                        started.elapsed() < Duration::from_secs(5),
                        "worker drop exceeded its shutdown budget"
                    );
                    let (lock, ready) = &*stopped;
                    let deadline = Instant::now() + Duration::from_secs(5);
                    let mut done = lock.lock().expect("stop state");
                    while !*done {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        assert!(!remaining.is_zero(), "lsp.stop was not sent");
                        let (next, _) = ready.wait_timeout(done, remaining).expect("stop state");
                        done = next;
                    }
                    server.join().expect("server");
                    let _ = std::fs::remove_file(&socket);
                    return;
                }
            }
            assert!(Instant::now() < deadline, "no diagnostics event");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

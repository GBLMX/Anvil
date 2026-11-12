//! Stable single-instance control socket for the Anvil TUI.
//!
//! The control socket is separate from the per-session gateway socket. It
//! accepts framed JSON-RPC requests from `anvil-ctl`, verifies peer credentials,
//! and forwards editor actions to the running TUI.

use anvil_ipc::{JSONRPC_VERSION, Listener, Request, Response, codes, parse_request};
use rustix::fs::FlockOperation;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// Protocol version reported by the control server.
pub const PROTOCOL_VERSION: &str = "3";

/// Control methods advertised by this protocol version.
pub const CAPABILITIES: &[&str] = &["status", "open", "save", "edit", "query", "exec", "quit"];

/// Coarse authority required by an editor command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Read,
    Edit,
    Git,
    Admin,
}

impl Capability {
    /// Whether the default remote surface may expose this capability.
    pub fn is_remote_allowed(self) -> bool {
        !matches!(self, Self::Admin)
    }
}

/// Buffer mutation mode exposed by `control.edit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditMode {
    Set,
    Insert,
    Replace,
}

/// One editor command and the authority it requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub capability: Capability,
}

/// Shared command manifest for execution, completion, CLI checks, and MCP.
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "w",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "wa",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "wq",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "q",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "q!",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "cq",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "e",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "help",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "settings",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "theme",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "theme-auto",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "theme-save",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "pkg",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "reconnect",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "config-reload",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "lsp-start",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "lsp-stop",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "tree",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "format",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "symbols",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "problems",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "replace",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "replace-all",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "bn",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "bp",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "new",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "bd",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "bd!",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "plugin-start",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "plugin-list",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "plugin-oom",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "plugin-exec",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "plugin-stop",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "plugin-enable",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "conflict-recover",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "conflict-discard",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "git-diff",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "git-hunk-next",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "git-hunk-prev",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "git-stage-hunk",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "git-unstage-hunk",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "git-blame",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "undo",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "redo",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "comment",
        capability: Capability::Edit,
    },
    CommandSpec {
        name: "git-conflicts",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "git-conflict-base",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "git-conflict-ours",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "git-conflict-theirs",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "search-project",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "search-cancel",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "replace-project",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "replace-project-apply",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "replace-project-cancel",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "git-resolve-ours",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "git-resolve-ours!",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "git-resolve-theirs",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "git-resolve-theirs!",
        capability: Capability::Git,
    },
    CommandSpec {
        name: "breakpoint",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "dap-attach",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "dap-detach",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "debug",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "debug-refresh",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "debug-panel",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "debug-watch",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "debug-watch-clear",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "task",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "task-output",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "tests",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "terminal",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "terminal-send",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "terminal-stop",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "terminal-read",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "workspace-add",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "workspace-remove",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "workspace-trust",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "workspace-untrust",
        capability: Capability::Admin,
    },
    CommandSpec {
        name: "workspace-list",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "recent-files",
        capability: Capability::Read,
    },
    CommandSpec {
        name: "recent-workspaces",
        capability: Capability::Read,
    },
];

/// Look up an exact command name.
pub fn command_capability(name: &str) -> Option<Capability> {
    COMMANDS
        .iter()
        .find(|command| command.name == name)
        .map(|command| command.capability)
}

/// Look up the capability required by a command line.
pub fn capability_for_command(command: &str) -> Option<Capability> {
    let name = command
        .trim()
        .trim_start_matches(':')
        .split_whitespace()
        .next()?;
    if name.starts_with("s/") || name.starts_with("%s/") {
        Some(Capability::Edit)
    } else {
        command_capability(name)
    }
}

/// Why a command is not accepted on the default remote surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPolicyError {
    Empty,
    Unknown,
    Admin,
    PathArgument,
}

impl std::fmt::Display for CommandPolicyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(formatter, "command is empty"),
            Self::Unknown => write!(formatter, "command is not available remotely"),
            Self::Admin => write!(formatter, "command requires admin capability"),
            Self::PathArgument => {
                write!(formatter, "use control.open or control.save for file paths")
            }
        }
    }
}

/// Validate a command against the shared capability and path policy.
pub fn validate_remote_command(command: &str) -> Result<(), CommandPolicyError> {
    let command = command.trim().trim_start_matches(':').trim();
    if command.is_empty() {
        return Err(CommandPolicyError::Empty);
    }
    let name = command.split_whitespace().next().unwrap_or_default();
    let has_argument = command.split_whitespace().nth(1).is_some();
    if matches!(name, "e" | "w" | "wq") && has_argument {
        return Err(CommandPolicyError::PathArgument);
    }
    match capability_for_command(command) {
        None => Err(CommandPolicyError::Unknown),
        Some(capability) if !capability.is_remote_allowed() => Err(CommandPolicyError::Admin),
        Some(_) => Ok(()),
    }
}

/// A one-based line and byte column inside that line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPoint {
    pub line: usize,
    pub byte_column: usize,
}

/// Half-open range used by `EditMode::Replace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditRange {
    pub start: TextPoint,
    pub end: TextPoint,
}

/// Bounded query request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryOptions {
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub include_text: bool,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            start_line: None,
            end_line: None,
            include_text: true,
        }
    }
}

/// Control response with the editor status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub version: String,
    pub protocol_version: String,
    pub workspace: PathBuf,
    pub file: Option<PathBuf>,
    pub dirty: bool,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// One open buffer in a query result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BufferInfo {
    pub path: Option<PathBuf>,
    pub modified: bool,
    pub active: bool,
}

/// Bounded buffer state returned by `control.query`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryResult {
    pub version: String,
    pub workspace: PathBuf,
    pub path: Option<PathBuf>,
    pub modified: bool,
    pub read_only: bool,
    pub cursor: usize,
    pub revision: u64,
    pub line_count: usize,
    pub undo_depth: usize,
    pub undo_bytes: usize,
    pub selection: Vec<(usize, usize)>,
    pub text: String,
    pub text_truncated: bool,
    pub start_line: usize,
    pub end_line: usize,
    pub active: usize,
    pub buffers: Vec<BufferInfo>,
}

/// Result of `control.edit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditResult {
    pub accepted: bool,
    pub revision: u64,
    pub modified: bool,
    pub cursor: usize,
}

/// Result of `control.exec`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecResult {
    pub accepted: bool,
    pub status: String,
}

/// A typed editor action requested by a control client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlRequest {
    Status,
    Open {
        path: PathBuf,
        line: usize,
        column: usize,
    },
    Exec {
        command: String,
    },
    Save {
        path: Option<PathBuf>,
    },
    Edit {
        mode: EditMode,
        text: String,
        range: Option<EditRange>,
        expected_revision: Option<u64>,
    },
    Query(QueryOptions),
    Quit {
        force: bool,
    },
}

/// Error payload returned through JSON-RPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

impl RpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Result sent by the TUI for one control request.
pub type ControlReply = Result<Value, RpcError>;

/// One request waiting for the TUI event loop.
#[derive(Debug)]
pub struct ControlCall {
    pub request: ControlRequest,
    pub reply: mpsc::Sender<ControlReply>,
}

/// A control socket, lock, and server thread.
#[derive(Debug)]
pub struct ControlServer {
    path: PathBuf,
    receiver: mpsc::Receiver<ControlCall>,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _lock: File,
}

/// Control client or server failure.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Frame(anvil_ipc::FrameError),
    Json(serde_json::Error),
    Rpc { code: i32, message: String },
    Protocol(&'static str),
    RuntimeDir(String),
    AlreadyRunning,
    Lock(String),
    Timeout,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Frame(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
            Self::Rpc { code, message } => write!(formatter, "control error {code}: {message}"),
            Self::Protocol(message) => write!(formatter, "control protocol error: {message}"),
            Self::RuntimeDir(message) => write!(formatter, "control runtime directory: {message}"),
            Self::AlreadyRunning => write!(formatter, "another Anvil instance is already running"),
            Self::Lock(message) => write!(formatter, "control lock: {message}"),
            Self::Timeout => write!(formatter, "control request timed out"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<anvil_ipc::FrameError> for Error {
    fn from(error: anvil_ipc::FrameError) -> Self {
        Self::Frame(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Stable control socket path under `$XDG_RUNTIME_DIR`.
pub fn socket_path() -> Result<PathBuf, Error> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RuntimeDir("XDG_RUNTIME_DIR is not set".to_owned()))?;
    Ok(PathBuf::from(runtime).join("anvil/control.sock"))
}

impl ControlServer {
    /// Start the control server at the stable runtime path.
    pub fn start() -> Result<Self, Error> {
        Self::start_at(socket_path()?)
    }

    /// Start the control server at an explicit socket path.
    pub fn start_at(socket: PathBuf) -> Result<Self, Error> {
        let parent = socket
            .parent()
            .ok_or_else(|| Error::RuntimeDir(format!("{} has no parent", socket.display())))?;
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        let lock_path = parent.join("control.lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(&lock_path)?;
        fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o600))?;
        if rustix::fs::flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
            if is_live(&socket) {
                return Err(Error::AlreadyRunning);
            }
            return Err(Error::Lock(format!(
                "{} is held by another process",
                lock_path.display()
            )));
        }
        if socket.exists() {
            if is_live(&socket) {
                return Err(Error::AlreadyRunning);
            }
            fs::remove_file(&socket)?;
        }
        let listener = Listener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let (sender, receiver) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let thread = thread::spawn(move || accept_loop(listener, sender, thread_shutdown));
        Ok(Self {
            path: socket,
            receiver,
            shutdown,
            thread: Some(thread),
            _lock: lock,
        })
    }

    /// Path of the bound socket.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Nonblocking fetch of the next request for the TUI.
    pub fn try_recv(&self) -> Result<ControlCall, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn accept_loop(listener: Listener, sender: mpsc::Sender<ControlCall>, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok(connection) => handle_connection(connection, &sender),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
}

fn handle_connection(mut connection: anvil_ipc::Connection, sender: &mpsc::Sender<ControlCall>) {
    let _ = connection.set_read_timeout(Some(Duration::from_secs(5)));
    let uid = match connection.peer_credentials() {
        Ok(credentials) => credentials.uid,
        Err(_) => {
            let _ = send_error(
                &mut connection,
                None,
                codes::UNAUTHORIZED,
                "peer credentials unavailable",
            );
            return;
        }
    };
    if uid != rustix::process::geteuid().as_raw() {
        let _ = send_error(
            &mut connection,
            None,
            codes::UNAUTHORIZED,
            "peer UID does not match",
        );
        return;
    }
    let Ok(Some(frame)) = connection.recv() else {
        return;
    };
    let request = match parse_request(std::str::from_utf8(&frame).unwrap_or_default()) {
        Ok(request) => request,
        Err(error) => {
            let _ = send_error(
                &mut connection,
                None,
                codes::PARSE_ERROR,
                &format!("invalid request: {error}"),
            );
            return;
        }
    };
    let id = request.id.clone();
    if request.method == "control.ping" {
        let _ = send_result(
            &mut connection,
            id,
            json!({"pong": true, "protocol": PROTOCOL_VERSION}),
        );
        return;
    }
    let control = match parse_control(&request) {
        Ok(control) => control,
        Err(error) => {
            let _ = send_error(&mut connection, id, error.code, &error.message);
            return;
        }
    };
    let (reply_sender, reply_receiver) = mpsc::channel();
    if sender
        .send(ControlCall {
            request: control,
            reply: reply_sender,
        })
        .is_err()
    {
        let _ = send_error(
            &mut connection,
            id,
            codes::UNAVAILABLE,
            "editor is shutting down",
        );
        return;
    }
    match reply_receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(value)) => {
            let _ = send_result(&mut connection, id, value);
        }
        Ok(Err(error)) => {
            let _ = send_error(&mut connection, id, error.code, &error.message);
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = send_error(&mut connection, id, codes::TIMEOUT, "editor did not answer");
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = send_error(&mut connection, id, codes::UNAVAILABLE, "editor shut down");
        }
    }
}

fn parse_control(request: &Request) -> Result<ControlRequest, RpcError> {
    match request.method.as_str() {
        "control.status" => Ok(ControlRequest::Status),
        "control.open" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, "path is required"))?;
            let line = request
                .params
                .get("line")
                .and_then(Value::as_u64)
                .unwrap_or(1) as usize;
            let column = request
                .params
                .get("column")
                .and_then(Value::as_u64)
                .unwrap_or(1) as usize;
            Ok(ControlRequest::Open {
                path: PathBuf::from(path),
                line,
                column,
            })
        }
        "control.exec" => {
            let command = request
                .params
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, "command is required"))?;
            Ok(ControlRequest::Exec {
                command: command.to_owned(),
            })
        }
        "control.save" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .map(PathBuf::from);
            Ok(ControlRequest::Save { path })
        }
        "control.edit" => {
            let mode = match request.params.get("mode").and_then(Value::as_str) {
                Some("set") => EditMode::Set,
                Some("insert") => EditMode::Insert,
                Some("replace") => EditMode::Replace,
                Some(_) => {
                    return Err(RpcError::new(
                        codes::INVALID_PARAMS,
                        "mode must be set, insert, or replace",
                    ));
                }
                None => {
                    return Err(RpcError::new(codes::INVALID_PARAMS, "mode is required"));
                }
            };
            let text = request
                .params
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, "text is required"))?;
            let range = request
                .params
                .get("range")
                .filter(|value| !value.is_null())
                .map(|value| serde_json::from_value(value.clone()))
                .transpose()
                .map_err(|error| {
                    RpcError::new(codes::INVALID_PARAMS, format!("invalid range: {error}"))
                })?;
            let expected_revision = request
                .params
                .get("expected_revision")
                .and_then(Value::as_u64);
            Ok(ControlRequest::Edit {
                mode,
                text: text.to_owned(),
                range,
                expected_revision,
            })
        }
        "control.query" => Ok(ControlRequest::Query(QueryOptions {
            start_line: request
                .params
                .get("start_line")
                .and_then(Value::as_u64)
                .map(|value| value as usize),
            end_line: request
                .params
                .get("end_line")
                .and_then(Value::as_u64)
                .map(|value| value as usize),
            include_text: request
                .params
                .get("include_text")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        })),
        "control.quit" => {
            let force = request
                .params
                .get("force")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Ok(ControlRequest::Quit { force })
        }
        _ => Err(RpcError::new(
            codes::METHOD_NOT_FOUND,
            "unknown control method",
        )),
    }
}

fn send_result(
    connection: &mut anvil_ipc::Connection,
    id: Option<String>,
    result: Value,
) -> Result<(), Error> {
    send_response(
        connection,
        Response {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: Some(result),
            error: None,
        },
    )
}

fn send_error(
    connection: &mut anvil_ipc::Connection,
    id: Option<String>,
    code: i32,
    message: &str,
) -> Result<(), Error> {
    send_response(
        connection,
        Response {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: None,
            error: Some(anvil_ipc::ErrorObject {
                code,
                message: message.to_owned(),
                data: None,
            }),
        },
    )
}

fn send_response(connection: &mut anvil_ipc::Connection, response: Response) -> Result<(), Error> {
    let body = serde_json::to_vec(&response)?;
    connection.send(&body)?;
    Ok(())
}

fn is_live(socket: &Path) -> bool {
    Client::connect_at(socket, Duration::from_millis(250))
        .and_then(|mut client| client.ping())
        .is_ok()
}

/// Client for one control server.
#[derive(Debug)]
pub struct Client {
    socket: PathBuf,
    timeout: Duration,
    next_id: u64,
}

impl Client {
    /// Connect to the stable control socket.
    pub fn connect() -> Result<Self, Error> {
        let socket = socket_path()?;
        Self::connect_at(&socket, Duration::from_secs(5))
    }

    /// Connect to an explicit socket path.
    pub fn connect_at(socket: &Path, timeout: Duration) -> Result<Self, Error> {
        Ok(Self {
            socket: socket.to_path_buf(),
            timeout,
            next_id: 1,
        })
    }

    /// Check that the server is alive.
    pub fn ping(&mut self) -> Result<(), Error> {
        self.request("control.ping", json!({}))?;
        Ok(())
    }

    /// Read the editor status.
    pub fn status(&mut self) -> Result<Status, Error> {
        let value = self.request("control.status", json!({}))?;
        Ok(serde_json::from_value(value)?)
    }

    /// Ask the running editor to open a file location.
    pub fn open(&mut self, path: &Path, line: usize, column: usize) -> Result<Value, Error> {
        self.request(
            "control.open",
            json!({"path": path, "line": line, "column": column}),
        )
    }

    /// Execute an Ex command through the editor state machine.
    pub fn exec(&mut self, command: &str) -> Result<ExecResult, Error> {
        let value = self.request("control.exec", json!({"command": command}))?;
        Ok(serde_json::from_value(value)?)
    }

    /// Save the current buffer, optionally as a path.
    pub fn save(&mut self, path: Option<&Path>) -> Result<Value, Error> {
        self.request(
            "control.save",
            json!({"path": path.map(|path| path.to_string_lossy())}),
        )
    }

    /// Replace the whole buffer or insert at the cursor.
    pub fn edit(&mut self, mode: EditMode, text: &str) -> Result<EditResult, Error> {
        let value = self.request("control.edit", json!({"mode": mode, "text": text}))?;
        Ok(serde_json::from_value(value)?)
    }

    /// Replace a bounded range after checking `expected_revision`.
    pub fn replace(
        &mut self,
        range: EditRange,
        text: &str,
        expected_revision: u64,
    ) -> Result<EditResult, Error> {
        let value = self.request(
            "control.edit",
            json!({
                "mode": EditMode::Replace,
                "range": range,
                "text": text,
                "expected_revision": expected_revision,
            }),
        )?;
        Ok(serde_json::from_value(value)?)
    }

    /// Query the active buffer with default bounds.
    pub fn query(&mut self) -> Result<QueryResult, Error> {
        self.query_with(QueryOptions::default())
    }

    /// Query the active buffer with explicit bounds.
    pub fn query_with(&mut self, options: QueryOptions) -> Result<QueryResult, Error> {
        let value = self.request("control.query", json!(options))?;
        Ok(serde_json::from_value(value)?)
    }

    /// Ask the editor to quit.
    pub fn quit(&mut self, force: bool) -> Result<Value, Error> {
        self.request("control.quit", json!({"force": force}))
    }

    /// Connect with an explicit timeout.
    pub fn connect_with_timeout(timeout: Duration) -> Result<Self, Error> {
        Self::connect_at(&socket_path()?, timeout)
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        let id = format!("ctl-{}", self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        let request = Request {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: Some(id.clone()),
            method: method.to_owned(),
            params,
            meta: None,
        };
        let body = serde_json::to_vec(&request)?;
        let mut connection = anvil_ipc::Connection::connect(&self.socket)?;
        connection.set_read_timeout(Some(self.timeout))?;
        connection.send(&body)?;
        let Some(frame) = connection.recv()? else {
            return Err(Error::Protocol("control server closed the connection"));
        };
        let response: Response = serde_json::from_slice(&frame)?;
        response
            .validate()
            .map_err(|_| Error::Protocol("invalid response envelope"))?;
        if response.id.as_deref() != Some(id.as_str()) {
            return Err(Error::Protocol("response id mismatch"));
        }
        if let Some(error) = response.error {
            return Err(Error::Rpc {
                code: error.code,
                message: error.message,
            });
        }
        response
            .result
            .ok_or(Error::Protocol("response has no result"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn server_at(path: &Path) -> Result<ControlServer, Error> {
        ControlServer::start_at(path.to_path_buf())
    }

    #[test]
    fn binds_private_socket_and_parent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("anvil/control.sock");
        let server = server_at(&socket).expect("server");
        assert_eq!(
            fs::metadata(socket.parent().expect("parent"))
                .expect("parent")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&socket).expect("socket").permissions().mode() & 0o777,
            0o600
        );
        drop(server);
    }

    #[test]
    fn replies_to_ping_and_removes_socket_on_drop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("anvil/control.sock");
        let server = server_at(&socket).expect("server");
        let mut client = Client::connect_at(&socket, Duration::from_secs(1)).expect("client");
        client.ping().expect("ping");
        drop(server);
        assert!(!socket.exists());
    }

    #[test]
    fn rejects_a_second_server_and_cleans_a_stale_socket() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("anvil/control.sock");
        let server = server_at(&socket).expect("server");
        assert!(matches!(server_at(&socket), Err(Error::AlreadyRunning)));
        drop(server);

        fs::write(&socket, "stale").expect("stale socket");
        let server = server_at(&socket).expect("stale socket cleanup");
        assert!(socket.exists());
        drop(server);
    }

    #[test]
    fn parses_mutation_requests() {
        let save = Request {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: Some("1".to_owned()),
            method: "control.save".to_owned(),
            params: json!({"path": "a.txt"}),
            meta: None,
        };
        assert!(matches!(
            parse_control(&save).unwrap(),
            ControlRequest::Save { path: Some(path) } if path == PathBuf::from("a.txt")
        ));
        let edit = Request {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: Some("2".to_owned()),
            method: "control.edit".to_owned(),
            params: json!({"mode": "set", "text": "hello"}),
            meta: None,
        };
        assert!(matches!(
            parse_control(&edit).unwrap(),
            ControlRequest::Edit { mode: EditMode::Set, text, .. } if text == "hello"
        ));
        let quit = Request {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: Some("3".to_owned()),
            method: "control.quit".to_owned(),
            params: json!({"force": true}),
            meta: None,
        };
        assert!(matches!(
            parse_control(&quit).unwrap(),
            ControlRequest::Quit { force: true }
        ));
    }

    #[test]
    fn command_manifest_is_unique_and_classified() {
        let mut names = std::collections::HashSet::new();
        for command in COMMANDS {
            assert!(names.insert(command.name), "duplicate {}", command.name);
        }
        assert_eq!(command_capability("pkg"), Some(Capability::Admin));
        assert_eq!(command_capability("git-diff"), Some(Capability::Git));
        assert_eq!(capability_for_command("%s/a/b/"), Some(Capability::Edit));
        assert!(!Capability::Admin.is_remote_allowed());
    }

    #[test]
    fn remote_policy_matches_every_manifest_capability() {
        for command in COMMANDS {
            let result = validate_remote_command(command.name);
            if command.capability.is_remote_allowed() {
                assert!(result.is_ok(), "{} should be allowed", command.name);
            } else {
                assert_eq!(
                    result,
                    Err(CommandPolicyError::Admin),
                    "{} should require admin",
                    command.name
                );
            }
        }
    }
}

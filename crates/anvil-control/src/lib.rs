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
pub const PROTOCOL_VERSION: &str = "2";

/// Control response with the editor status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub version: String,
    pub workspace: PathBuf,
    pub file: Option<PathBuf>,
    pub dirty: bool,
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
        mode: String,
        text: String,
    },
    Query,
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
            let mode = request
                .params
                .get("mode")
                .and_then(Value::as_str)
                .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, "mode is required"))?;
            let text = request
                .params
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, "text is required"))?;
            Ok(ControlRequest::Edit {
                mode: mode.to_owned(),
                text: text.to_owned(),
            })
        }
        "control.query" => Ok(ControlRequest::Query),
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
    pub fn exec(&mut self, command: &str) -> Result<Value, Error> {
        self.request("control.exec", json!({"command": command}))
    }

    /// Save the current buffer, optionally as a path.
    pub fn save(&mut self, path: Option<&Path>) -> Result<Value, Error> {
        self.request(
            "control.save",
            json!({"path": path.map(|path| path.to_string_lossy())}),
        )
    }

    /// Edit the active buffer with `set` or `insert`.
    pub fn edit(&mut self, mode: &str, text: &str) -> Result<Value, Error> {
        self.request("control.edit", json!({"mode": mode, "text": text}))
    }

    /// Query the active buffer state.
    pub fn query(&mut self) -> Result<Value, Error> {
        self.request("control.query", json!({}))
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
            ControlRequest::Edit { mode, text } if mode == "set" && text == "hello"
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
}

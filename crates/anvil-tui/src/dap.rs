//! Minimal Debug Adapter Protocol client for a separate adapter process.

use anvil_process::{ProcessSpec, ProcessTask, kill_process_group};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{ChildStdin, ChildStdout},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

pub const MAX_DAP_MESSAGE: usize = 8 * 1024 * 1024;

/// Upper bound for adapter messages buffered while waiting for a response.
///
/// Adapter events (output, stopped, terminated, …) are not consumed by the
/// current panels, so the queue drops its oldest entry instead of growing for
/// the lifetime of the session.
pub const MAX_DAP_EVENTS: usize = 256;

/// Cross-thread kill switch for one adapter process group.
///
/// The worker that owns the [`DapClient`] registers its pid here; the UI thread
/// signals the group without touching the client, which unblocks a worker stuck
/// reading a hung adapter. The owner still reaps the child on drop.
#[derive(Clone, Debug, Default)]
pub struct DapCancel {
    pid: Arc<AtomicU32>,
    cancelled: Arc<AtomicBool>,
}

impl DapCancel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the adapter process-group leader so [`Self::cancel`] can reach it.
    ///
    /// Kills immediately when cancellation already happened, closing the race
    /// with a cancel issued before the adapter finished spawning.
    pub fn set_pid(&self, pid: u32) {
        self.pid.store(pid, Ordering::SeqCst);
        if self.cancelled.load(Ordering::SeqCst) {
            kill_process_group(pid);
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid.load(Ordering::SeqCst)
    }

    /// Kill the adapter process group. Safe to call more than once.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let pid = self.pid.load(Ordering::SeqCst);
        if pid != 0 {
            kill_process_group(pid);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Bounded lifecycle for a DAP worker thread.
///
/// Dropping the worker kills the adapter process group and joins the thread, so
/// timed-out or superseded work cannot leave a stuck thread behind.
pub struct DapWorker {
    cancel: DapCancel,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl DapWorker {
    pub fn new(cancel: DapCancel, handle: std::thread::JoinHandle<()>) -> Self {
        Self {
            cancel,
            handle: Some(handle),
        }
    }

    /// Pid registered by the worker so far (`0` before the adapter spawns).
    pub fn pid(&self) -> u32 {
        self.cancel.pid()
    }

    /// Join a worker that already delivered its result without killing the
    /// adapter it handed back to the caller.
    pub fn finish(mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for DapWorker {
    fn drop(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        if !handle.is_finished() {
            self.cancel.cancel();
        }
        let _ = handle.join();
    }
}

#[derive(Debug)]
pub enum DapError {
    Io(std::io::Error),
    Protocol(String),
    Json(serde_json::Error),
    AdapterExited,
}

impl std::fmt::Display for DapError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "dap io: {error}"),
            Self::Protocol(message) => write!(formatter, "dap protocol: {message}"),
            Self::Json(error) => write!(formatter, "dap json: {error}"),
            Self::AdapterExited => formatter.write_str("dap adapter exited"),
        }
    }
}

impl std::error::Error for DapError {}

impl From<std::io::Error> for DapError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DapError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub struct DapClient {
    process: ProcessTask,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_seq: i64,
    events: VecDeque<Value>,
    adapter_id: String,
}

impl DapClient {
    pub fn spawn(program: &Path, args: &[String], cwd: &Path) -> Result<Self, DapError> {
        let program = if program.is_absolute() {
            let program = program.canonicalize().map_err(DapError::Io)?;
            if !program.is_file() {
                return Err(DapError::Protocol(
                    "adapter program is not a file".to_owned(),
                ));
            }
            program
        } else {
            program.to_path_buf()
        };
        let cwd = cwd.canonicalize().map_err(DapError::Io)?;
        let spec = ProcessSpec::new(program)
            .args(args.iter().cloned())
            .cwd(cwd)
            .stdin(true)
            .stdout(true);
        let mut process = ProcessTask::spawn(spec)?;
        let stdin = process
            .take_stdin()
            .ok_or_else(|| DapError::Protocol("missing adapter stdin".to_owned()))?;
        let stdout = process
            .take_stdout()
            .ok_or_else(|| DapError::Protocol("missing adapter stdout".to_owned()))?;
        Ok(Self {
            process,
            stdin,
            stdout: BufReader::new(stdout),
            next_seq: 1,
            events: VecDeque::new(),
            adapter_id: "anvil".to_owned(),
        })
    }

    /// Spawn and register the pid with `cancel` so another thread can kill a
    /// hung adapter. Cancellation requested between spawn and registration is
    /// still honoured by [`DapCancel::set_pid`].
    pub fn spawn_cancellable(
        program: &Path,
        args: &[String],
        cwd: &Path,
        cancel: &DapCancel,
    ) -> Result<Self, DapError> {
        let client = Self::spawn(program, args, cwd)?;
        cancel.set_pid(client.pid());
        if cancel.is_cancelled() {
            return Err(DapError::Protocol("dap attach cancelled".to_owned()));
        }
        Ok(client)
    }

    pub fn pid(&self) -> u32 {
        self.process.pid()
    }

    pub fn set_adapter_id(&mut self, adapter_id: impl Into<String>) {
        self.adapter_id = adapter_id.into();
    }

    pub fn request(&mut self, command: &str, arguments: Value) -> Result<i64, DapError> {
        let seq = self.next_seq;
        self.next_seq += 1;
        let payload = json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        });
        write_frame(&mut self.stdin, &payload)?;
        Ok(seq)
    }

    pub fn initialize(&mut self) -> Result<i64, DapError> {
        let adapter_id = self.adapter_id.clone();
        self.request(
            "initialize",
            json!({
                "clientID": "anvil",
                "clientName": "Anvil",
                "adapterID": adapter_id,
                "linesStartAt1": true,
                "columnsStartAt1": true,
                "pathFormat": "path",
                "supportsVariableType": true,
            }),
        )
    }

    pub fn launch(&mut self, program: &str, args: &[String], cwd: &Path) -> Result<i64, DapError> {
        self.request(
            "launch",
            json!({
                "program": program,
                "args": args,
                "cwd": cwd,
                "noDebug": false,
            }),
        )
    }

    pub fn set_breakpoints(&mut self, path: &Path, lines: &[u32]) -> Result<i64, DapError> {
        let breakpoints = lines
            .iter()
            .map(|line| json!({"line": line}))
            .collect::<Vec<_>>();
        self.request(
            "setBreakpoints",
            json!({
                "source": {"path": absolute_path(path)},
                "breakpoints": breakpoints,
                "lines": lines,
                "sourceModified": false,
            }),
        )
    }

    pub fn configuration_done(&mut self) -> Result<i64, DapError> {
        self.request("configurationDone", json!({}))
    }

    pub fn continue_(&mut self, thread_id: i64) -> Result<i64, DapError> {
        self.request("continue", json!({"threadId": thread_id}))
    }

    pub fn next(&mut self, thread_id: i64) -> Result<i64, DapError> {
        self.request("next", json!({"threadId": thread_id}))
    }

    pub fn step_in(&mut self, thread_id: i64) -> Result<i64, DapError> {
        self.request("stepIn", json!({"threadId": thread_id}))
    }

    pub fn step_out(&mut self, thread_id: i64) -> Result<i64, DapError> {
        self.request("stepOut", json!({"threadId": thread_id}))
    }

    pub fn disconnect(&mut self) -> Result<i64, DapError> {
        self.request("disconnect", json!({"terminateDebuggee": false}))
    }

    pub fn wait_response(&mut self, request_seq: i64) -> Result<Value, DapError> {
        loop {
            let message = read_frame(&mut self.stdout)?;
            if message.get("type").and_then(Value::as_str) == Some("response")
                && message.get("request_seq").and_then(Value::as_i64) == Some(request_seq)
            {
                if message.get("success").and_then(Value::as_bool) == Some(false) {
                    let message = message
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("adapter request failed");
                    return Err(DapError::Protocol(message.to_owned()));
                }
                return Ok(message);
            }
            queue_event(&mut self.events, message);
        }
    }

    pub fn read_event(&mut self) -> Result<Value, DapError> {
        if let Some(event) = self.events.pop_front() {
            return Ok(event);
        }
        read_frame(&mut self.stdout)
    }

    pub fn try_take_event(&mut self) -> Option<Value> {
        self.events.pop_front()
    }

    /// Number of adapter messages buffered behind the current request.
    pub fn queued_events(&self) -> usize {
        self.events.len()
    }

    pub fn kill(&mut self) {
        let _ = self.process.kill();
    }
}

/// Buffer one adapter message, dropping the oldest once [`MAX_DAP_EVENTS`] is
/// reached so a chatty adapter cannot grow memory without bound.
fn queue_event(events: &mut VecDeque<Value>, message: Value) {
    if events.len() >= MAX_DAP_EVENTS {
        events.pop_front();
    }
    events.push_back(message);
}

impl Drop for DapClient {
    fn drop(&mut self) {
        self.kill();
    }
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn write_frame(writer: &mut impl Write, message: &Value) -> Result<(), DapError> {
    let body = serde_json::to_vec(message)?;
    if body.len() > MAX_DAP_MESSAGE {
        return Err(DapError::Protocol("outgoing message too large".to_owned()));
    }
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Value, DapError> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Err(DapError::AdapterExited);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(DapError::Protocol("malformed header".to_owned()));
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| DapError::Protocol("invalid content-length".to_owned()))?,
            );
        }
    }
    let length =
        content_length.ok_or_else(|| DapError::Protocol("missing content-length".to_owned()))?;
    if length > MAX_DAP_MESSAGE {
        return Err(DapError::Protocol("incoming message too large".to_owned()));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frames_round_trip() {
        let message = json!({"seq": 1, "type": "request", "command": "initialize"});
        let mut encoded = Vec::new();
        write_frame(&mut encoded, &message).expect("encode frame");
        let mut reader = Cursor::new(encoded);
        let decoded = read_frame(&mut reader).expect("decode frame");
        assert_eq!(decoded, message);
    }

    #[test]
    fn rejects_missing_content_length() {
        let mut reader = Cursor::new(b"X-Test: 1\r\n\r\n{}".to_vec());
        assert!(matches!(
            read_frame(&mut reader),
            Err(DapError::Protocol(_))
        ));
    }

    #[test]
    fn event_queue_drops_oldest_when_full() {
        let mut events = VecDeque::new();
        for seq in 0..(MAX_DAP_EVENTS + 10) {
            queue_event(&mut events, json!({ "seq": seq }));
        }
        assert_eq!(events.len(), MAX_DAP_EVENTS);
        assert_eq!(events.front().expect("front")["seq"], json!(10));
        assert_eq!(
            events.back().expect("back")["seq"],
            json!(MAX_DAP_EVENTS + 9)
        );
    }

    #[cfg(unix)]
    #[test]
    fn waiting_for_response_keeps_unsolicited_events_bounded() {
        let directory = test_directory("anvil-dap-events");
        let adapter = directory.join("adapter.py");
        std::fs::write(
            &adapter,
            r#"import json, sys
def send(message):
    data = json.dumps(message).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(data)}\r\n\r\n".encode() + data)
    sys.stdout.buffer.flush()
while True:
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            sys.exit(0)
        if line in (b"\r\n", b"\n"):
            break
        key, value = line.decode().split(":", 1)
        headers[key.lower()] = value.strip()
    request = json.loads(sys.stdin.buffer.read(int(headers["content-length"])))
    for seq in range(1, 301):
        send({"seq": seq, "type": "event", "event": "output", "body": {}})
    send({
        "seq": 999,
        "type": "response",
        "request_seq": request["seq"],
        "success": True,
        "command": request["command"],
        "body": {},
    })
"#,
        )
        .expect("write adapter");
        let mut client = DapClient::spawn(
            Path::new("python3"),
            &[adapter.display().to_string()],
            &directory,
        )
        .expect("spawn adapter");
        let sequence = client.initialize().expect("initialize");
        let response = client.wait_response(sequence).expect("initialize response");
        assert_eq!(response["command"], "initialize");
        assert_eq!(client.queued_events(), MAX_DAP_EVENTS);
        // The oldest 44 of the 300 unsolicited events were dropped.
        assert_eq!(
            client.try_take_event().expect("queued event")["seq"],
            json!(45)
        );
        client.kill();
        let _ = std::fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    fn test_directory(prefix: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&directory).expect("create temp dir");
        directory
    }

    #[test]
    fn lldb_dap_initializes() {
        if !Path::new("/usr/bin/lldb-dap").is_file() {
            return;
        }
        let mut client =
            DapClient::spawn(Path::new("/usr/bin/lldb-dap"), &[], &std::env::temp_dir())
                .expect("spawn");
        let sequence = client.initialize().expect("initialize");
        let response = client.wait_response(sequence).expect("response");
        assert_eq!(response["command"], "initialize");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn shutdown_kills_adapter_process_group() {
        let directory = std::env::temp_dir().join(format!(
            "anvil-dap-group-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&directory).expect("create temp dir");
        let mut client = DapClient::spawn(
            Path::new("/bin/sh"),
            &[
                "-c".to_owned(),
                "sleep 30 & echo $! > child.pid; wait".to_owned(),
            ],
            &directory,
        )
        .expect("spawn adapter");
        let pid_file = directory.join("child.pid");
        for _ in 0..100 {
            if pid_file.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let pid = std::fs::read_to_string(&pid_file).expect("child pid");
        let proc = PathBuf::from("/proc").join(pid.trim());
        assert!(proc.exists(), "child did not start");
        client.kill();
        for _ in 0..100 {
            if !proc.exists() {
                let _ = std::fs::remove_dir_all(directory);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = std::fs::remove_dir_all(directory);
        panic!("adapter process group survived shutdown");
    }

    #[cfg(unix)]
    #[test]
    fn talks_to_separate_adapter_process() {
        let directory = std::env::temp_dir().join(format!(
            "anvil-dap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&directory).expect("create temp dir");
        let adapter = directory.join("adapter.py");
        std::fs::write(
            &adapter,
            r#"import json, sys
while True:
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            sys.exit(0)
        if line in (b"\r\n", b"\n"):
            break
        key, value = line.decode().split(":", 1)
        headers[key.lower()] = value.strip()
    body = sys.stdin.buffer.read(int(headers["content-length"]))
    request = json.loads(body)
    response = {
        "seq": 1,
        "type": "response",
        "request_seq": request["seq"],
        "success": True,
        "command": request["command"],
        "body": {},
    }
    data = json.dumps(response).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(data)}\r\n\r\n".encode() + data)
    sys.stdout.buffer.flush()
"#,
        )
        .expect("write adapter");
        let mut client = DapClient::spawn(
            Path::new("python3"),
            &[adapter.display().to_string()],
            &directory,
        )
        .expect("spawn adapter");
        let sequence = client.initialize().expect("send initialize");
        let response = client.wait_response(sequence).expect("initialize response");
        assert_eq!(response["command"], "initialize");
        client.kill();
        let _ = std::fs::remove_dir_all(directory);
    }
}

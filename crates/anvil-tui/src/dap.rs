//! Minimal Debug Adapter Protocol client for a separate adapter process.

use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

pub const MAX_DAP_MESSAGE: usize = 8 * 1024 * 1024;

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
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_seq: i64,
    events: VecDeque<Value>,
    adapter_id: String,
}

impl DapClient {
    pub fn spawn(program: &Path, args: &[String], cwd: &Path) -> Result<Self, DapError> {
        let mut child = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| DapError::Protocol("missing adapter stdin".to_owned()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| DapError::Protocol("missing adapter stdout".to_owned()))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_seq: 1,
            events: VecDeque::new(),
            adapter_id: "anvil".to_owned(),
        })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
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
            self.events.push_back(message);
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

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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

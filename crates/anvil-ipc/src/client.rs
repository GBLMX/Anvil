//! Synchronous client for the per-session workspace gateway.
//!
//! The client owns one framed connection and sends each request in order. It
//! implements the chunked file protocol from ADR-0011; it does not fall back to
//! direct filesystem access.

use std::collections::VecDeque;
use std::path::Path;

use base64::{Engine as _, engine::general_purpose};

use crate::{Connection, EnvelopeError, FrameError, JSONRPC_VERSION, Request, Response, codes};

/// Bytes requested per read or accepted per write.
pub const FILE_CHUNK: usize = 5 * 1024 * 1024 + 512 * 1024;

/// Raw binary read chunk; one flag byte plus this fits the 8 MiB frame.
pub const BINARY_READ_CHUNK: usize = 7 * 1024 * 1024;

/// A gateway client failure.
#[derive(Debug)]
pub enum ClientError {
    /// Socket or frame I/O failed.
    Io(std::io::ErrorKind),
    /// Framing failed.
    Frame(FrameError),
    /// JSON encoding or decoding failed.
    Json(serde_json::Error),
    /// Envelope validation failed.
    Envelope(EnvelopeError),
    /// The gateway returned a JSON-RPC error.
    Rpc { code: i32, message: String },
    /// The gateway returned an unexpected response shape.
    Protocol(&'static str),
    /// A pipeline failed; the connection may contain unread frames.
    Poisoned,
    /// A requested file exceeded the caller's limit.
    Limit,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(kind) => write!(formatter, "gateway I/O failed: {kind:?}"),
            Self::Frame(error) => write!(formatter, "gateway frame failed: {error}"),
            Self::Json(error) => write!(formatter, "gateway JSON failed: {error}"),
            Self::Envelope(error) => write!(formatter, "gateway envelope failed: {error}"),
            Self::Rpc { code, message } => write!(formatter, "gateway error {code}: {message}"),
            Self::Protocol(message) => write!(formatter, "gateway protocol error: {message}"),
            Self::Poisoned => write!(formatter, "gateway protocol error: restart gateway and TUI"),
            Self::Limit => write!(formatter, "file exceeds the requested limit"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<FrameError> for ClientError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<EnvelopeError> for ClientError {
    fn from(error: EnvelopeError) -> Self {
        Self::Envelope(error)
    }
}

impl From<std::io::Error> for ClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.kind())
    }
}

/// One entry returned by `sys.read_dir`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}

/// One chunk returned by `sys.read_file`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadChunk {
    pub data: Vec<u8>,
    pub next_offset: u64,
    pub eof: bool,
    pub seq: u64,
}

/// A connected, handshaken gateway client.
#[derive(Debug)]
pub struct GatewayClient {
    connection: Connection,
    next_id: u64,
    poisoned: bool,
    notifications: VecDeque<Request>,
}

impl GatewayClient {
    /// Connect to `socket` and perform `session.hello`.
    pub fn connect(socket: impl AsRef<Path>, token: &[u8; 32]) -> Result<Self, ClientError> {
        let mut client = Self {
            connection: Connection::connect(socket)?,
            next_id: 1,
            poisoned: false,
            notifications: VecDeque::new(),
        };
        client.request(
            "session.hello",
            serde_json::json!({
                "token": general_purpose::URL_SAFE_NO_PAD.encode(token),
                "api_version": "0.1.0",
                "protocol": "anvil-jsonrpc-1",
            }),
        )?;
        Ok(client)
    }

    /// Drain server-initiated notifications received while waiting for
    /// responses.
    pub fn take_notifications(&mut self) -> Vec<Request> {
        self.notifications.drain(..).collect()
    }

    /// Start a gateway-supervised LSP process.
    pub fn lsp_start(&mut self, tool: &str, args: &[String]) -> Result<(), ClientError> {
        self.request("lsp.start", serde_json::json!({"tool": tool, "args": args}))?;
        Ok(())
    }

    /// Write one raw LSP JSON message to the supervised process.
    pub fn lsp_send(&mut self, data: &[u8]) -> Result<(), ClientError> {
        let message: serde_json::Value = serde_json::from_slice(data)?;
        self.request("lsp.send", serde_json::json!({"data": message}))?;
        Ok(())
    }

    /// Stop the supervised LSP process.
    pub fn lsp_stop(&mut self) -> Result<(), ClientError> {
        self.request("lsp.stop", serde_json::json!({}))?;
        Ok(())
    }

    /// Block for one server-initiated notification.
    pub fn recv_notification(&mut self) -> Result<Option<Request>, ClientError> {
        let Some(frame) = self.connection.recv()? else {
            return Ok(None);
        };
        if let Some(notification) = parse_notification(&frame) {
            return Ok(Some(notification));
        }
        Err(ClientError::Protocol(
            "unexpected response while waiting for notification",
        ))
    }

    /// Read at most `max_bytes` from `path` at `offset`.
    pub fn read_file(
        &mut self,
        path: &str,
        offset: u64,
        max_bytes: usize,
    ) -> Result<ReadChunk, ClientError> {
        if max_bytes == 0 || max_bytes > FILE_CHUNK {
            return Err(ClientError::Protocol("read chunk size"));
        }
        let result = self.request(
            "sys.read_file",
            serde_json::json!({
                "path": path,
                "offset": offset,
                "max_bytes": max_bytes,
            }),
        )?;
        let encoded = result
            .get("data")
            .and_then(serde_json::Value::as_str)
            .ok_or(ClientError::Protocol("read data"))?;
        let data = general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| ClientError::Protocol("read base64"))?;
        let next_offset = result
            .get("next_offset")
            .and_then(serde_json::Value::as_u64)
            .ok_or(ClientError::Protocol("read next_offset"))?;
        let eof = result
            .get("eof")
            .and_then(serde_json::Value::as_bool)
            .ok_or(ClientError::Protocol("read eof"))?;
        if next_offset != offset + data.len() as u64 {
            return Err(ClientError::Protocol("read offset"));
        }
        Ok(ReadChunk {
            data,
            next_offset,
            eof,
            seq: 0,
        })
    }

    /// Read a complete file, rejecting it once it exceeds `limit`.
    pub fn read_all(&mut self, path: &str, limit: usize) -> Result<Vec<u8>, ClientError> {
        let mut data = Vec::new();
        let mut offset = 0;
        loop {
            let chunk = self.read_file(path, offset, FILE_CHUNK)?;
            if data.len().saturating_add(chunk.data.len()) > limit {
                return Err(ClientError::Limit);
            }
            data.extend_from_slice(&chunk.data);
            if chunk.eof {
                return Ok(data);
            }
            if chunk.next_offset <= offset {
                return Err(ClientError::Protocol("read made no progress"));
            }
            offset = chunk.next_offset;
        }
    }

    /// Write one chunk at `offset`; `eof` commits the atomic replacement.
    pub fn write_file(
        &mut self,
        path: &str,
        offset: u64,
        data: &[u8],
        eof: bool,
    ) -> Result<u64, ClientError> {
        if data.len() > FILE_CHUNK {
            return Err(ClientError::Protocol("write chunk size"));
        }
        let result = self.request(
            "sys.write_file",
            serde_json::json!({
                "path": path,
                "offset": offset,
                "data": general_purpose::STANDARD.encode(data),
                "eof": eof,
            }),
        )?;
        let next_offset = result
            .get("next_offset")
            .and_then(serde_json::Value::as_u64)
            .ok_or(ClientError::Protocol("write next_offset"))?;
        if next_offset != offset + data.len() as u64 {
            return Err(ClientError::Protocol("write offset"));
        }
        Ok(next_offset)
    }

    /// Write a complete file in 5.5 MiB chunks.
    pub fn write_all(&mut self, path: &str, data: &[u8]) -> Result<(), ClientError> {
        if data.is_empty() {
            self.write_file(path, 0, &[], true)?;
            return Ok(());
        }
        let chunk_count = data.len().div_ceil(FILE_CHUNK);
        for (index, chunk) in data.chunks(FILE_CHUNK).enumerate() {
            let offset = (index * FILE_CHUNK) as u64;
            self.write_file(path, offset, chunk, index + 1 == chunk_count)?;
        }
        Ok(())
    }

    /// List one workspace directory through the gateway.
    pub fn read_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, ClientError> {
        let result = self.request("sys.read_dir", serde_json::json!({ "path": path }))?;
        let entries = result
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .ok_or(ClientError::Protocol("read_dir entries"))?;
        entries
            .iter()
            .map(|entry| {
                let name = entry
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(ClientError::Protocol("read_dir name"))?;
                let is_dir = entry
                    .get("is_dir")
                    .and_then(serde_json::Value::as_bool)
                    .ok_or(ClientError::Protocol("read_dir is_dir"))?;
                Ok(DirEntry {
                    name: name.to_owned(),
                    is_dir,
                })
            })
            .collect()
    }

    /// Return the workspace-relative regular file size through the gateway.
    pub fn file_size(&mut self, path: &str) -> Result<u64, ClientError> {
        let result = self.request("sys.stat", serde_json::json!({ "path": path }))?;
        result
            .get("size")
            .and_then(serde_json::Value::as_u64)
            .ok_or(ClientError::Protocol("stat size"))
    }

    /// Read a complete file with a bounded pipeline of binary requests.
    pub fn read_all_binary_pipelined(
        &mut self,
        path: &str,
        size: u64,
        limit: usize,
        window: usize,
    ) -> Result<Vec<u8>, ClientError> {
        if size > limit as u64 {
            return Err(ClientError::Limit);
        }
        if size == 0 {
            return Ok(Vec::new());
        }
        let result = self.read_all_binary_pipelined_inner(path, size, window);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    fn read_all_binary_pipelined_inner(
        &mut self,
        path: &str,
        size: u64,
        window: usize,
    ) -> Result<Vec<u8>, ClientError> {
        let total = size.div_ceil(BINARY_READ_CHUNK as u64) as usize;
        let window = window.clamp(1, 8);
        let mut chunks: Vec<Option<ReadChunk>> = (0..total).map(|_| None).collect();
        let mut pending: Vec<(String, u64, u64)> = Vec::with_capacity(window);
        let mut next = 0_usize;
        while next < total || !pending.is_empty() {
            while pending.len() < window && next < total {
                let offset = next as u64 * BINARY_READ_CHUNK as u64;
                let seq = next as u64;
                let id = self.send_binary_request(path, offset, BINARY_READ_CHUNK, seq)?;
                pending.push((id, offset, seq));
                next += 1;
            }
            let frame = loop {
                let frame = self
                    .connection
                    .recv()?
                    .ok_or(ClientError::Protocol("gateway closed"))?;
                if let Some(notification) = parse_notification(&frame) {
                    self.notifications.push_back(notification);
                    continue;
                }
                break frame;
            };
            if frame.first() == Some(&b'{') {
                let response: Response = serde_json::from_slice(&frame)?;
                response.validate()?;
                let id = response
                    .id
                    .as_deref()
                    .ok_or(ClientError::Protocol("response id"))?;
                let index = pending
                    .iter()
                    .position(|(pending_id, _, _)| pending_id == id)
                    .ok_or(ClientError::Protocol("unexpected response id"))?;
                pending.remove(index);
                if let Some(error) = response.error {
                    return Err(ClientError::Rpc {
                        code: error.code,
                        message: error.message,
                    });
                }
                return Err(ClientError::Protocol("binary frame expected"));
            }
            let (offset, seq, chunk) = parse_binary_frame(&frame)?;
            let index = pending
                .iter()
                .position(|(_, pending_offset, pending_seq)| {
                    *pending_offset == offset && *pending_seq == seq
                })
                .ok_or(ClientError::Protocol("unexpected binary header"))?;
            pending.remove(index);
            if chunk.next_offset != offset + chunk.data.len() as u64 {
                return Err(ClientError::Protocol("binary offset"));
            }
            let slot = chunks
                .get_mut(seq as usize)
                .ok_or(ClientError::Protocol("binary sequence"))?;
            *slot = Some(chunk);
        }
        let mut data = Vec::with_capacity(size as usize);
        for chunk in chunks {
            let chunk = chunk.ok_or(ClientError::Protocol("missing binary chunk"))?;
            data.extend_from_slice(&chunk.data);
        }
        if data.len() as u64 != size {
            return Err(ClientError::Protocol("binary size mismatch"));
        }
        Ok(data)
    }

    /// Write a complete file using binary data frames.
    pub fn write_all_binary(&mut self, path: &str, data: &[u8]) -> Result<(), ClientError> {
        if data.is_empty() {
            return self.write_all(path, data);
        }
        let chunk_count = data.len().div_ceil(BINARY_READ_CHUNK);
        for (index, chunk) in data.chunks(BINARY_READ_CHUNK).enumerate() {
            let offset = (index * BINARY_READ_CHUNK) as u64;
            let seq = index as u64;
            let eof = index + 1 == chunk_count;
            let id = self.send_request(
                "sys.write_file",
                serde_json::json!({
                    "path": path,
                    "offset": offset,
                    "eof": eof,
                    "binary": true,
                    "seq": seq,
                    "data_len": chunk.len(),
                }),
            )?;
            self.connection.send(chunk)?;
            let frame = loop {
                let frame = self
                    .connection
                    .recv()?
                    .ok_or(ClientError::Protocol("gateway closed"))?;
                if let Some(notification) = parse_notification(&frame) {
                    self.notifications.push_back(notification);
                    continue;
                }
                break frame;
            };
            if frame.first() == Some(&b'{') {
                parse_response(&frame, &id)?;
                return Err(ClientError::Protocol("binary write response expected"));
            }
            if frame.len() != 17 {
                return Err(ClientError::Protocol("short binary write response"));
            }
            let frame_seq = u64::from_be_bytes(frame[1..9].try_into().expect("seq"));
            let next_offset = u64::from_be_bytes(frame[9..17].try_into().expect("offset"));
            if frame_seq != seq || next_offset != offset + chunk.len() as u64 {
                return Err(ClientError::Protocol("binary write mismatch"));
            }
        }
        Ok(())
    }

    /// Read one raw binary chunk with sequence zero.
    pub fn read_file_binary(
        &mut self,
        path: &str,
        offset: u64,
        max_bytes: usize,
    ) -> Result<ReadChunk, ClientError> {
        self.read_file_binary_seq(path, offset, max_bytes, 0)
    }

    /// Read one raw binary chunk. The frame is
    /// `[flags][seq u64][offset u64][raw bytes]`.
    pub fn read_file_binary_seq(
        &mut self,
        path: &str,
        offset: u64,
        max_bytes: usize,
        seq: u64,
    ) -> Result<ReadChunk, ClientError> {
        if max_bytes == 0 || max_bytes > BINARY_READ_CHUNK {
            return Err(ClientError::Protocol("read chunk size"));
        }
        let id = self.send_binary_request(path, offset, max_bytes, seq)?;
        let frame = loop {
            let frame = self
                .connection
                .recv()?
                .ok_or(ClientError::Protocol("gateway closed"))?;
            if let Some(notification) = parse_notification(&frame) {
                self.notifications.push_back(notification);
                continue;
            }
            break frame;
        };
        if frame.first() == Some(&b'{') {
            let _ = parse_response(&frame, &id)?;
            return Err(ClientError::Protocol("binary frame expected"));
        }
        let (frame_offset, frame_seq, chunk) = parse_binary_frame(&frame)?;
        if frame_seq != seq || frame_offset != offset {
            return Err(ClientError::Protocol("binary header mismatch"));
        }
        Ok(chunk)
    }

    /// Read a complete file using raw binary frames.
    pub fn read_all_binary(&mut self, path: &str, limit: usize) -> Result<Vec<u8>, ClientError> {
        let mut data = Vec::new();
        let mut offset = 0;
        loop {
            let chunk = self.read_file_binary(path, offset, BINARY_READ_CHUNK)?;
            if data.len().saturating_add(chunk.data.len()) > limit {
                return Err(ClientError::Limit);
            }
            data.extend_from_slice(&chunk.data);
            if chunk.eof {
                return Ok(data);
            }
            if chunk.next_offset <= offset {
                return Err(ClientError::Protocol("read made no progress"));
            }
            offset = chunk.next_offset;
        }
    }

    fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let id = self.send_request(method, params)?;
        loop {
            let frame = self
                .connection
                .recv()?
                .ok_or(ClientError::Protocol("gateway closed"))?;
            if let Some(notification) = parse_notification(&frame) {
                self.notifications.push_back(notification);
                continue;
            }
            return parse_response(&frame, &id);
        }
    }

    fn send_binary_request(
        &mut self,
        path: &str,
        offset: u64,
        max_bytes: usize,
        seq: u64,
    ) -> Result<String, ClientError> {
        self.send_request(
            "sys.read_file",
            serde_json::json!({
                "path": path,
                "offset": offset,
                "max_bytes": max_bytes,
                "binary": true,
                "seq": seq,
            }),
        )
    }

    fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<String, ClientError> {
        if self.poisoned {
            return Err(ClientError::Poisoned);
        }
        let id = format!("anvil-{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let request = Request {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: Some(id.clone()),
            method: method.to_owned(),
            params,
            meta: None,
        };
        request.validate()?;
        self.connection.send(&serde_json::to_vec(&request)?)?;
        Ok(id)
    }
}

fn parse_binary_frame(frame: &[u8]) -> Result<(u64, u64, ReadChunk), ClientError> {
    if frame.len() < 17 {
        return Err(ClientError::Protocol("short binary header"));
    }
    let flags = frame[0];
    let seq = u64::from_be_bytes(frame[1..9].try_into().expect("seq"));
    let offset = u64::from_be_bytes(frame[9..17].try_into().expect("offset"));
    let data = &frame[17..];
    Ok((
        offset,
        seq,
        ReadChunk {
            data: data.to_vec(),
            next_offset: offset + data.len() as u64,
            eof: flags & 1 != 0,
            seq,
        },
    ))
}

fn parse_notification(frame: &[u8]) -> Option<Request> {
    if frame.first() != Some(&b'{') {
        return None;
    }
    let request: Request = serde_json::from_slice(frame).ok()?;
    if request.is_notification() && request.validate().is_ok() {
        Some(request)
    } else {
        None
    }
}

fn parse_response(frame: &[u8], id: &str) -> Result<serde_json::Value, ClientError> {
    let response: Response = serde_json::from_slice(frame)?;
    response.validate()?;
    if response.id.as_deref() != Some(id) {
        return Err(ClientError::Protocol("response id"));
    }
    if let Some(error) = response.error {
        return Err(ClientError::Rpc {
            code: error.code,
            message: error.message,
        });
    }
    Ok(response.result.unwrap_or(serde_json::Value::Null))
}

/// True when the error is the gateway's policy-denied code.
pub fn is_policy_denied(error: &ClientError) -> bool {
    matches!(
        error,
        ClientError::Rpc { code, .. } if *code == codes::POLICY_DENIED
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Listener, Request};
    use serde_json::{Value, json};
    use std::{
        fs,
        path::PathBuf,
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn socket_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "anvil-ipc-{}-{name}-{nanos}.sock",
            std::process::id()
        ))
    }

    fn receive(connection: &mut Connection) -> Request {
        let frame = connection.recv().expect("frame").expect("body");
        serde_json::from_slice(&frame).expect("request")
    }

    fn respond(connection: &mut Connection, id: &str, result: Value) {
        let response = Response {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: Some(id.to_owned()),
            result: Some(result),
            error: None,
        };
        connection
            .send(&serde_json::to_vec(&response).expect("response"))
            .expect("send");
    }

    fn respond_error(connection: &mut Connection, id: &str, code: i32) {
        let response = Response {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: Some(id.to_owned()),
            result: None,
            error: Some(crate::ErrorObject {
                code,
                message: "denied".to_owned(),
                data: None,
            }),
        };
        connection
            .send(&serde_json::to_vec(&response).expect("response"))
            .expect("send");
    }

    #[test]
    fn handshakes_and_reads_a_chunk() {
        let path = socket_path("read");
        let listener = Listener::bind(&path).expect("bind");
        let server = thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            let hello = receive(&mut connection);
            assert_eq!(hello.method, "session.hello");
            respond(
                &mut connection,
                hello.id.as_deref().expect("id"),
                json!({"session_id": "test"}),
            );
            let read = receive(&mut connection);
            assert_eq!(read.method, "sys.read_file");
            assert_eq!(read.params["path"], "file.txt");
            respond(
                &mut connection,
                read.id.as_deref().expect("id"),
                json!({
                    "data": general_purpose::STANDARD.encode(b"abc"),
                    "next_offset": 3,
                    "eof": true
                }),
            );
        });

        let mut client = GatewayClient::connect(&path, &[7; 32]).expect("connect");
        let chunk = client.read_file("file.txt", 0, 4).expect("read");
        assert_eq!(chunk.data, b"abc");
        assert_eq!(chunk.next_offset, 3);
        assert!(chunk.eof);
        server.join().expect("server");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn queues_server_notifications_before_response() {
        let path = socket_path("notification");
        let listener = Listener::bind(&path).expect("bind");
        let server = thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            let hello = receive(&mut connection);
            respond(
                &mut connection,
                hello.id.as_deref().expect("id"),
                json!({"session_id": "test"}),
            );
            let read = receive(&mut connection);
            let notification = json!({
                "jsonrpc": "2.0",
                "method": "lsp.diagnostics",
                "params": {"uri": "file:///a.rs"}
            });
            connection
                .send(&serde_json::to_vec(&notification).expect("notification"))
                .expect("send");
            respond(
                &mut connection,
                read.id.as_deref().expect("id"),
                json!({"data": "", "next_offset": 0, "eof": true}),
            );
        });

        let mut client = GatewayClient::connect(&path, &[7; 32]).expect("connect");
        client.read_file("file.txt", 0, 4).expect("read");
        let notifications = client.take_notifications();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].method, "lsp.diagnostics");
        server.join().expect("server");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn reads_a_binary_frame() {
        let path = socket_path("binary");
        let listener = Listener::bind(&path).expect("bind");
        let server = thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            let hello = receive(&mut connection);
            respond(
                &mut connection,
                hello.id.as_deref().expect("id"),
                json!({"session_id": "test"}),
            );
            let read = receive(&mut connection);
            assert_eq!(read.method, "sys.read_file");
            assert_eq!(read.params["binary"], true);
            let mut payload = vec![1];
            payload.extend_from_slice(&0_u64.to_be_bytes());
            payload.extend_from_slice(&0_u64.to_be_bytes());
            payload.extend_from_slice(b"hi");
            connection.send(&payload).expect("binary");
        });

        let mut client = GatewayClient::connect(&path, &[7; 32]).expect("connect");
        let chunk = client
            .read_file_binary("file.txt", 0, 16)
            .expect("binary read");
        assert_eq!(chunk.data, b"hi");
        assert_eq!(chunk.next_offset, 2);
        assert!(chunk.eof);
        server.join().expect("server");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn pipeline_error_poisons_the_client() {
        let path = socket_path("pipeline-error");
        let listener = Listener::bind(&path).expect("bind");
        let server = thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            let hello = receive(&mut connection);
            respond(
                &mut connection,
                hello.id.as_deref().expect("id"),
                json!({"session_id": "test"}),
            );
            let first = receive(&mut connection);
            let _second = receive(&mut connection);
            respond_error(
                &mut connection,
                first.id.as_deref().expect("id"),
                codes::POLICY_DENIED,
            );
            let mut payload = vec![1];
            payload.extend_from_slice(&1_u64.to_be_bytes());
            payload.extend_from_slice(&(BINARY_READ_CHUNK as u64).to_be_bytes());
            payload.push(b'x');
            connection.send(&payload).expect("second binary");
        });

        let mut client = GatewayClient::connect(&path, &[7; 32]).expect("connect");
        let size = BINARY_READ_CHUNK as u64 + 1;
        let error = client
            .read_all_binary_pipelined("file.txt", size, usize::MAX, 2)
            .expect_err("pipeline error");
        assert!(is_policy_denied(&error));
        let poisoned = client.file_size("file.txt").expect_err("poisoned");
        assert!(poisoned.to_string().contains("restart gateway"));
        server.join().expect("server");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn writes_chunks_and_reports_policy_errors() {
        let path = socket_path("write");
        let listener = Listener::bind(&path).expect("bind");
        let server = thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            let hello = receive(&mut connection);
            respond(
                &mut connection,
                hello.id.as_deref().expect("id"),
                json!({"session_id": "test"}),
            );
            let first = receive(&mut connection);
            assert_eq!(first.method, "sys.write_file");
            assert_eq!(first.params["offset"], 0);
            assert_eq!(first.params["eof"], false);
            respond(
                &mut connection,
                first.id.as_deref().expect("id"),
                json!({"next_offset": 2}),
            );
            let second = receive(&mut connection);
            assert_eq!(second.params["offset"], 2);
            assert_eq!(second.params["eof"], true);
            respond(
                &mut connection,
                second.id.as_deref().expect("id"),
                json!({"next_offset": 4}),
            );
            let denied = receive(&mut connection);
            respond_error(
                &mut connection,
                denied.id.as_deref().expect("id"),
                codes::POLICY_DENIED,
            );
        });

        let mut client = GatewayClient::connect(&path, &[7; 32]).expect("connect");
        assert_eq!(
            client
                .write_file("file.txt", 0, b"ab", false)
                .expect("first"),
            2
        );
        assert_eq!(
            client
                .write_file("file.txt", 2, b"cd", true)
                .expect("second"),
            4
        );
        let error = client.read_file("secret", 0, 4).expect_err("denied");
        assert!(is_policy_denied(&error));
        server.join().expect("server");
        let _ = fs::remove_file(path);
    }
}

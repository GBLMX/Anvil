//! LSP wire codec and diagnostics model.
//!
//! This module deliberately does not spawn a language server. Process ownership
//! and sandboxing belong to the gateway/host layer; this layer only transports
//! and parses LSP messages once a supervised transport exists.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::PathBuf;

#[derive(Debug)]
pub enum LspError {
    Io(std::io::Error),
    MissingHeader,
    InvalidHeader,
    MissingBody,
    Json(serde_json::Error),
}

impl std::fmt::Display for LspError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "lsp I/O: {error}"),
            Self::MissingHeader => formatter.write_str("lsp missing Content-Length"),
            Self::InvalidHeader => formatter.write_str("lsp invalid Content-Length"),
            Self::MissingBody => formatter.write_str("lsp truncated body"),
            Self::Json(error) => write!(formatter, "lsp JSON: {error}"),
        }
    }
}

impl std::error::Error for LspError {}

impl From<std::io::Error> for LspError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for LspError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: Option<u32>,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PublishDiagnostics {
    uri: String,
    diagnostics: Vec<Diagnostic>,
}

/// A byte-oriented LSP transport. The caller owns the process and its pipes.
pub struct LspConnection<R: BufRead, W: Write> {
    reader: R,
    writer: W,
    next_id: u64,
}

impl<R: BufRead, W: Write> LspConnection<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader,
            writer,
            next_id: 1,
        }
    }

    pub fn send(&mut self, message: &Value) -> Result<(), LspError> {
        let body = serde_json::to_vec(message)?;
        write!(self.writer, "Content-Length: {}\r\n\r\n", body.len())?;
        self.writer.write_all(&body)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<u64, LspError> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;
        Ok(id)
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<(), LspError> {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }))
    }

    pub fn recv(&mut self) -> Result<Option<Value>, LspError> {
        let Some(body) = read_body(&mut self.reader)? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_slice(&body)?))
    }
}

/// Reads one `Content-Length` framed LSP message.
pub fn read_body<R: BufRead>(reader: &mut R) -> Result<Option<Vec<u8>>, LspError> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(LspError::InvalidHeader);
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| LspError::InvalidHeader)?,
            );
        }
    }
    let length = content_length.ok_or(LspError::MissingHeader)?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Extracts diagnostics from a `textDocument/publishDiagnostics` notification.
pub fn publish_diagnostics(message: &Value) -> Option<(PathBuf, Vec<Diagnostic>)> {
    if message.get("method")?.as_str()? != "textDocument/publishDiagnostics" {
        return None;
    }
    let params = message.get("params")?.clone();
    let parsed: PublishDiagnostics = serde_json::from_value(params).ok()?;
    Some((PathBuf::from(parsed.uri), parsed.diagnostics))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trips_content_length_frames() {
        let mut output = Vec::new();
        {
            let mut connection = LspConnection::new(Cursor::new(Vec::new()), &mut output);
            connection
                .notify("initialized", serde_json::json!({}))
                .expect("notify");
        }
        let mut reader = Cursor::new(output);
        let body = read_body(&mut reader).expect("read").expect("frame");
        let value: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["method"], "initialized");
    }

    #[test]
    fn parses_publish_diagnostics() {
        let message = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": "file:///tmp/a.rs",
                "diagnostics": [{
                    "range": {
                        "start": {"line": 1, "character": 2},
                        "end": {"line": 1, "character": 3}
                    },
                    "severity": 1,
                    "message": "mismatched types",
                    "source": "rust-analyzer"
                }]
            }
        });
        let (path, diagnostics) = publish_diagnostics(&message).expect("diagnostics");
        assert_eq!(path, PathBuf::from("file:///tmp/a.rs"));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range.start.line, 1);
    }

    #[test]
    fn rejects_missing_content_length() {
        let mut reader = Cursor::new(b"Content-Type: application/json\r\n\r\n{}".to_vec());
        assert!(matches!(
            read_body(&mut reader),
            Err(LspError::MissingHeader)
        ));
    }
}

//! Bounded text preview reads.

use std::time::{Duration, Instant};

use anvil_ipc::GatewayClient;

pub(crate) const PREVIEW_MAX_BYTES: usize = 64 * 1024;
pub const PREVIEW_MAX_LINES: usize = 200;
const PREVIEW_TIMEOUT: Duration = Duration::from_millis(500);
const PREVIEW_CHUNK: usize = 16 * 1024;

/// Bounded text preview returned by the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewData {
    pub lines: Vec<String>,
    pub truncated: bool,
}

pub(super) fn read_preview(client: &mut GatewayClient, path: &str) -> Result<PreviewData, String> {
    let started = Instant::now();
    let size = client.file_size(path).map_err(|error| error.to_string())?;
    let limit = usize::try_from(size)
        .unwrap_or(usize::MAX)
        .min(PREVIEW_MAX_BYTES);
    let mut data = Vec::new();
    let mut offset = 0;
    let mut truncated = size > PREVIEW_MAX_BYTES as u64;
    while data.len() < limit {
        if started.elapsed() > PREVIEW_TIMEOUT {
            return Err("preview timed out".to_owned());
        }
        let remaining = limit - data.len();
        let chunk = client
            .read_file(path, offset, remaining.min(PREVIEW_CHUNK))
            .map_err(|error| error.to_string())?;
        if chunk.data.is_empty() {
            break;
        }
        data.extend_from_slice(&chunk.data);
        offset = chunk.next_offset;
        if chunk.eof {
            break;
        }
    }
    if data.len() >= PREVIEW_MAX_BYTES {
        truncated = true;
    }
    decode_preview(data, truncated)
}

pub(crate) fn decode_preview(data: Vec<u8>, mut truncated: bool) -> Result<PreviewData, String> {
    let text = match std::str::from_utf8(&data) {
        Ok(text) => text,
        Err(error) if truncated && error.error_len().is_none() => {
            std::str::from_utf8(&data[..error.valid_up_to()])
                .map_err(|_| "binary or invalid UTF-8".to_owned())?
        }
        Err(_) => return Err("binary or invalid UTF-8".to_owned()),
    };
    if text.contains('\0') {
        return Err("binary file".to_owned());
    }
    let mut lines = Vec::new();
    for line in text.split('\n') {
        if lines.len() >= PREVIEW_MAX_LINES {
            truncated = true;
            break;
        }
        lines.push(line.trim_end_matches('\r').to_owned());
    }
    Ok(PreviewData { lines, truncated })
}

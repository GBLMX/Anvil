//! Editor-side RPC endpoint for plugin hosts.

use anvil_ipc::Listener;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A reply channel for one editor RPC request.
pub type EditorReply = Sender<Result<Value, (i32, String)>>;

/// One editor RPC request forwarded to the UI thread.
pub struct EditorRequest {
    pub plugin_id: String,
    pub method: String,
    pub params: Value,
    pub reply: EditorReply,
}

/// A Unix socket service that forwards editor RPC calls to the UI.
pub(crate) struct EditorService {
    path: PathBuf,
    alive: Arc<AtomicBool>,
}

impl EditorService {
    pub(crate) fn start(
        requests: Sender<EditorRequest>,
        identities: Arc<Mutex<HashMap<[u8; 32], String>>>,
    ) -> io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "anvil-editor-rpc-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_file(&path);
        let listener = Listener::bind(&path)?;
        listener.set_nonblocking(true)?;
        let alive = Arc::new(AtomicBool::new(true));
        let thread_alive = Arc::clone(&alive);
        thread::spawn(move || {
            while thread_alive.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok(connection) => {
                        let requests = requests.clone();
                        let alive = Arc::clone(&thread_alive);
                        let identities = Arc::clone(&identities);
                        thread::spawn(move || serve(connection, &alive, requests, &identities));
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => return,
                }
            }
        });
        Ok(Self { path, alive })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for EditorService {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.path);
    }
}

fn authenticate(
    connection: &mut anvil_ipc::Connection,
    identities: &Arc<Mutex<HashMap<[u8; 32], String>>>,
) -> Option<String> {
    let frame = connection.recv().ok().flatten()?;
    let request: Value = serde_json::from_slice(&frame).ok()?;
    if request.get("method").and_then(Value::as_str) != Some("session.hello") {
        return None;
    }
    let plugin_id = request
        .pointer("/params/plugin_id")
        .and_then(Value::as_str)?
        .to_owned();
    let token = request
        .pointer("/params/token")
        .and_then(Value::as_str)
        .and_then(decode_hex)?;
    let hash: [u8; 32] = Sha256::digest(token).into();
    let identities = identities.lock().ok()?;
    (identities.get(&hash) == Some(&plugin_id)).then_some(plugin_id)
}

fn decode_hex(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut token = [0u8; 32];
    for (index, byte) in token.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(token)
}

fn serve(
    mut connection: anvil_ipc::Connection,
    alive: &AtomicBool,
    requests: Sender<EditorRequest>,
    identities: &Arc<Mutex<HashMap<[u8; 32], String>>>,
) {
    if connection
        .set_read_timeout(Some(Duration::from_secs(2)))
        .is_err()
    {
        return;
    }
    let plugin_id = match authenticate(&mut connection, identities) {
        Some(plugin_id) => plugin_id,
        None => return,
    };
    if connection.set_read_timeout(None).is_err() {
        return;
    }
    while alive.load(Ordering::Relaxed) {
        let frame = match connection.recv() {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(_) => return,
        };
        let Ok(request) = serde_json::from_slice::<Value>(&frame) else {
            return;
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            return;
        };
        let params = request.get("params").cloned().unwrap_or(Value::Null);
        let (reply_tx, reply_rx) = mpsc::channel();
        if requests
            .send(EditorRequest {
                plugin_id: plugin_id.clone(),
                method: method.to_owned(),
                params,
                reply: reply_tx,
            })
            .is_err()
        {
            return;
        }
        let result = match reply_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(result) => result,
            Err(_) => Err((-32005, "editor request timed out".to_owned())),
        };
        let response = match result {
            Ok(result) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }),
            Err((code, message)) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": code, "message": message },
            }),
        };
        if connection
            .send(
                serde_json::to_string(&response)
                    .unwrap_or_default()
                    .as_bytes(),
            )
            .is_err()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn hello(plugin_id: &str, token: [u8; 32]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session.hello",
            "params": {
                "plugin_id": plugin_id,
                "token": token.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
            }
        }))
        .expect("hello")
    }

    fn authenticate_with(plugin_id: &str, token: [u8; 32]) -> Option<String> {
        let dir = std::env::temp_dir().join(format!(
            "anvil-editor-auth-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir(&dir).expect("tempdir");
        let socket = dir.join("editor.sock");
        let listener = anvil_ipc::Listener::bind(&socket).expect("bind");
        let identities = Arc::new(Mutex::new(HashMap::from([(
            <[u8; 32]>::from(Sha256::digest([7u8; 32])),
            "demo".to_owned(),
        )])));
        let server_identities = Arc::clone(&identities);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            let _ = tx.send(authenticate(&mut connection, &server_identities));
        });
        let mut client = anvil_ipc::Connection::connect(socket).expect("connect");
        client.send(&hello(plugin_id, token)).expect("send hello");
        let result = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("auth result");
        let _ = std::fs::remove_dir_all(dir);
        result
    }

    #[test]
    fn accepts_the_authenticated_plugin_identity() {
        assert_eq!(authenticate_with("demo", [7; 32]).as_deref(), Some("demo"));
    }

    #[test]
    fn rejects_a_token_claimed_by_another_plugin() {
        assert_eq!(authenticate_with("other", [7; 32]), None);
    }

    #[test]
    fn rejects_an_unknown_token() {
        assert_eq!(authenticate_with("demo", [8; 32]), None);
    }
}

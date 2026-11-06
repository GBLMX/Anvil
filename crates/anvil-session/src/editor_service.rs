//! Editor-side RPC endpoint for plugin hosts.

use anvil_ipc::Listener;
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A reply channel for one editor RPC request.
pub type EditorReply = Sender<Result<Value, (i32, String)>>;

/// One editor RPC request forwarded to the UI thread.
pub struct EditorRequest {
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
    pub(crate) fn start(requests: Sender<EditorRequest>) -> io::Result<Self> {
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
                        thread::spawn(move || serve(connection, &alive, requests));
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

fn serve(
    mut connection: anvil_ipc::Connection,
    alive: &AtomicBool,
    requests: Sender<EditorRequest>,
) {
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

//! Editor-side RPC endpoint for plugin hosts.

use anvil_ipc::Listener;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A reply channel for one editor RPC request.
pub type EditorReply = Sender<Result<Value, (i32, String)>>;

/// JSON-RPC error code returned when the editor request queue is saturated.
pub(crate) const EDITOR_QUEUE_FULL: i32 = anvil_ipc::codes::LIMIT_EXCEEDED;

/// Number of plugin host connections served at once.
/// Every host holds an editor proxy and an editor bridge connection, so the cap
/// admits the supported fan-out of `MAX_RUNNING_HOSTS` while refusing peers
/// beyond it instead of spawning unbounded threads.
const MAX_CONNECTIONS: usize = 16;

/// Deadline for the `session.hello` frame, including a dribbled frame body.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

/// Deadline for one request frame after authentication.
const FRAME_TIMEOUT: Duration = Duration::from_secs(2);

/// How long an authenticated connection may stay silent before it is dropped.
///
/// Plugin hosts keep their editor connection for the host's lifetime, so this
/// must be far longer than any idle gap between editor calls while still
/// bounding a peer that authenticates and then never sends another frame.
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Poll granularity while waiting for the next request frame.
const IDLE_POLL: Duration = Duration::from_millis(250);

/// How long the UI thread has to answer one forwarded request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);

/// Backoff after a transient accept failure such as `EMFILE`.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

/// Releases one admission slot when its connection thread ends, including when
/// the thread panics, so a failed connection cannot shrink the cap forever.
struct ConnectionSlot(Arc<AtomicUsize>);

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

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
    accept: Option<thread::JoinHandle<()>>,
}

impl EditorService {
    pub(crate) fn start(
        requests: SyncSender<EditorRequest>,
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
        let active = Arc::new(AtomicUsize::new(0));
        let accept = thread::spawn(move || {
            let mut handles: Vec<thread::JoinHandle<()>> = Vec::new();
            while thread_alive.load(Ordering::Relaxed) {
                // Reap finished connection threads so the handle list stays
                // proportional to live peers.
                let mut index = 0;
                while index < handles.len() {
                    if handles[index].is_finished() {
                        let handle = handles.swap_remove(index);
                        let _ = handle.join();
                    } else {
                        index += 1;
                    }
                }
                match listener.accept() {
                    Ok(connection) => {
                        let admitted = active
                            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |connected| {
                                (connected < MAX_CONNECTIONS).then_some(connected + 1)
                            })
                            .is_ok();
                        if !admitted {
                            // Admission cap: close the stream so the backlog
                            // drains without another thread or blocking accept.
                            drop(connection);
                            continue;
                        }
                        let requests = requests.clone();
                        let alive = Arc::clone(&thread_alive);
                        let identities = Arc::clone(&identities);
                        let slot = ConnectionSlot(Arc::clone(&active));
                        handles.push(thread::spawn(move || {
                            let _slot = slot;
                            serve(connection, &alive, &requests, &identities);
                        }));
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => {
                        // Transient failures (EMFILE, ENFILE, ECONNABORTED) must
                        // not take the endpoint down permanently; back off and
                        // keep accepting while the editor is alive.
                        thread::sleep(ACCEPT_BACKOFF);
                    }
                }
            }
        });
        Ok(Self {
            path,
            alive,
            accept: Some(accept),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for EditorService {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.path);
        if let Some(accept) = self.accept.take() {
            // Bounded: the accept loop sleeps at most `ACCEPT_BACKOFF` per pass.
            // Connection threads observe `alive` and stop within the poll or
            // frame deadline instead of running forever.
            let _ = accept.join();
        }
    }
}

fn authenticate(
    connection: &mut anvil_ipc::Connection,
    deadline: Instant,
    identities: &Arc<Mutex<HashMap<[u8; 32], String>>>,
) -> Option<String> {
    let frame = connection.recv_deadline(deadline).ok().flatten()?;
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
    requests: &SyncSender<EditorRequest>,
    identities: &Arc<Mutex<HashMap<[u8; 32], String>>>,
) {
    let plugin_id = match authenticate(
        &mut connection,
        Instant::now() + HANDSHAKE_TIMEOUT,
        identities,
    ) {
        Some(plugin_id) => plugin_id,
        None => return,
    };
    let mut last_frame = Instant::now();
    while alive.load(Ordering::Relaxed) {
        if last_frame.elapsed() >= IDLE_TIMEOUT {
            return;
        }
        match connection.wait_readable(IDLE_POLL) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(_) => return,
        }
        let frame = match connection.recv_deadline(Instant::now() + FRAME_TIMEOUT) {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(_) => return,
        };
        last_frame = Instant::now();
        let Ok(request) = serde_json::from_slice::<Value>(&frame) else {
            return;
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            return;
        };
        let params = request.get("params").cloned().unwrap_or(Value::Null);
        let (reply_tx, reply_rx) = mpsc::channel();
        match requests.try_send(EditorRequest {
            plugin_id: plugin_id.clone(),
            method: method.to_owned(),
            params,
            reply: reply_tx,
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                // The UI thread has not caught up. Answer the peer directly so a
                // slow editor can never grow the queue without bound.
                let response = Err((EDITOR_QUEUE_FULL, "editor request queue full".to_owned()));
                if !send_response(&mut connection, &id, response) {
                    return;
                }
                continue;
            }
            Err(TrySendError::Disconnected(_)) => return,
        }
        let result = match reply_rx.recv_timeout(REPLY_TIMEOUT) {
            Ok(result) => result,
            Err(_) => Err((
                anvil_ipc::codes::TIMEOUT,
                "editor request timed out".to_owned(),
            )),
        };
        if !send_response(&mut connection, &id, result) {
            return;
        }
    }
}

fn send_response(
    connection: &mut anvil_ipc::Connection,
    id: &Value,
    result: Result<Value, (i32, String)>,
) -> bool {
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
    let Ok(body) = serde_json::to_vec(&response) else {
        return false;
    };
    connection.send(&body).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixStream;

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

    fn identities() -> Arc<Mutex<HashMap<[u8; 32], String>>> {
        Arc::new(Mutex::new(HashMap::from([(
            <[u8; 32]>::from(Sha256::digest([7u8; 32])),
            "demo".to_owned(),
        )])))
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
        let mut client = anvil_ipc::Connection::connect(&socket).expect("connect");
        let mut server = listener.accept().expect("accept");
        client.send(&hello(plugin_id, token)).expect("send hello");
        let result = authenticate(
            &mut server,
            Instant::now() + HANDSHAKE_TIMEOUT,
            &identities(),
        );
        let _ = std::fs::remove_dir_all(dir);
        result
    }

    fn connect(path: &Path) -> anvil_ipc::Connection {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match anvil_ipc::Connection::connect(path) {
                Ok(connection) => return connection,
                Err(error) => {
                    assert!(
                        Instant::now() < deadline,
                        "editor socket unavailable: {error}"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    fn read_response(connection: &mut anvil_ipc::Connection) -> Value {
        let frame = connection
            .recv()
            .expect("read response")
            .expect("response frame");
        serde_json::from_slice(&frame).expect("response json")
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

    #[test]
    fn half_frame_peer_times_out_without_blocking_other_requests() {
        let (requests, receiver) = mpsc::sync_channel(4);
        let service = EditorService::start(requests, identities()).expect("editor service");
        let path = service.path().to_path_buf();

        // Peer A authenticates and then stalls halfway through a frame body.
        let mut stalled = connect(&path);
        stalled.send(&hello("demo", [7; 32])).expect("hello");
        let mut raw: UnixStream = stalled.try_clone_stream().expect("clone");
        raw.write_all(&[0, 0, 0, 32]).expect("frame header");
        raw.write_all(b"{").expect("partial body");

        // Peer B must complete a full round trip while A is still stalled.
        let mut healthy = connect(&path);
        healthy
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("timeout");
        healthy.send(&hello("demo", [7; 32])).expect("hello");
        healthy
            .send(br#"{"jsonrpc":"2.0","id":1,"method":"editor.get_view","params":{}}"#)
            .expect("request");

        // The UI thread analogue: answer the forwarded request.
        let request = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("request forwarded while another peer is stalled");
        assert_eq!(request.plugin_id, "demo");
        assert_eq!(request.method, "editor.get_view");
        let _ = request.reply.send(Ok(serde_json::json!({ "view": "ok" })));

        let response = read_response(&mut healthy);
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"]["view"], "ok");

        // The stalled frame is cut off at the frame deadline instead of pinning
        // the connection thread until the peer decides to finish it.
        stalled
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        assert!(
            stalled.recv().expect("read after deadline").is_none(),
            "half frame must be closed at the deadline"
        );

        drop(healthy);
        drop(stalled);
        drop(service);
    }

    #[test]
    fn saturated_request_queue_answers_with_an_error() {
        let (requests, _receiver) = mpsc::sync_channel::<EditorRequest>(1);
        let (reply, _reply_rx) = mpsc::channel();
        requests
            .try_send(EditorRequest {
                plugin_id: "demo".to_owned(),
                method: "editor.get_view".to_owned(),
                params: Value::Null,
                reply,
            })
            .expect("fill queue");
        let service = EditorService::start(requests, identities()).expect("editor service");
        let mut client = connect(service.path());
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("timeout");
        client.send(&hello("demo", [7; 32])).expect("hello");
        client
            .send(br#"{"jsonrpc":"2.0","id":9,"method":"editor.get_view","params":{}}"#)
            .expect("request");
        let response = read_response(&mut client);
        assert_eq!(response["id"], 9);
        assert_eq!(response["error"]["code"], EDITOR_QUEUE_FULL);
        assert!(
            response["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("queue full")),
            "{response}"
        );
        drop(client);
        drop(service);
    }

    #[test]
    fn connections_beyond_the_admission_cap_are_closed() {
        let (requests, _receiver) = mpsc::sync_channel(4);
        let service = EditorService::start(requests, identities()).expect("editor service");
        let path = service.path().to_path_buf();

        let mut admitted = Vec::new();
        for _ in 0..MAX_CONNECTIONS {
            admitted.push(connect(&path));
        }
        // Let the accept loop take all admitted connections before the extra one.
        thread::sleep(Duration::from_millis(200));

        let mut extra = connect(&path);
        extra
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("timeout");
        assert!(
            extra.recv().expect("read beyond cap").is_none(),
            "peer beyond the cap must see a closed connection"
        );

        drop(extra);
        drop(admitted);
        drop(service);
    }
}

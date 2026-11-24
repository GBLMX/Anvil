use crate::plugin_runtime::{HostExit, HostProcess as HostProcessPort, HostSpawner, HostStart};
use anvil_ipc::{Connection, FrameError, Listener};
use std::io;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_QUEUED_EVENTS: usize = 64;
const MAX_PENDING_COMMANDS: usize = 128;
const MAX_QUEUED_HOST_EVENTS: usize = 128;

/// Absolute deadline for one host RPC round trip.
///
/// Provider calls can legitimately take seconds on slow file systems, so the
/// former two-second socket timeout killed healthy sessions. The deadline still
/// bounds a wedged host and, unlike a socket read timeout, it covers a peer that
/// dribbles a frame.
const HOST_RPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Bound on one control-socket write so a host that stops reading cannot block
/// the worker thread (and therefore `stop`) forever.
const HOST_WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// First id used for control requests (`sys.ping`, `sys.exec`, event stats).
///
/// Provider calls use ids minted by the supervisor. Starting control ids far
/// above that range keeps a late provider response from ever matching the next
/// control request, even after a provider call timed out.
const CONTROL_ID_BASE: u64 = 1 << 62;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SendOutcome {
    Sent,
    Full,
    Closed,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    Ready,
    Pong,
    ExecUnsupported,
    Error(String),
    Provider {
        id: u64,
        result: Result<serde_json::Value, String>,
    },
    ProviderAction {
        id: u64,
        result: Result<serde_json::Value, String>,
    },
    EventStats(u64),
    Exited(HostExit),
}

enum HostCommand {
    Exec,
    Provider {
        id: u64,
        handler: String,
        query: String,
        limit: u32,
    },
    ProviderAction {
        id: u64,
        handler: String,
        value: String,
    },
    Event {
        subscription_id: u64,
        event: String,
        sequence: u64,
        payload: serde_json::Value,
    },
    EventStats,
}

pub struct HostSession {
    host: Box<dyn HostProcessPort>,
    commands: SyncSender<HostCommand>,
    events: Receiver<HostEvent>,
    alive: Arc<AtomicBool>,
    pending: Arc<AtomicUsize>,
    queued_events: Arc<AtomicUsize>,
    accepting: Arc<AtomicBool>,
    socket: PathBuf,
    worker: Option<std::thread::JoinHandle<()>>,
    exited: bool,
}

struct SocketCleanup(Option<PathBuf>);

impl SocketCleanup {
    fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }

    fn keep(mut self) -> PathBuf {
        self.0.take().expect("socket cleanup guard armed")
    }
}

impl Drop for SocketCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl HostSession {
    pub(crate) fn start(spawner: &dyn HostSpawner, start: HostStart<'_>) -> Result<Self, String> {
        let socket = std::env::temp_dir().join(format!(
            "anvil-host-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_file(&socket);
        let cleanup = SocketCleanup::new(socket.clone());
        let listener = Listener::bind(&socket).map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let process = spawner.spawn(&start, &socket)?;
        let (command_tx, command_rx) = mpsc::sync_channel(MAX_PENDING_COMMANDS);
        let (event_tx, event_rx) = mpsc::sync_channel(MAX_QUEUED_HOST_EVENTS);
        let alive = Arc::new(AtomicBool::new(true));
        let pending = Arc::new(AtomicUsize::new(0));
        let queued_events = Arc::new(AtomicUsize::new(0));
        let accepting = Arc::new(AtomicBool::new(true));
        let thread_alive = Arc::clone(&alive);
        let thread_queued_events = Arc::clone(&queued_events);
        let thread_pending = Arc::clone(&pending);
        let worker = std::thread::spawn(move || {
            let mut connection = loop {
                if !thread_alive.load(Ordering::Relaxed) {
                    return;
                }
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => {
                        let _ =
                            event_tx.try_send(HostEvent::Error(format!("host accept: {error}")));
                        return;
                    }
                }
            };
            let _ = event_tx.try_send(HostEvent::Ready);
            let _ = connection.set_read_timeout(Some(HOST_RPC_TIMEOUT));
            let _ = connection.set_write_timeout(Some(HOST_WRITE_TIMEOUT));
            let mut next_id = CONTROL_ID_BASE;
            if !request(
                &mut connection,
                next_id,
                "sys.ping",
                &event_tx,
                &thread_alive,
            ) {
                return;
            }
            next_id += 1;
            while thread_alive.load(Ordering::Relaxed) {
                match command_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(HostCommand::Exec) => {
                        let result = request(
                            &mut connection,
                            next_id,
                            "sys.exec",
                            &event_tx,
                            &thread_alive,
                        );
                        thread_pending.fetch_sub(1, Ordering::AcqRel);
                        if !result {
                            return;
                        }
                        next_id += 1;
                    }
                    Ok(HostCommand::ProviderAction { id, handler, value }) => {
                        let result = call(
                            &mut connection,
                            id,
                            "provider.action",
                            serde_json::json!({
                                "request_id": id,
                                "handler": handler,
                                "value": value,
                            }),
                            &thread_alive,
                        );
                        thread_pending.fetch_sub(1, Ordering::AcqRel);
                        if !send_host_result(&event_tx, HostEvent::ProviderAction { id, result }) {
                            return;
                        }
                    }
                    Ok(HostCommand::Provider {
                        id,
                        handler,
                        query,
                        limit,
                    }) => {
                        let result = call(
                            &mut connection,
                            id,
                            "provider.request",
                            serde_json::json!({
                                "request_id": id,
                                "handler": handler,
                                "query": query,
                                "limit": limit,
                            }),
                            &thread_alive,
                        );
                        thread_pending.fetch_sub(1, Ordering::AcqRel);
                        if !send_host_result(&event_tx, HostEvent::Provider { id, result }) {
                            return;
                        }
                    }
                    Ok(HostCommand::Event {
                        subscription_id,
                        event,
                        sequence,
                        payload,
                    }) => {
                        thread_queued_events.fetch_sub(1, Ordering::AcqRel);
                        let body = serde_json::to_vec(&event_frame(
                            subscription_id,
                            &event,
                            sequence,
                            payload,
                        ))
                        .map_err(|error| io::Error::other(format!("encode host.event: {error}")));
                        let body = match body {
                            Ok(body) => body,
                            Err(_) => return,
                        };
                        if connection.send(&body).is_err() {
                            return;
                        }
                    }
                    Ok(HostCommand::EventStats) => {
                        let result = call(
                            &mut connection,
                            next_id,
                            "host.event_stats",
                            serde_json::json!({}),
                            &thread_alive,
                        );
                        next_id += 1;
                        let errors = result
                            .ok()
                            .and_then(|value| {
                                value
                                    .get("handler_errors")
                                    .and_then(serde_json::Value::as_u64)
                            })
                            .unwrap_or(0);
                        if !send_host_result(&event_tx, HostEvent::EventStats(errors)) {
                            return;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        Ok(Self {
            host: process,
            commands: command_tx,
            events: event_rx,
            alive,
            pending,
            queued_events,
            accepting,
            socket: cleanup.keep(),
            worker: Some(worker),
            exited: false,
        })
    }

    #[allow(dead_code)]
    pub fn id(&self) -> u32 {
        self.host.id()
    }

    pub fn exec(&self) -> SendOutcome {
        if !self.accepting.load(Ordering::Acquire) {
            return SendOutcome::Closed;
        }
        self.pending.fetch_add(1, Ordering::AcqRel);
        let outcome = match self.commands.try_send(HostCommand::Exec) {
            Ok(()) => SendOutcome::Sent,
            Err(TrySendError::Full(_)) => SendOutcome::Full,
            Err(TrySendError::Disconnected(_)) => SendOutcome::Closed,
        };
        if outcome != SendOutcome::Sent {
            self.pending.fetch_sub(1, Ordering::AcqRel);
        }
        outcome
    }

    pub fn event(
        &self,
        subscription_id: u64,
        event: &str,
        sequence: u64,
        payload: serde_json::Value,
    ) -> bool {
        if !self.accepting.load(Ordering::Acquire) {
            return false;
        }
        let reserved = self
            .queued_events
            .try_update(Ordering::AcqRel, Ordering::Acquire, |queued| {
                (queued < MAX_QUEUED_EVENTS).then_some(queued + 1)
            })
            .is_ok();
        if !reserved {
            return false;
        }
        if self
            .commands
            .try_send(HostCommand::Event {
                subscription_id,
                event: event.to_owned(),
                sequence,
                payload,
            })
            .is_err()
        {
            self.queued_events.fetch_sub(1, Ordering::AcqRel);
            return false;
        }
        true
    }

    pub fn event_stats(&self) -> bool {
        self.accepting.load(Ordering::Acquire)
            && self.commands.try_send(HostCommand::EventStats).is_ok()
    }

    pub fn provider(&self, id: u64, handler: &str, query: &str, limit: u32) -> SendOutcome {
        if !self.accepting.load(Ordering::Acquire) {
            return SendOutcome::Closed;
        }
        self.pending.fetch_add(1, Ordering::AcqRel);
        let outcome = match self.commands.try_send(HostCommand::Provider {
            id,
            handler: handler.to_owned(),
            query: query.to_owned(),
            limit,
        }) {
            Ok(()) => SendOutcome::Sent,
            Err(TrySendError::Full(_)) => SendOutcome::Full,
            Err(TrySendError::Disconnected(_)) => SendOutcome::Closed,
        };
        if outcome != SendOutcome::Sent {
            self.pending.fetch_sub(1, Ordering::AcqRel);
        }
        outcome
    }

    pub fn provider_action(&self, id: u64, handler: &str, value: &str) -> SendOutcome {
        if !self.accepting.load(Ordering::Acquire) {
            return SendOutcome::Closed;
        }
        self.pending.fetch_add(1, Ordering::AcqRel);
        let outcome = match self.commands.try_send(HostCommand::ProviderAction {
            id,
            handler: handler.to_owned(),
            value: value.to_owned(),
        }) {
            Ok(()) => SendOutcome::Sent,
            Err(TrySendError::Full(_)) => SendOutcome::Full,
            Err(TrySendError::Disconnected(_)) => SendOutcome::Closed,
        };
        if outcome != SendOutcome::Sent {
            self.pending.fetch_sub(1, Ordering::AcqRel);
        }
        outcome
    }

    pub fn has_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire) > 0
    }

    pub fn drain(&self, timeout: Duration) {
        self.accepting.store(false, Ordering::Release);
        let deadline = std::time::Instant::now() + timeout;
        while self.pending.load(Ordering::Acquire) > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn poll(&mut self) -> Vec<HostEvent> {
        let mut events = self.events.try_iter().collect::<Vec<_>>();
        if !self.exited
            && let Ok(Some(status)) = self.host.try_wait()
        {
            self.exited = true;
            events.push(HostEvent::Exited(status));
        }
        events
    }

    pub fn stop(&mut self) {
        self.accepting.store(false, Ordering::Release);
        self.alive.store(false, Ordering::Relaxed);
        self.host.terminate();
        if let Some(worker) = self.worker.take() {
            // Bounded: the worker re-checks `alive` every 100ms and its socket
            // reads and writes carry deadlines, so terminating the host makes
            // the join return instead of detaching a thread that read forever.
            let _ = worker.join();
        }
        let _ = std::fs::remove_file(&self.socket);
    }

    #[cfg(test)]
    pub(crate) fn start_with_host(
        _workspace: &Path,
        host: &Path,
        allocate_mb: Option<u32>,
        cgroup_root: Option<PathBuf>,
        editor_socket: Option<&Path>,
    ) -> Result<Self, String> {
        let spawner = TestSpawner {
            host: host.to_path_buf(),
            allocate_mb,
            cgroup_root,
        };
        Self::start(
            &spawner,
            HostStart {
                plugin_id: "test",
                plugin_root: None,
                token: crate::plugin_runtime::HostToken::new(),
                gateway_socket: None,
                editor_socket,
                command: None,
                allocate_mb,
            },
        )
    }
}

impl Drop for HostSession {
    fn drop(&mut self) {
        self.stop();
    }
}

fn send_host_result(events: &SyncSender<HostEvent>, event: HostEvent) -> bool {
    match events.try_send(event) {
        Ok(()) | Err(TrySendError::Full(_)) => true,
        Err(TrySendError::Disconnected(_)) => false,
    }
}

fn request(
    connection: &mut Connection,
    id: u64,
    method: &str,
    events: &SyncSender<HostEvent>,
    alive: &AtomicBool,
) -> bool {
    request_with_timeout(connection, id, method, events, alive, HOST_RPC_TIMEOUT)
}

fn request_with_timeout(
    connection: &mut Connection,
    id: u64,
    method: &str,
    events: &SyncSender<HostEvent>,
    alive: &AtomicBool,
    timeout: Duration,
) -> bool {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method
    });
    let body = match serde_json::to_vec(&request) {
        Ok(body) => body,
        Err(error) => {
            let _ = events.try_send(HostEvent::Error(error.to_string()));
            return false;
        }
    };
    if let Err(error) = connection.send(&body) {
        let _ = events.try_send(HostEvent::Error(error.to_string()));
        return false;
    }
    let deadline = Instant::now() + timeout;
    loop {
        match connection.recv_deadline(deadline) {
            Ok(Some(frame)) => {
                let response = match serde_json::from_slice::<serde_json::Value>(&frame) {
                    Ok(response) => response,
                    Err(error) => {
                        let _ = events.try_send(HostEvent::Error(error.to_string()));
                        return false;
                    }
                };
                if !response_matches_id(&response, id) {
                    // A late or stale response belongs to an earlier request and
                    // must not be mistaken for this one.
                    continue;
                }
                if response.get("result").is_some() {
                    let _ = events.try_send(HostEvent::Pong);
                    return true;
                }
                if response.get("error").is_some() {
                    let _ = events.try_send(HostEvent::ExecUnsupported);
                    return true;
                }
                return true;
            }
            Ok(None) => return false,
            Err(error) if rpc_timeout(&error) => {
                let _ = events.try_send(HostEvent::Error("host RPC timeout".to_owned()));
                alive.store(false, Ordering::Relaxed);
                return false;
            }
            Err(error) => {
                let _ = events.try_send(HostEvent::Error(error.to_string()));
                return false;
            }
        }
    }
}

fn call(
    connection: &mut Connection,
    id: u64,
    method: &str,
    params: serde_json::Value,
    alive: &AtomicBool,
) -> Result<serde_json::Value, String> {
    call_with_timeout(connection, id, method, params, alive, HOST_RPC_TIMEOUT)
}

fn call_with_timeout(
    connection: &mut Connection,
    id: u64,
    method: &str,
    params: serde_json::Value,
    alive: &AtomicBool,
    timeout: Duration,
) -> Result<serde_json::Value, String> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    connection
        .send(&serde_json::to_vec(&request).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + timeout;
    loop {
        let frame = match connection.recv_deadline(deadline) {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                alive.store(false, Ordering::Relaxed);
                return Err("host socket closed".to_owned());
            }
            // A slow provider call is reported without poisoning the session;
            // the supervisor decides whether to retry or restart the host.
            Err(error) if rpc_timeout(&error) => return Err("host RPC timeout".to_owned()),
            Err(error) => {
                alive.store(false, Ordering::Relaxed);
                return Err(error.to_string());
            }
        };
        let response: serde_json::Value = match serde_json::from_slice(&frame) {
            Ok(response) => response,
            Err(error) => {
                alive.store(false, Ordering::Relaxed);
                return Err(error.to_string());
            }
        };
        if !response_matches_id(&response, id) {
            // Late responses for earlier calls stay out of this result.
            continue;
        }
        if let Some(error) = response.get("error") {
            return Err(error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("host provider request failed")
                .to_owned());
        }
        return Ok(response
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null));
    }
}

fn rpc_timeout(error: &FrameError) -> bool {
    matches!(
        error,
        FrameError::Timeout | FrameError::Io(io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock)
    )
}

fn event_frame(
    subscription_id: u64,
    event: &str,
    sequence: u64,
    payload: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "host.event",
        "params": {
            "subscription_id": subscription_id,
            "event": event,
            "sequence": sequence,
            "payload": payload,
        }
    })
}

#[cfg(test)]
fn cgroup_root() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os("ANVIL_CGROUP_ROOT") {
        return Ok(PathBuf::from(root));
    }
    anvil_plugin_host::Cgroup::detect_root().map_err(|error| format!("cgroup: {error}"))
}

fn response_matches_id(response: &serde_json::Value, id: u64) -> bool {
    response.get("id").and_then(serde_json::Value::as_u64) == Some(id)
}
#[cfg(test)]
struct TestSpawner {
    host: PathBuf,
    allocate_mb: Option<u32>,
    cgroup_root: Option<PathBuf>,
}

#[cfg(test)]
impl HostSpawner for TestSpawner {
    fn spawn(
        &self,
        start: &HostStart<'_>,
        socket: &Path,
    ) -> Result<Box<dyn HostProcessPort>, String> {
        let cgroup = match &self.cgroup_root {
            Some(root) => Some(
                anvil_plugin_host::Cgroup::create(
                    root,
                    &format!(
                        "anvil-test-{}-{}",
                        std::process::id(),
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|time| time.as_nanos())
                            .unwrap_or_default()
                    ),
                    64,
                    256 << 20,
                    100,
                )
                .map_err(|error| format!("cgroup: {error}"))?,
            ),
            None => None,
        };
        let mut args = vec![
            "--socket".to_owned(),
            socket.display().to_string(),
            "--plugin-id".to_owned(),
            start.plugin_id.to_owned(),
        ];
        if let Some(mb) = self.allocate_mb {
            args.push("--allocate-mb".to_owned());
            args.push(mb.to_string());
        }
        if let Some(editor_socket) = start.editor_socket {
            args.push("--editor-socket".to_owned());
            args.push(editor_socket.display().to_string());
        }
        let token = anvil_plugin_host::Token(start.token.0);
        let process = match cgroup {
            Some(cgroup) => anvil_plugin_host::spawn_in_cgroup(&self.host, &args, token, cgroup),
            None => anvil_plugin_host::spawn(&self.host, &args, token),
        }
        .map_err(|error| format!("spawn host: {error}"))?;
        Ok(Box::new(TestProcess(process)))
    }
}

#[cfg(test)]
struct TestProcess(anvil_plugin_host::HostProcess);

#[cfg(test)]
impl HostProcessPort for TestProcess {
    fn id(&self) -> u32 {
        self.0.id()
    }

    fn try_wait(&mut self) -> io::Result<Option<HostExit>> {
        let memory_limited = self.0.memory_limited();
        Ok(self.0.try_wait()?.map(|status| {
            match anvil_plugin_host::classify_exit(status, memory_limited) {
                anvil_plugin_host::HostExit::Exited(code) => HostExit::Exited(code),
                anvil_plugin_host::HostExit::Signaled(signal) => HostExit::Signaled(signal),
                anvil_plugin_host::HostExit::OomKilled => HostExit::OomKilled,
            }
        }))
    }

    fn terminate(&mut self) {
        self.0.terminate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::libc;

    fn control_pair() -> (Connection, Connection, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "anvil-host-rpc-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or_default()
        ));
        let listener = Listener::bind(&path).expect("bind");
        let client = Connection::connect(&path).expect("connect");
        let server = listener.accept().expect("accept");
        (client, server, path)
    }

    fn frame(value: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&value).expect("frame")
    }

    #[test]
    fn request_skips_a_stale_response_id() {
        let (mut client, mut server, path) = control_pair();
        let (events, received) = mpsc::sync_channel(4);
        let alive = AtomicBool::new(true);
        // A late provider response (low id) arrives before the control reply.
        server
            .send(&frame(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 6,
                "result": { "stale": true },
            })))
            .expect("stale response");
        // The control channel reply for this request.
        server
            .send(&frame(serde_json::json!({
                "jsonrpc": "2.0",
                "id": CONTROL_ID_BASE,
                "result": "pong",
            })))
            .expect("matching response");
        assert!(request(
            &mut client,
            CONTROL_ID_BASE,
            "sys.ping",
            &events,
            &alive
        ));
        assert!(alive.load(Ordering::Relaxed));
        assert_eq!(received.try_recv(), Ok(HostEvent::Pong));
        assert!(
            received.try_recv().is_err(),
            "a stale response must not answer the request"
        );
        drop(server);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn request_deadline_stops_the_control_channel() {
        let (mut client, server, path) = control_pair();
        let (events, received) = mpsc::sync_channel(4);
        let alive = AtomicBool::new(true);
        let started = Instant::now();
        assert!(!request_with_timeout(
            &mut client,
            1,
            "sys.ping",
            &events,
            &alive,
            Duration::from_millis(50),
        ));
        assert!(started.elapsed() >= Duration::from_millis(50));
        assert!(!alive.load(Ordering::Relaxed));
        assert!(matches!(
            received.try_recv(),
            Ok(HostEvent::Error(message)) if message.contains("timeout")
        ));
        drop(server);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn slow_call_timeout_does_not_kill_the_session() {
        let (mut client, mut server, path) = control_pair();
        let alive = AtomicBool::new(true);
        let error = call_with_timeout(
            &mut client,
            1,
            "provider.request",
            serde_json::json!({}),
            &alive,
            Duration::from_millis(50),
        )
        .expect_err("slow call times out");
        assert!(error.contains("timeout"), "{error}");
        assert!(
            alive.load(Ordering::Relaxed),
            "a slow provider call must not kill the session"
        );

        // The timed-out response and the next response both arrive before the
        // next request is read; the stale id must be skipped, not returned.
        server
            .send(&frame(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "late": true },
            })))
            .expect("late response");
        server
            .send(&frame(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": { "ok": true },
            })))
            .expect("response");
        let result = call_with_timeout(
            &mut client,
            2,
            "provider.request",
            serde_json::json!({}),
            &alive,
            Duration::from_secs(2),
        )
        .expect("healthy call after a slow one");
        assert_eq!(result["ok"], true);
        assert!(alive.load(Ordering::Relaxed));
        drop(server);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn host_responses_must_match_the_request_id() {
        assert!(response_matches_id(
            &serde_json::json!({"jsonrpc": "2.0", "id": 7, "result": {}}),
            7
        ));
        assert!(!response_matches_id(
            &serde_json::json!({"jsonrpc": "2.0", "id": 8, "result": {}}),
            7
        ));
        assert!(!response_matches_id(
            &serde_json::json!({"jsonrpc": "2.0", "result": {}}),
            7
        ));
    }

    #[test]
    fn host_event_frame_is_a_notification() {
        let frame = event_frame(
            7,
            "diagnostics.changed",
            3,
            serde_json::json!({"generation": 9}),
        );
        assert_eq!(frame["method"], "host.event");
        assert!(frame.get("id").is_none());
        assert_eq!(frame["params"]["subscription_id"], 7);
        assert_eq!(frame["params"]["event"], "diagnostics.changed");
        assert_eq!(frame["params"]["sequence"], 3);
        assert_eq!(frame["params"]["payload"]["generation"], 9);
    }

    #[test]
    fn host_event_is_written_to_the_control_socket() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "anvil-event-frame-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let script = dir.join("host.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 5\n").expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let session = HostSession::start_with_host(Path::new("."), &script, None, None, None)
            .expect("start host");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut connection = loop {
            match Connection::connect(&session.socket) {
                Ok(connection) => break connection,
                Err(_) => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "host socket was not ready"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        };
        connection
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("timeout");
        let ping = connection.recv().expect("read ping").expect("ping frame");
        let ping: serde_json::Value = serde_json::from_slice(&ping).expect("ping json");
        assert_eq!(ping["method"], "sys.ping");
        let response = serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": ping["id"].clone(),
            "result": "pong",
        }))
        .expect("response");
        connection.send(&response).expect("send pong");
        assert!(session.event(
            7,
            "diagnostics.changed",
            3,
            serde_json::json!({ "generation": 9 }),
        ));
        let frame = connection.recv().expect("read event").expect("event frame");
        let frame: serde_json::Value = serde_json::from_slice(&frame).expect("event json");
        assert_eq!(frame["method"], "host.event");
        assert!(frame.get("id").is_none());
        assert_eq!(frame["params"]["subscription_id"], 7);
        assert_eq!(frame["params"]["payload"]["generation"], 9);
        drop(session);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn full_host_event_queue_drops_without_closing() {
        let (events, receiver) = mpsc::sync_channel(1);
        events.try_send(HostEvent::Pong).expect("prime queue");
        assert!(send_host_result(&events, HostEvent::Ready));
        drop(receiver);
        assert!(!send_host_result(&events, HostEvent::Ready));
    }

    #[test]
    fn socket_cleanup_removes_unkept_paths() {
        let path = std::env::temp_dir().join(format!(
            "anvil-socket-guard-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::write(&path, b"x").expect("socket placeholder");
        {
            let _cleanup = SocketCleanup::new(path.clone());
        }
        assert!(!path.exists());

        std::fs::write(&path, b"x").expect("socket placeholder");
        let kept = SocketCleanup::new(path.clone()).keep();
        assert!(kept.exists());
        let _ = std::fs::remove_file(kept);
    }

    #[test]
    fn host_sigkill_is_reported_without_blocking_the_loop() {
        let Some(host) = std::env::current_exe()
            .ok()
            .and_then(|path| {
                path.parent()?
                    .parent()
                    .map(|parent| parent.join("anvil-plugin-host"))
            })
            .filter(|path| path.is_file())
        else {
            eprintln!("skipping SIGKILL test: plugin host binary not built");
            return;
        };
        let Ok(mut session) = HostSession::start_with_host(Path::new("."), &host, None, None, None)
        else {
            eprintln!("skipping SIGKILL test: host did not start");
            return;
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            for event in session.poll() {
                if event == HostEvent::Pong {
                    let pid = session.id() as i32;
                    unsafe {
                        libc::kill(pid, libc::SIGKILL);
                    }
                    let deadline = std::time::Instant::now() + Duration::from_secs(2);
                    loop {
                        for event in session.poll() {
                            if event == HostEvent::Exited(HostExit::Signaled(9)) {
                                session.stop();
                                return;
                            }
                        }
                        assert!(
                            std::time::Instant::now() < deadline,
                            "host exit was not observed"
                        );
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "host did not become ready"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn oom_kill_is_reported_without_blocking_the_loop() {
        let Some(host) = std::env::current_exe()
            .ok()
            .and_then(|path| {
                path.parent()?
                    .parent()
                    .map(|parent| parent.join("anvil-plugin-host"))
            })
            .filter(|path| path.is_file())
        else {
            eprintln!("skipping OOM test: plugin host binary not built");
            return;
        };
        let Ok(root) = cgroup_root() else {
            eprintln!("skipping OOM test: no delegated cgroup2 root");
            return;
        };
        let Ok(mut session) =
            HostSession::start_with_host(Path::new("."), &host, Some(300), Some(root), None)
        else {
            eprintln!("skipping OOM test: no delegated cgroup2 root");
            return;
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            for event in session.poll() {
                if event == HostEvent::Exited(HostExit::OomKilled) {
                    session.stop();
                    return;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "host did not report OOM"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn host_receives_editor_socket_argument() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "anvil-host-args-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let script = dir.join("host.sh");
        let editor_socket = dir.join("editor.sock");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\nsleep 1\n",
        )
        .expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let session =
            HostSession::start_with_host(Path::new("."), &script, None, None, Some(&editor_socket))
                .expect("start host");
        let args_path = PathBuf::from(format!("{}.args", script.display()));
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let args = loop {
            if let Ok(args) = std::fs::read_to_string(&args_path)
                && args.contains("--editor-socket")
                && args.contains(editor_socket.to_string_lossy().as_ref())
            {
                break args;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "host did not write args"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(args.contains("--editor-socket"), "{args}");
        assert!(
            args.contains(editor_socket.to_string_lossy().as_ref()),
            "{args}"
        );
        drop(session);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn event_queue_rejects_overflow_without_blocking() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "anvil-event-queue-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let script = dir.join("host.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 5\n").expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let session = HostSession::start_with_host(Path::new("."), &script, None, None, None)
            .expect("start host");
        let started = std::time::Instant::now();
        for index in 0..MAX_QUEUED_EVENTS {
            assert!(
                session.event(
                    index as u64,
                    "diagnostics.changed",
                    index as u64,
                    serde_json::json!({ "generation": index }),
                ),
                "event {index} should fit"
            );
        }
        assert!(
            !session.event(
                MAX_QUEUED_EVENTS as u64,
                "diagnostics.changed",
                MAX_QUEUED_EVENTS as u64,
                serde_json::json!({}),
            ),
            "overflowing event should be rejected"
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(session);
        let _ = std::fs::remove_dir_all(dir);
    }
}

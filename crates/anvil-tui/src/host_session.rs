use anvil_ipc::{Connection, FrameError, Listener};
use anvil_plugin_host::{Cgroup, HostExit, HostProcess, Token, spawn};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    Ready,
    Pong,
    ExecUnsupported,
    Error(String),
    Exited(HostExit),
}

enum HostCommand {
    Exec,
}

pub struct HostSession {
    host: HostProcess,
    cgroup: Option<Cgroup>,
    commands: Sender<HostCommand>,
    events: Receiver<HostEvent>,
    alive: Arc<AtomicBool>,
    socket: PathBuf,
    exited: bool,
}

impl HostSession {
    pub fn start(_workspace: &Path) -> Result<Self, String> {
        let host = host_binary()?;
        let socket = std::env::temp_dir().join(format!(
            "anvil-host-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_file(&socket);
        let listener = Listener::bind(&socket).map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let cgroup = match std::env::var_os("ANVIL_CGROUP_ROOT") {
            Some(root) => {
                let name = format!("anvil-plugin-{}", std::process::id());
                Some(
                    Cgroup::create(root, &name, 64, 256 << 20)
                        .map_err(|error| format!("cgroup: {error}"))?,
                )
            }
            None => None,
        };
        let token = token();
        let process = spawn(
            &host,
            &["--socket".to_owned(), socket.display().to_string()],
            token,
        )
        .map_err(|error| format!("spawn host: {error}"))?;
        if let Some(cgroup) = &cgroup {
            process
                .attach_cgroup(cgroup)
                .map_err(|error| format!("attach cgroup: {error}"))?;
        }
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let alive = Arc::new(AtomicBool::new(true));
        let thread_alive = Arc::clone(&alive);
        std::thread::spawn(move || {
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
                        let _ = event_tx.send(HostEvent::Error(format!("host accept: {error}")));
                        return;
                    }
                }
            };
            let _ = event_tx.send(HostEvent::Ready);
            let _ = connection.set_read_timeout(Some(Duration::from_secs(2)));
            let mut next_id = 1u64;
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
                        if !request(
                            &mut connection,
                            next_id,
                            "sys.exec",
                            &event_tx,
                            &thread_alive,
                        ) {
                            return;
                        }
                        next_id += 1;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        Ok(Self {
            host: process,
            cgroup,
            commands: command_tx,
            events: event_rx,
            alive,
            socket,
            exited: false,
        })
    }

    pub fn exec(&self) -> bool {
        self.commands.send(HostCommand::Exec).is_ok()
    }

    pub fn poll(&mut self) -> Vec<HostEvent> {
        let mut events = self.events.try_iter().collect::<Vec<_>>();
        if !self.exited
            && let Ok(Some(status)) = self.host.try_wait()
        {
            self.exited = true;
            events.push(HostEvent::Exited(anvil_plugin_host::classify_exit(
                status,
                self.cgroup.is_some(),
            )));
        }
        events
    }

    pub fn stop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        self.host.terminate();
        let _ = std::fs::remove_file(&self.socket);
    }
}

impl Drop for HostSession {
    fn drop(&mut self) {
        self.stop();
    }
}

fn request(
    connection: &mut Connection,
    id: u64,
    method: &str,
    events: &Sender<HostEvent>,
    alive: &AtomicBool,
) -> bool {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method
    });
    let body = match serde_json::to_vec(&request) {
        Ok(body) => body,
        Err(error) => {
            let _ = events.send(HostEvent::Error(error.to_string()));
            return false;
        }
    };
    if let Err(error) = connection.send(&body) {
        let _ = events.send(HostEvent::Error(error.to_string()));
        return false;
    }
    match connection.recv() {
        Ok(Some(frame)) => match serde_json::from_slice::<serde_json::Value>(&frame) {
            Ok(response) if response.get("result").is_some() => {
                let _ = events.send(HostEvent::Pong);
                true
            }
            Ok(response) if response.get("error").is_some() => {
                let _ = events.send(HostEvent::ExecUnsupported);
                true
            }
            Ok(_) => true,
            Err(error) => {
                let _ = events.send(HostEvent::Error(error.to_string()));
                false
            }
        },
        Ok(None) => false,
        Err(FrameError::Io(io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock)) => {
            let _ = events.send(HostEvent::Error("host RPC timeout".to_owned()));
            alive.store(false, Ordering::Relaxed);
            false
        }
        Err(error) => {
            let _ = events.send(HostEvent::Error(error.to_string()));
            false
        }
    }
}

fn host_binary() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("ANVIL_PLUGIN_HOST") {
        return Ok(PathBuf::from(path));
    }
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    let path = current
        .parent()
        .ok_or_else(|| "no executable parent".to_owned())?
        .join("anvil-plugin-host");
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("plugin host not found at {}", path.display()))
    }
}

fn token() -> Token {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or_default();
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&(std::process::id() as u64).to_le_bytes());
    bytes[8..24].copy_from_slice(&nanos.to_le_bytes());
    bytes[24..].copy_from_slice(&(nanos.rotate_left(17) as u64).to_le_bytes());
    Token(bytes)
}

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
    pub fn start(workspace: &Path) -> Result<Self, String> {
        let host = host_binary()?;
        let cgroup_root = std::env::var_os("ANVIL_CGROUP_ROOT").map(PathBuf::from);
        Self::start_with_host(workspace, &host, None, cgroup_root)
    }

    pub fn start_oom(workspace: &Path) -> Result<Self, String> {
        let host = host_binary()?;
        let root = std::env::var_os("ANVIL_CGROUP_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/sys/fs/cgroup"));
        Self::start_with_host(workspace, &host, Some(300), Some(root))
    }

    pub(crate) fn start_with_host(
        _workspace: &Path,
        host: &Path,
        allocate_mb: Option<u32>,
        cgroup_root: Option<PathBuf>,
    ) -> Result<Self, String> {
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
        let cgroup = match cgroup_root {
            Some(root) => {
                let name = format!("anvil-plugin-{}", std::process::id());
                Some(
                    Cgroup::create(root, &name, 64, 256 << 20)
                        .map_err(|error| format!("cgroup: {error}"))?,
                )
            }
            None => None,
        };
        let mut args = vec!["--socket".to_owned(), socket.display().to_string()];
        if let Some(mb) = allocate_mb {
            args.push("--allocate-mb".to_owned());
            args.push(mb.to_string());
        }
        let token = token();
        let process = spawn(host, &args, token).map_err(|error| format!("spawn host: {error}"))?;
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

    #[allow(dead_code)]
    pub fn id(&self) -> u32 {
        self.host.id()
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

#[cfg(test)]
mod tests {
    use super::*;
    use nix::libc;

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
        let Ok(mut session) = HostSession::start_with_host(Path::new("."), &host, None, None)
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
        let root = std::env::var_os("ANVIL_CGROUP_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/sys/fs/cgroup"));
        if !root.is_dir() {
            eprintln!("skipping OOM test: cgroup2 unavailable");
            return;
        }
        let Ok(mut session) =
            HostSession::start_with_host(Path::new("."), &host, Some(300), Some(root))
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
}

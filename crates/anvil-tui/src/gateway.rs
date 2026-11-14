//! Per-session gateway process and client ownership.

use std::{
    env,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anvil_ipc::GatewayClient;
use sha2::{Digest, Sha256};

/// A running gateway child plus its socket path.
pub struct GatewaySession {
    child: Child,
    socket: Option<PathBuf>,
    lsp_token: [u8; 32],
    stopped: bool,
}

impl GatewaySession {
    /// Start `anvil-gateway` for `workspace` and return a handshaken client.
    pub fn start(workspace: &Path) -> Result<(Self, GatewayClient, GatewayClient), String> {
        let workspace = workspace
            .canonicalize()
            .map_err(|error| format!("workspace {}: {error}", workspace.display()))?;
        if !workspace.is_dir() {
            return Err(format!(
                "workspace is not a directory: {}",
                workspace.display()
            ));
        }
        let binary = gateway_binary();
        let token = random_token()?;
        let lsp_token = random_token()?;
        let hash = hex(&Sha256::digest(token));
        let lsp_hash = hex(&Sha256::digest(lsp_token));
        let socket = socket_path()?;
        let socket_arg = socket.to_string_lossy().into_owned();
        let workspace_arg = workspace.to_string_lossy().into_owned();
        let gateway_stderr = env::var_os("ANVIL_GATEWAY_LOG")
            .and_then(|path| File::create(path).ok())
            .map(Stdio::from)
            .unwrap_or_else(Stdio::null);
        let mut command = Command::new(&binary);
        command
            .arg("-socket")
            .arg(&socket_arg)
            .arg("-workspace")
            .arg(&workspace_arg)
            .arg("-plugin-id")
            .arg("anvil-tui")
            .arg("-token-sha256")
            .arg(&hash)
            .arg("-lsp-token-sha256")
            .arg(&lsp_hash)
            .arg("-allow-read")
            .arg("-allow-write")
            .arg("-no-sandbox-lsp")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(gateway_stderr);
        if let Some(runner) = gateway_binary()
            .parent()
            .map(|parent| parent.join("anvil-exec-runner"))
            .filter(|path| path.is_file())
        {
            command.arg("-allow-exec").arg("-exec-runner").arg(runner);
        }
        let child = command
            .spawn()
            .map_err(|error| format!("start {}: {error}", binary.display()))?;
        let mut session = Self {
            child,
            socket: Some(socket),
            lsp_token,
            stopped: false,
        };
        let result = (|| {
            let client = session.connect(&token)?;
            let lsp_client = session.connect(&lsp_token)?;
            Ok((client, lsp_client))
        })();
        match result {
            Ok((client, lsp_client)) => Ok((session, client, lsp_client)),
            Err(error) => {
                session.stop();
                Err(error)
            }
        }
    }

    /// Start the fetch-only installer role for one allowed host.
    pub fn start_installer(workspace: &Path, host: &str) -> Result<(Self, GatewayClient), String> {
        if host.is_empty() || host.chars().any(char::is_whitespace) {
            return Err("installer host is required".to_owned());
        }
        Self::start_installer_mode(workspace, Some(host))
    }

    /// Start an installer role for local package files only.
    pub fn start_local_installer(workspace: &Path) -> Result<(Self, GatewayClient), String> {
        Self::start_installer_mode(workspace, None)
    }

    fn start_installer_mode(
        workspace: &Path,
        host: Option<&str>,
    ) -> Result<(Self, GatewayClient), String> {
        let workspace = workspace
            .canonicalize()
            .map_err(|error| format!("workspace {}: {error}", workspace.display()))?;
        let binary = gateway_binary();
        let token = random_token()?;
        let hash = hex(&Sha256::digest(token));
        let socket = socket_path()?;
        let gateway_stderr = env::var_os("ANVIL_GATEWAY_LOG")
            .and_then(|path| File::create(path).ok())
            .map(Stdio::from)
            .unwrap_or_else(Stdio::null);
        let mut command = Command::new(&binary);
        command
            .arg("-socket")
            .arg(socket.to_string_lossy().as_ref())
            .arg("-workspace")
            .arg(workspace.to_string_lossy().as_ref())
            .arg("-plugin-id")
            .arg("anvil-installer")
            .arg("-role")
            .arg("installer")
            .arg("-token-sha256")
            .arg(&hash);
        if let Some(host) = host {
            command.arg("-allow-fetch").arg("-fetch-allow").arg(host);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(gateway_stderr);
        let child = command
            .spawn()
            .map_err(|error| format!("start {}: {error}", binary.display()))?;
        let mut session = Self {
            child,
            socket: Some(socket),
            lsp_token: [0; 32],
            stopped: false,
        };
        let client = match session.connect(&token) {
            Ok(client) => client,
            Err(error) => {
                session.stop();
                return Err(error);
            }
        };
        Ok((session, client))
    }

    /// Open a fresh authenticated LSP connection.
    pub fn connect_lsp(&mut self) -> Result<GatewayClient, String> {
        let token = self.lsp_token;
        self.connect(&token)
    }

    fn connect(&mut self, token: &[u8; 32]) -> Result<GatewayClient, String> {
        let socket = self
            .socket
            .clone()
            .ok_or_else(|| "gateway stopped".to_owned())?;
        let mut last_error = String::new();
        for _ in 0..100 {
            if let Some(status) = self
                .child
                .try_wait()
                .map_err(|error| format!("gateway status: {error}"))?
            {
                self.stopped = true;
                self.remove_socket();
                return Err(format!("gateway exited with {status}"));
            }
            match GatewayClient::connect(&socket, token) {
                Ok(client) => return Ok(client),
                Err(error) => last_error = error.to_string(),
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err(format!("gateway did not become ready: {last_error}"))
    }

    /// Ask the gateway to exit, then kill it after `timeout`.
    pub fn shutdown_gracefully(&mut self, timeout: Duration) -> bool {
        if self.stopped {
            return true;
        }
        #[cfg(unix)]
        {
            use nix::{
                sys::signal::{Signal, kill},
                unistd::Pid,
            };
            let Ok(pid) = i32::try_from(self.child.id()) else {
                self.stop();
                return false;
            };
            if kill(Pid::from_raw(pid), Signal::SIGTERM).is_err() {
                self.stop();
                return false;
            }
        }
        #[cfg(not(unix))]
        {
            self.stop();
            return false;
        }
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    self.stopped = true;
                    self.remove_socket();
                    return true;
                }
                Ok(None) => {}
                Err(_) => break,
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        self.stop();
        false
    }

    fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.remove_socket();
    }

    fn remove_socket(&mut self) {
        if let Some(socket) = self.socket.take() {
            let _ = std::fs::remove_file(socket);
        }
    }
}

impl Drop for GatewaySession {
    fn drop(&mut self) {
        self.stop();
    }
}

fn gateway_binary() -> PathBuf {
    if let Some(path) = env::var_os("ANVIL_GATEWAY") {
        return PathBuf::from(path);
    }
    if let Ok(executable) = env::current_exe()
        && let Some(parent) = executable.parent()
    {
        let sibling = parent.join("anvil-gateway");
        if sibling.exists() {
            return sibling;
        }
    }
    PathBuf::from("anvil-gateway")
}

fn random_token() -> Result<[u8; 32], String> {
    let mut token = [0u8; 32];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut token))
        .map_err(|error| format!("read /dev/urandom: {error}"))?;
    Ok(token)
}

fn socket_path() -> Result<PathBuf, String> {
    let suffix = random_token()?;
    Ok(env::temp_dir().join(format!(
        "anvil-tui-{}-{}.sock",
        std::process::id(),
        hex(&suffix[..8])
    )))
}

fn hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(result, "{byte:02x}");
    }
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn wait_for(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while !path.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(path.exists(), "child did not become ready");
    }

    fn test_session(shell: &str, marker: &Path) -> (GatewaySession, PathBuf) {
        let socket = marker.with_extension("sock");
        std::fs::write(&socket, b"stale").expect("socket marker");
        let child = Command::new("sh")
            .args(["-c", shell, "sh"])
            .arg(marker)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("test child");
        wait_for(marker);
        (
            GatewaySession {
                child,
                socket: Some(socket.clone()),
                lsp_token: [0; 32],
                stopped: false,
            },
            socket,
        )
    }

    #[test]
    fn graceful_shutdown_waits_and_removes_socket_once() {
        let directory = tempfile::tempdir().expect("tempdir");
        let marker = directory.path().join("ready");
        let graceful = marker.with_extension("graceful");
        let (mut session, socket) = test_session(
            "trap 'echo graceful > \"$1.graceful\"; exit 0' TERM; echo ready > \"$1\"; while :; do sleep 0.01; done",
            &marker,
        );
        assert!(session.shutdown_gracefully(Duration::from_secs(1)));
        assert!(graceful.exists());
        assert!(session.shutdown_gracefully(Duration::from_secs(1)));
        assert!(!socket.exists());
        session.stop();
        session.stop();
    }

    #[test]
    fn timeout_falls_back_to_kill_and_wait() {
        let directory = tempfile::tempdir().expect("tempdir");
        let marker = directory.path().join("ready");
        let (mut session, socket) =
            test_session("trap '' TERM; echo ready > \"$1\"; sleep 5", &marker);
        let started = Instant::now();
        assert!(!session.shutdown_gracefully(Duration::from_millis(30)));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(!socket.exists());
        session.stop();
    }
}

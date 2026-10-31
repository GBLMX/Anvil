//! Per-session gateway process and client ownership.

use std::{
    env,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use anvil_ipc::GatewayClient;
use sha2::{Digest, Sha256};

/// A running gateway child plus its socket path.
pub struct GatewaySession {
    child: Child,
    socket: PathBuf,
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
        let mut session = Self { child, socket };
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

    fn connect(&mut self, token: &[u8; 32]) -> Result<GatewayClient, String> {
        let mut last_error = String::new();
        for _ in 0..100 {
            if let Some(status) = self
                .child
                .try_wait()
                .map_err(|error| format!("gateway status: {error}"))?
            {
                return Err(format!("gateway exited with {status}"));
            }
            match GatewayClient::connect(&self.socket, token) {
                Ok(client) => return Ok(client),
                Err(error) => last_error = error.to_string(),
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err(format!("gateway did not become ready: {last_error}"))
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
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

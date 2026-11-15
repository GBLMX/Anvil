//! Concrete plugin host process adapter for the TUI frontend.

use anvil_plugin_host::{
    Cgroup, HostExit as PluginHostExit, HostProcess as PluginHostProcess, Token, classify_exit,
    spawn_in_cgroup,
};
use anvil_session::plugin_runtime::{HostExit, HostProcess, HostSpawner, HostStart};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) struct SystemHostSpawner;

impl HostSpawner for SystemHostSpawner {
    fn spawn(&self, start: &HostStart<'_>, socket: &Path) -> Result<Box<dyn HostProcess>, String> {
        let host = host_binary()?;
        let name = format!(
            "anvil-plugin-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or_default()
        );
        let cgroup = Cgroup::create(cgroup_root()?, &name, 64, 256 << 20, 100)
            .map_err(|error| format!("cgroup: {error}"))?;
        let mut args = vec![
            "--socket".to_owned(),
            socket.display().to_string(),
            "--plugin-id".to_owned(),
            start.plugin_id.to_owned(),
        ];
        if let Some(mb) = start.allocate_mb {
            args.push("--allocate-mb".to_owned());
            args.push(mb.to_string());
        }
        if let Some(plugin_root) = start.plugin_root {
            args.push("--plugin".to_owned());
            args.push(plugin_root.display().to_string());
        }
        if let Some(gateway_socket) = start.gateway_socket {
            args.push("--gateway-socket".to_owned());
            args.push(gateway_socket.display().to_string());
        }
        if let Some(editor_socket) = start.editor_socket {
            args.push("--editor-socket".to_owned());
            args.push(editor_socket.display().to_string());
        }
        if let Some(command) = start.command {
            args.push("--activation-command".to_owned());
            args.push(command.to_owned());
        }
        let process = spawn_in_cgroup(&host, &args, Token(start.token.0), cgroup)
            .map_err(|error| format!("spawn host: {error}"))?;
        Ok(Box::new(SystemHostProcess(process)))
    }
}

struct SystemHostProcess(PluginHostProcess);

impl HostProcess for SystemHostProcess {
    fn id(&self) -> u32 {
        self.0.id()
    }

    fn try_wait(&mut self) -> io::Result<Option<HostExit>> {
        let memory_limited = self.0.memory_limited();
        Ok(self
            .0
            .try_wait()?
            .map(|status| match classify_exit(status, memory_limited) {
                PluginHostExit::Exited(code) => HostExit::Exited(code),
                PluginHostExit::Signaled(signal) => HostExit::Signaled(signal),
                PluginHostExit::OomKilled => HostExit::OomKilled,
            }))
    }

    fn terminate(&mut self) {
        self.0.terminate();
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

fn cgroup_root() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os("ANVIL_CGROUP_ROOT") {
        return Ok(PathBuf::from(root));
    }
    Cgroup::detect_root().map_err(|error| format!("cgroup: {error}"))
}

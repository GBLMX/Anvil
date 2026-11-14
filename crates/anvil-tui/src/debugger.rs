//! Local breakpoint state and optional DAP synchronization.

use super::*;
use crate::dap::{DapCancel, DapClient, DapWorker};
use anvil_session::provider::{
    DapAdapterManager, DapAdapterProvider, DapAdapterSpec, DapContext, ProviderId, ProviderOrigin,
    ProviderPriority, RustDapProvider,
};

impl Model {
    pub(crate) fn start_dap_attach(&mut self, program: PathBuf, args: Vec<String>) {
        if self.dap_attach_blocked() {
            return;
        }
        let workspace = self.workspace.path.clone();
        let Some(program) = resolve_adapter_program(&program) else {
            self.status.text = format!("E: dap adapter not found: {}", program.display());
            self.file.dirty = true;
            return;
        };
        let spec = match DapAdapterManager::validate(
            DapAdapterSpec {
                id: ProviderId::new("builtin.explicit"),
                program,
                args,
                language_id: "explicit".to_owned(),
                cwd: Some(workspace.clone()),
            },
            &workspace,
        ) {
            Ok(spec) => spec,
            Err(error) => {
                self.status.text = format!("E: dap adapter: {error}");
                self.file.dirty = true;
                return;
            }
        };
        self.spawn_dap_spec(spec);
    }

    /// Reject a new attach while one is starting or a client is already active.
    fn dap_attach_blocked(&mut self) -> bool {
        if self.language.dap.attach.is_some() {
            self.status.text = "E: dap attach already in progress".to_owned();
            self.file.dirty = true;
            return true;
        }
        if self.language.dap.client.is_some() || self.language.dap.request.is_some() {
            self.status.text = "E: debug adapter already attached; use :dap-detach".to_owned();
            self.file.dirty = true;
            return true;
        }
        false
    }

    pub(crate) fn start_dap_auto(&mut self) {
        if self.dap_attach_blocked() {
            return;
        }
        let Some(path) = self.session.path.as_deref() else {
            self.status.text = "E: dap auto requires a file".to_owned();
            self.file.dirty = true;
            return;
        };
        let Some(language_id) = dap_language(path) else {
            self.status.text = "E: no builtin debug adapter for this file".to_owned();
            self.file.dirty = true;
            return;
        };
        let workspace = self.workspace.path.clone();
        let mut manager = DapAdapterManager::new();
        let provider = std::sync::Arc::new(RustDapProvider);
        if manager
            .register(
                provider.id(),
                ProviderPriority::new(1),
                ProviderOrigin::Builtin,
                provider,
            )
            .is_err()
        {
            self.status.text = "E: dap provider registration failed".to_owned();
            self.file.dirty = true;
            return;
        }
        let context = DapContext {
            path: Some(path),
            language_id,
        };
        match manager.resolve(&context, &workspace) {
            Ok(Some(spec)) => self.spawn_dap_spec(spec),
            Ok(None) => {
                self.status.text = "E: no debug adapter available".to_owned();
                self.file.dirty = true;
            }
            Err(error) => {
                self.status.text = format!("E: dap adapter: {error}");
                self.file.dirty = true;
            }
        }
    }

    fn spawn_dap_spec(&mut self, spec: DapAdapterSpec) {
        if self.dap_attach_blocked() {
            return;
        }
        let display = spec.program.display().to_string();
        let program = spec.program;
        let args = spec.args;
        let workspace = spec.cwd.unwrap_or_else(|| self.workspace.path.clone());
        let generation = self.language.dap.generation;
        let cancel = DapCancel::new();
        let worker_cancel = cancel.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("anvil-dap-attach".to_owned())
            .spawn(move || {
                let result =
                    DapClient::spawn_cancellable(&program, &args, &workspace, &worker_cancel)
                        .and_then(|mut client| {
                            let request = client.initialize()?;
                            client.wait_response(request).map(|_| client)
                        })
                        .map_err(|error| error.to_string());
                let _ = sender.send((generation, result));
            });
        match worker {
            Ok(handle) => {
                self.language.dap.attach_worker = Some(DapWorker::new(cancel, handle));
                self.language.dap.attach = Some(receiver);
                self.language.dap.attach_started = Some(Instant::now());
                self.status.text = format!("attaching debug adapter: {display}");
            }
            Err(error) => {
                self.status.text = format!("E: dap attach worker: {error}");
            }
        }
        self.file.dirty = true;
    }

    pub(crate) fn poll_dap_attach(&mut self) {
        let received = self
            .language
            .dap
            .attach
            .as_ref()
            .map(std::sync::mpsc::Receiver::try_recv);
        let Some(received) = received else {
            return;
        };
        match received {
            Ok((generation, result)) => {
                let worker = self.language.dap.attach_worker.take();
                self.language.dap.attach = None;
                self.language.dap.attach_started = None;
                if let Some(worker) = worker {
                    worker.finish();
                }
                if generation != self.language.dap.generation {
                    // Detached or superseded while the adapter was starting; the
                    // client (if any) is dropped here and its process group dies.
                    return;
                }
                match result {
                    Ok(client) => {
                        self.attach_dap_client(client);
                        self.status.text = "debug adapter attached".to_owned();
                    }
                    Err(error) => self.status.text = format!("E: dap initialize: {error}"),
                }
                self.file.dirty = true;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                let worker = self.language.dap.attach_worker.take();
                self.language.dap.attach = None;
                self.language.dap.attach_started = None;
                drop(worker);
                self.status.text = "E: dap attach worker stopped".to_owned();
                self.file.dirty = true;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if self
                    .language
                    .dap
                    .attach_started
                    .is_some_and(|started| started.elapsed() >= DAP_ATTACH_TIMEOUT)
                {
                    // Dropping the worker kills the adapter's process group and
                    // joins the thread, so a hung adapter cannot pin a thread.
                    let worker = self.language.dap.attach_worker.take();
                    self.language.dap.attach = None;
                    self.language.dap.attach_started = None;
                    drop(worker);
                    self.status.text = "E: dap attach timed out (adapter may be hung)".to_owned();
                    self.file.dirty = true;
                }
            }
        }
    }

    fn attach_dap_client(&mut self, client: DapClient) {
        self.language.dap.generation = self.language.dap.generation.wrapping_add(1);
        self.language.dap.client = Some(client);
        self.sync_all_breakpoints();
    }

    /// Drop the adapter and any in-flight attach or refresh work.
    ///
    /// The generation bump makes late worker results inert; dropping the workers
    /// kills their adapter process groups and joins their threads.
    pub(crate) fn detach_dap(&mut self) {
        self.language.dap.generation = self.language.dap.generation.wrapping_add(1);
        self.language.dap.client = None;
        self.language.dap.attach = None;
        self.language.dap.attach_started = None;
        self.language.dap.request = None;
        self.language.dap.request_started = None;
        self.language.dap.attach_worker = None;
        self.language.dap.request_worker = None;
        if let Some(panel) = self.overlays.debug_panel.as_mut() {
            panel.pending = false;
        }
    }

    pub(crate) fn toggle_breakpoint(&mut self) {
        let Some(path) = self.session.path.clone() else {
            self.status.text = "E: breakpoint requires a file".to_owned();
            self.file.dirty = true;
            return;
        };
        let Some(line) = self.current_line().map(|line| line as u32 + 1) else {
            self.status.text = "E: breakpoint requires a line".to_owned();
            self.file.dirty = true;
            return;
        };
        let entry = self
            .language
            .dap
            .breakpoints
            .entry(path.clone())
            .or_default();
        if !entry.remove(&line) {
            entry.insert(line);
        }
        let enabled = entry.contains(&line);
        if entry.is_empty() {
            self.language.dap.breakpoints.remove(&path);
        }
        self.sync_breakpoints(&path);
        self.status.text = if enabled {
            format!("breakpoint {}:{line}", path.display())
        } else {
            format!("breakpoint cleared {}:{line}", path.display())
        };
        self.file.dirty = true;
    }

    pub(crate) fn breakpoint_marker(&self, path: &Path, line: usize) -> bool {
        self.language
            .dap
            .breakpoints
            .get(path)
            .is_some_and(|lines| lines.contains(&(line as u32 + 1)))
    }

    fn sync_breakpoints(&mut self, path: &Path) {
        let Some(dap) = self.language.dap.client.as_mut() else {
            return;
        };
        let lines = self
            .language
            .dap
            .breakpoints
            .get(path)
            .map(|lines| lines.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        if let Err(error) = dap.set_breakpoints(path, &lines) {
            self.status.text = format!("E: breakpoint sync: {error}");
            self.file.dirty = true;
        }
    }

    fn sync_all_breakpoints(&mut self) {
        let paths = self
            .language
            .dap
            .breakpoints
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for path in paths {
            self.sync_breakpoints(&path);
        }
    }
}

/// How long an adapter may take to spawn and answer `initialize` before its
/// process group is killed and the attach is reported as timed out.
const DAP_ATTACH_TIMEOUT: Duration = Duration::from_secs(5);

fn dap_language(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("rs") => Some("rust"),
        _ => None,
    }
}

fn resolve_adapter_program(program: &Path) -> Option<PathBuf> {
    if program.is_absolute() {
        return program.is_file().then(|| program.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
}

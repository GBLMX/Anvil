//! Local breakpoint state and optional DAP synchronization.

use super::*;
use crate::dap::DapClient;
use anvil_session::provider::{DapAdapterManager, DapAdapterSpec, ProviderId};

impl Model {
    pub(crate) fn start_dap_attach(&mut self, program: PathBuf, args: Vec<String>) {
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
        let display = spec.program.display().to_string();
        let program = spec.program;
        let args = spec.args;
        let workspace = spec.cwd.unwrap_or(workspace);
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = DapClient::spawn(&program, &args, &workspace)
                .and_then(|mut client| {
                    let request = client.initialize()?;
                    client.wait_response(request).map(|_| client)
                })
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        self.language.dap.attach = Some(receiver);
        self.language.dap.attach_started = Some(Instant::now());
        self.status.text = format!("attaching debug adapter: {display}");
        self.file.dirty = true;
    }

    pub(crate) fn poll_dap_attach(&mut self) {
        let Some(receiver) = self.language.dap.attach.as_ref() else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(client)) => {
                self.language.dap.attach = None;
                self.language.dap.attach_started = None;
                self.attach_dap_client(client);
                self.status.text = "debug adapter attached".to_owned();
                self.file.dirty = true;
            }
            Ok(Err(error)) => {
                self.language.dap.attach = None;
                self.language.dap.attach_started = None;
                self.status.text = format!("E: dap initialize: {error}");
                self.file.dirty = true;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.language.dap.attach = None;
                self.language.dap.attach_started = None;
                self.status.text = "E: dap attach worker stopped".to_owned();
                self.file.dirty = true;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if self
                    .language
                    .dap
                    .attach_started
                    .is_some_and(|started| started.elapsed() >= Duration::from_secs(5))
                {
                    self.language.dap.attach = None;
                    self.language.dap.attach_started = None;
                    self.status.text = "E: dap attach timed out (adapter may be hung)".to_owned();
                    self.file.dirty = true;
                }
            }
        }
    }

    fn attach_dap_client(&mut self, client: DapClient) {
        self.language.dap.client = Some(client);
        self.sync_all_breakpoints();
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

fn resolve_adapter_program(program: &Path) -> Option<PathBuf> {
    if program.is_absolute() {
        return program.is_file().then(|| program.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
}

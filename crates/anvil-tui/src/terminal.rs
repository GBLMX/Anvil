//! Terminal lifecycle and the main event loop.

use super::*;
use anvil_tui_kit::{TerminalOptions, TerminalSession};
use std::io::{IsTerminal, Read};
use std::sync::Arc;
/// Runs the minimal terminal loop in the current directory workspace.
pub fn run(path: Option<PathBuf>) -> io::Result<()> {
    run_with_workspace(path, std::env::current_dir()?)
}

/// Runs the minimal terminal loop with an explicit workspace.
pub fn run_with_workspace(path: Option<PathBuf>, workspace: PathBuf) -> io::Result<()> {
    run_with_workspace_status(path, workspace, false).map(|_| ())
}

/// Runs the terminal loop and returns the process exit code.
pub fn run_with_workspace_status(
    path: Option<PathBuf>,
    workspace: PathBuf,
    editor_mode: bool,
) -> io::Result<i32> {
    run_with_input(path, workspace, editor_mode, false, false)
}

#[cfg(feature = "official-git")]
fn default_git_service() -> Option<Arc<dyn anvil_module::git::GitService>> {
    Some(Arc::new(anvil_mod_git::SystemGitService))
}

#[cfg(not(feature = "official-git"))]
fn default_git_service() -> Option<Arc<dyn anvil_module::git::GitService>> {
    None
}

/// Runs the terminal loop with optional stdin input and read-only mode.
pub fn run_with_input(
    path: Option<PathBuf>,
    workspace: PathBuf,
    editor_mode: bool,
    read_stdin: bool,
    read_only: bool,
) -> io::Result<i32> {
    run_with_input_and_git(
        path,
        workspace,
        editor_mode,
        read_stdin,
        read_only,
        default_git_service(),
    )
}

/// Runs the terminal loop with an explicitly selected Git service.
pub fn run_with_input_and_git(
    path: Option<PathBuf>,
    workspace: PathBuf,
    editor_mode: bool,
    read_stdin: bool,
    read_only: bool,
    git: Option<Arc<dyn anvil_module::git::GitService>>,
) -> io::Result<i32> {
    run_with_services(
        path,
        workspace,
        editor_mode,
        read_stdin,
        read_only,
        git,
        None,
    )
}

/// Runs the terminal loop with explicitly selected distribution services.
pub fn run_with_services(
    path: Option<PathBuf>,
    workspace: PathBuf,
    editor_mode: bool,
    read_stdin: bool,
    read_only: bool,
    git: Option<Arc<dyn anvil_module::git::GitService>>,
    workspace_index: Option<Arc<dyn anvil_module::workspace_index::WorkspaceIndexService>>,
) -> io::Result<i32> {
    run_with_all_services(
        path,
        workspace,
        editor_mode,
        read_stdin,
        read_only,
        DistributionServices {
            git,
            workspace_index,
            desktop: None,
            language_tooling: None,
        },
    )
}

#[derive(Default, Clone)]
pub struct DistributionServices {
    pub git: Option<Arc<dyn anvil_module::git::GitService>>,
    pub workspace_index: Option<Arc<dyn anvil_module::workspace_index::WorkspaceIndexService>>,
    pub desktop: Option<Arc<dyn anvil_module::desktop::DesktopService>>,
    pub language_tooling: Option<Arc<dyn anvil_module::language::LanguageToolingService>>,
}

/// Runs the terminal loop with the full distribution service set.
pub fn run_with_all_services(
    path: Option<PathBuf>,
    workspace: PathBuf,
    editor_mode: bool,
    read_stdin: bool,
    read_only: bool,
    services: DistributionServices,
) -> io::Result<i32> {
    let DistributionServices {
        git,
        workspace_index,
        desktop,
        language_tooling,
    } = services;
    let stdin_text = if read_stdin || !io::stdin().is_terminal() {
        if !io::stdout().is_terminal() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stdin mode requires a terminal stdout",
            ));
        }
        let text = read_stdin_bounded()?;
        reopen_tty_stdin()?;
        Some(text)
    } else {
        None
    };
    let enable_control = control_enabled(editor_mode, stdin_text.is_some());
    let session_enabled = !editor_mode && stdin_text.is_none();
    let restore_session = session_enabled && path.is_none();
    let timing = std::env::var_os("ANVIL_OPEN_TIMING").is_some();
    let start = Instant::now();
    let workspace = workspace.canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("workspace {}: {error}", workspace.display()),
        )
    })?;
    if !workspace.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("workspace is not a directory: {}", workspace.display()),
        ));
    }
    theme::init_color_support();
    theme::set_background_is_light(terminal_background::query_is_light());
    let (mut model, session) = match GatewaySession::start(&workspace) {
        Ok((session, client, lsp_client)) => {
            let mut model = Model::with_gateway(client, workspace.clone());
            model.workspace.git_service = git.clone();
            model.workspace.workspace_index_service = workspace_index.clone();
            model.workspace.desktop_service = desktop.clone();
            model.workspace.language_tooling_service = language_tooling.clone();
            model.language.lsp.client = Some(lsp_client);
            if stdin_text.is_none()
                && let Some(path) = path
                && let Err(error) = model.open_path(path)
            {
                model.status.text = format!("E: {error}");
                model.file.dirty = true;
            }
            (model, Some(session))
        }
        Err(error) => {
            let mut model = Model::new(EditorState::from_text(""), None);
            model.workspace.git_service = git.clone();
            model.workspace.workspace_index_service = workspace_index.clone();
            model.workspace.desktop_service = desktop.clone();
            model.workspace.language_tooling_service = language_tooling.clone();
            model.workspace.path = workspace.clone();
            model.reload_icons();
            model.workspace.require_gateway = true;
            model.status.text = format!("E: gateway unavailable: {error}");
            model.file.dirty = true;
            (model, None)
        }
    };
    model.workspace.gateway_session = session;
    model.set_editor_mode(editor_mode);
    if session_enabled {
        model.install_recents(crate::recents::load());
        model.record_recent_workspace(&workspace);
    }

    if let Some(text) = stdin_text {
        model.session.editor = EditorState::from_text(&text).into();
        model.session.path = None;
        model.session.cursor = 0;
        model.view.scroll = 0;
        model.view.hscroll = 0;
        model.file.modified = false;
        model.file.missing = false;
        model.set_stdin_buffer(read_only);
        model.set_pipe_locations(&text);
        model
            .syntax_cache
            .highlights
            .borrow_mut()
            .switch_path(None, false);
        model.status.text = if read_only {
            "[stdin] read-only".to_owned()
        } else {
            "[stdin]".to_owned()
        };
        model.file.dirty = true;
    }
    model.set_read_only(read_only);
    if restore_session && let Err(error) = session::restore(&mut model, &workspace) {
        let message = format!("E: session: {error}");
        if model.status.text.is_empty() {
            model.status.text = message;
        } else {
            model.status.text.push_str(" | ");
            model.status.text.push_str(&message);
        }
        model.file.dirty = true;
    }
    if let Err(error) = theme::load_custom_themes() {
        let message = format!("E: themes: {error}");
        if model.status.text.is_empty() {
            model.status.text = message;
        } else {
            model.status.text.push_str(" | ");
            model.status.text.push_str(&message);
        }
        model.file.dirty = true;
    }
    let project_trusted = model
        .workspace
        .roots
        .primary()
        .is_some_and(|root| root.trusted);
    match settings::load_for_workspace(&workspace, project_trusted) {
        Ok(settings) => model.apply_settings(settings),
        Err(error) => {
            let message = format!("E: settings: {error}");
            if model.status.text.is_empty() {
                model.status.text = message;
            } else {
                model.status.text.push_str(" | ");
                model.status.text.push_str(&message);
            }
            model.file.dirty = true;
        }
    }
    if let Some(theme_name) = std::env::var_os("ANVIL_THEME") {
        let theme_name = theme_name.to_string_lossy();
        if let Err(error) = theme::set_startup_theme(&theme_name) {
            let message = format!("E: {error}");
            if model.status.text.is_empty() {
                model.status.text = message;
            } else {
                model.status.text.push_str(" | ");
                model.status.text.push_str(&message);
            }
            model.file.dirty = true;
        }
    }
    theme::set_high_contrast(model.config.settings.ui.high_contrast);
    if let Err(error) = model.load_keymap_config() {
        let message = format!("E: keymap config: {error}");
        if model.status.text.is_empty() {
            model.status.text = message;
        } else {
            model.status.text.push_str(" | ");
            model.status.text.push_str(&message);
        }
        model.file.dirty = true;
    }

    #[cfg(target_os = "linux")]
    if let Err(error) = model.load_plugins() {
        let message = format!("E: plugins: {error}");
        if model.status.text.is_empty() {
            model.status.text = message;
        } else {
            model.status.text.push_str(" | ");
            model.status.text.push_str(&message);
        }
        model.file.dirty = true;
    }
    #[cfg(target_os = "linux")]
    if startup_activation_allowed() {
        model.activate_plugins(ActivationEvent::Startup);
    }
    #[cfg(target_os = "linux")]
    model.activate_workspace_plugins();
    #[cfg(target_os = "linux")]
    model.activate_current_language();
    let control = if enable_control {
        match anvil_control::ControlServer::start_for(&workspace) {
            Ok(server) => Some(server),
            Err(anvil_control::Error::AlreadyRunning) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "another Anvil instance is already running",
                ));
            }
            Err(error) => {
                let message = format!("E: control: {error}");
                if model.status.text.is_empty() {
                    model.status.text = message;
                } else {
                    model.status.text.push_str(" | ");
                    model.status.text.push_str(&message);
                }
                model.file.dirty = true;
                None
            }
        }
    } else {
        None
    };
    let model_ready = start.elapsed();
    let mut terminal = TerminalSession::start_with(TerminalOptions {
        keyboard_enhancement: !crate::terminal_background::terminal_queries_disabled(),
    })?;
    let terminal_ready = start.elapsed();
    let mut first_draw = true;
    let mut last_session_revision = model.session.editor.revision();
    let mut last_session_active = model.buffers.active;
    let mut session_saved_at = Instant::now();
    let mut session_error_reported = false;
    let mut insert_cursor: Option<bool> = None;
    loop {
        let wants_bar = matches!(model.ui.input_mode, InputMode::Insert);
        if insert_cursor != Some(wants_bar) {
            terminal.set_cursor_shape(wants_bar)?;
            insert_cursor = Some(wants_bar);
        }
        let size = terminal.terminal().size()?;
        model.view.viewport_height = size.height;
        model.view.viewport_width = size.width;
        #[cfg(unix)]
        model.resize_terminal();
        let text_height = model.editor_text_height(size.height);
        let text_width = model.editor_text_width(size.width);
        model.ensure_cursor_visible(text_height, text_width);
        model.tick_status();
        model.poll_icons();
        model.poll_dap_attach();
        model.poll_debug_request();
        model.poll_sticky_symbols();
        match theme::reload_custom_if_changed() {
            Ok(true) => model.file.dirty = true,
            Ok(false) => {}
            Err(error) => {
                model.status.text = format!("E: theme reload: {error}");
                model.file.dirty = true;
            }
        }
        model.tick_search();
        if model.file.dirty {
            terminal.terminal().draw(|frame| view(&model, frame))?;
            model.file.dirty = false;
            if first_draw {
                first_draw = false;
                if timing {
                    eprintln!(
                        "open_timing model={:?} terminal={:?} first_frame={:?}",
                        model_ready,
                        terminal_ready,
                        start.elapsed()
                    );
                }
            }
        }
        model.poll_host();
        model.poll_pkg_notifications();
        model.poll_syntax();
        model.poll_lsp();
        model.poll_lsp_retry();
        model.poll_file_worker();
        model.poll_task();
        #[cfg(unix)]
        model.poll_terminal();
        if let Some(control) = &control {
            while let Ok(call) = control.try_recv() {
                let reply = model.handle_control(call.request);
                let _ = call.reply.send(reply);
            }
        }
        if model.status.busy.is_some() && !model.config.settings.ui.reduced_motion {
            model.status.spinner = model.status.spinner.wrapping_add(1);
            model.file.dirty = true;
        }
        model.poll_inlay_hints(size.height.saturating_sub(2));
        model.poll_git();
        model.expire_leader();
        if event::poll(std::time::Duration::from_millis(50))? {
            handle_event(&mut model, event::read()?);
        }
        if session_enabled
            && (model.session.editor.revision() != last_session_revision
                || model.buffers.active != last_session_active)
            && session_saved_at.elapsed() >= std::time::Duration::from_secs(2)
        {
            match session::save(&mut model, &workspace) {
                Ok(()) => {
                    session_error_reported = false;
                    last_session_revision = model.session.editor.revision();
                    last_session_active = model.buffers.active;
                    session_saved_at = Instant::now();
                }
                Err(error) => {
                    if !session_error_reported {
                        model.status.text = format!("E: session save: {error}");
                        model.file.dirty = true;
                        session_error_reported = true;
                    }
                }
            }
        }
        if model.file.quit {
            if session_enabled {
                let _ = session::save(&mut model, &workspace);
            }
            return Ok(model.exit_code());
        }
    }
}

fn read_stdin_bounded() -> io::Result<String> {
    let mut data = Vec::new();
    io::stdin()
        .take(MAX_OPEN_BYTES as u64 + 1)
        .read_to_end(&mut data)?;
    if data.len() > MAX_OPEN_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("stdin exceeds {MAX_OPEN_BYTES} bytes"),
        ));
    }
    String::from_utf8(data)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "stdin is not valid UTF-8"))
}

#[cfg(unix)]
fn reopen_tty_stdin() -> io::Result<()> {
    use std::os::fd::AsRawFd;

    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")?;
    let result = unsafe { nix::libc::dup2(tty.as_raw_fd(), 0) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn reopen_tty_stdin() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "stdin mode requires /dev/tty",
    ))
}

#[cfg(target_os = "linux")]
impl Drop for Model {
    fn drop(&mut self) {
        self.runtime.task_worker = None;
        #[cfg(unix)]
        {
            self.runtime.terminal = None;
        }
        self.workspace.file_worker = None;
        self.workspace.root_file_workers.clear();
        self.language.lsp.workers.clear();
        self.language.lsp.client = None;
        self.language.dap.client = None;
        self.language.dap.attach = None;
        self.language.dap.request = None;
        self.plugins
            .supervisor
            .shutdown(self.workspace.gateway.as_mut());
        self.workspace.gateway = None;
        let timeout = std::time::Duration::from_millis(250);
        if let Some(mut session) = self.workspace.gateway_session.take() {
            session.shutdown_gracefully(timeout);
        }
        let gateways = std::mem::take(&mut self.workspace.root_gateways);
        for (_, (mut session, client)) in gateways {
            drop(client);
            session.shutdown_gracefully(timeout);
        }
        self.stop_installer();
    }
}

fn control_enabled(editor_mode: bool, stdin: bool) -> bool {
    !editor_mode && !stdin
}

#[cfg(test)]
mod tests {
    use super::control_enabled;

    #[test]
    fn editor_and_stdin_modes_do_not_claim_the_control_socket() {
        assert!(control_enabled(false, false));
        assert!(!control_enabled(true, false));
        assert!(!control_enabled(false, true));
        assert!(!control_enabled(true, true));
    }
}

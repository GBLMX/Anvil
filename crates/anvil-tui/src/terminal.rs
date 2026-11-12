//! Terminal lifecycle and the main event loop.

use super::*;
use std::io::{IsTerminal, Read};
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

/// Runs the terminal loop with optional stdin input and read-only mode.
pub fn run_with_input(
    path: Option<PathBuf>,
    workspace: PathBuf,
    editor_mode: bool,
    read_stdin: bool,
    read_only: bool,
) -> io::Result<i32> {
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
    theme::set_background_is_light(terminal_background::query_is_light());
    let (mut model, session) = match GatewaySession::start(&workspace) {
        Ok((session, client, lsp_client)) => {
            let mut model = Model::with_gateway(client, workspace.clone());
            model.lsp_client = Some(lsp_client);
            if stdin_text.is_none()
                && let Some(path) = path
                && let Err(error) = model.open_path(path)
            {
                model.status = format!("E: {error}");
                model.dirty = true;
            }
            (model, Some(session))
        }
        Err(error) => {
            let mut model = Model::new(EditorState::from_text(""), None);
            model.workspace = workspace.clone();
            model.reload_icons();
            model.require_gateway = true;
            model.status = format!("E: gateway unavailable: {error}");
            model.dirty = true;
            (model, None)
        }
    };
    model.gateway_session = session;
    model.set_editor_mode(editor_mode);

    if let Some(text) = stdin_text {
        model.editor = EditorState::from_text(&text).into();
        model.path = None;
        model.cursor = 0;
        model.scroll = 0;
        model.hscroll = 0;
        model.modified = false;
        model.missing = false;
        model.set_stdin_buffer(read_only);
        model.set_pipe_locations(&text);
        model.syntax.borrow_mut().switch_path(None, false);
        model.status = if read_only {
            "[stdin] read-only".to_owned()
        } else {
            "[stdin]".to_owned()
        };
        model.dirty = true;
    }
    model.set_read_only(read_only);
    if restore_session && let Err(error) = session::restore(&mut model, &workspace) {
        let message = format!("E: session: {error}");
        if model.status.is_empty() {
            model.status = message;
        } else {
            model.status.push_str(" | ");
            model.status.push_str(&message);
        }
        model.dirty = true;
    }
    if let Err(error) = theme::load_custom_themes() {
        let message = format!("E: themes: {error}");
        if model.status.is_empty() {
            model.status = message;
        } else {
            model.status.push_str(" | ");
            model.status.push_str(&message);
        }
        model.dirty = true;
    }
    if let Err(error) = theme_config::load() {
        let message = format!("E: theme config: {error}");
        if model.status.is_empty() {
            model.status = message;
        } else {
            model.status.push_str(" | ");
            model.status.push_str(&message);
        }
        model.dirty = true;
    }
    match settings::load() {
        Ok(settings) => model.apply_settings(settings),
        Err(error) => {
            let message = format!("E: settings: {error}");
            if model.status.is_empty() {
                model.status = message;
            } else {
                model.status.push_str(" | ");
                model.status.push_str(&message);
            }
            model.dirty = true;
        }
    }
    if let Some(theme_name) = std::env::var_os("ANVIL_THEME") {
        let theme_name = theme_name.to_string_lossy();
        if let Err(error) = theme::set_startup_theme(&theme_name) {
            let message = format!("E: {error}");
            if model.status.is_empty() {
                model.status = message;
            } else {
                model.status.push_str(" | ");
                model.status.push_str(&message);
            }
            model.dirty = true;
        }
    }
    if let Err(error) = model.load_keymap_config() {
        let message = format!("E: keymap config: {error}");
        if model.status.is_empty() {
            model.status = message;
        } else {
            model.status.push_str(" | ");
            model.status.push_str(&message);
        }
        model.dirty = true;
    }

    #[cfg(target_os = "linux")]
    if let Err(error) = model.load_plugins() {
        let message = format!("E: plugins: {error}");
        if model.status.is_empty() {
            model.status = message;
        } else {
            model.status.push_str(" | ");
            model.status.push_str(&message);
        }
        model.dirty = true;
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
        match anvil_control::ControlServer::start() {
            Ok(server) => Some(server),
            Err(anvil_control::Error::AlreadyRunning) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "another Anvil instance is already running",
                ));
            }
            Err(error) => {
                let message = format!("E: control: {error}");
                if model.status.is_empty() {
                    model.status = message;
                } else {
                    model.status.push_str(" | ");
                    model.status.push_str(&message);
                }
                model.dirty = true;
                None
            }
        }
    } else {
        None
    };
    let model_ready = start.elapsed();
    let mut terminal = TerminalSession::start()?;
    let terminal_ready = start.elapsed();
    let mut first_draw = true;
    let mut last_session_revision = model.editor.revision();
    let mut last_session_active = model.active;
    let mut session_saved_at = Instant::now();
    let mut session_error_reported = false;
    let mut insert_cursor: Option<bool> = None;
    loop {
        let wants_bar = matches!(model.ui.input_mode, InputMode::Insert);
        if insert_cursor != Some(wants_bar) {
            terminal.set_cursor_shape(wants_bar)?;
            insert_cursor = Some(wants_bar);
        }
        let size = terminal.terminal.size()?;
        model.viewport_height = size.height;
        model.viewport_width = size.width;
        #[cfg(unix)]
        model.resize_terminal();
        model.ensure_cursor_visible(size.height.saturating_sub(2), size.width.saturating_sub(2));
        model.tick_status();
        model.poll_icons();
        model.poll_dap_attach();
        model.poll_debug_request();
        model.poll_sticky_symbols();
        match theme::reload_custom_if_changed() {
            Ok(true) => model.dirty = true,
            Ok(false) => {}
            Err(error) => {
                model.status = format!("E: theme reload: {error}");
                model.dirty = true;
            }
        }
        model.tick_search();
        if model.dirty {
            terminal.terminal.draw(|frame| view(&model, frame))?;
            model.dirty = false;
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
        if model.busy.is_some() {
            model.spinner = model.spinner.wrapping_add(1);
            model.dirty = true;
        }
        model.poll_inlay_hints(size.height.saturating_sub(2));
        model.poll_git();
        model.expire_leader();
        if event::poll(std::time::Duration::from_millis(50))? {
            handle_event(&mut model, event::read()?);
        }
        if session_enabled
            && (model.editor.revision() != last_session_revision
                || model.active != last_session_active)
            && session_saved_at.elapsed() >= std::time::Duration::from_secs(2)
        {
            match session::save(&mut model, &workspace) {
                Ok(()) => {
                    session_error_reported = false;
                    last_session_revision = model.editor.revision();
                    last_session_active = model.active;
                    session_saved_at = Instant::now();
                }
                Err(error) => {
                    if !session_error_reported {
                        model.status = format!("E: session save: {error}");
                        model.dirty = true;
                        session_error_reported = true;
                    }
                }
            }
        }
        if model.quit {
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
        self.plugin_supervisor.shutdown(self.gateway.as_mut());
    }
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    keyboard_enhancement: bool,
}

impl TerminalSession {
    fn start() -> io::Result<Self> {
        enable_raw_mode()?;
        let supported = if std::env::var_os("ANVIL_SKIP_TERMINAL_QUERY").is_some() {
            false
        } else {
            supports_keyboard_enhancement().unwrap_or(false)
        };
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        if let Err(error) = execute!(stdout, EnableBracketedPaste) {
            let _ = execute!(stdout, DisableBracketedPaste, LeaveAlternateScreen);
            let _ = disable_raw_mode();
            return Err(error);
        }
        if let Err(error) = execute!(stdout, EnableFocusChange) {
            let _ = execute!(
                stdout,
                DisableFocusChange,
                DisableBracketedPaste,
                LeaveAlternateScreen
            );
            let _ = disable_raw_mode();
            return Err(error);
        }
        if let Err(error) = execute!(stdout, EnableMouseCapture) {
            let _ = execute!(stdout, DisableBracketedPaste, LeaveAlternateScreen);
            let _ = disable_raw_mode();
            return Err(error);
        }
        let keyboard_enhancement = supported
            && execute!(
                stdout,
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES,
                )
            )
            .is_ok();
        let backend = CrosstermBackend::new(stdout);
        match Terminal::new(backend) {
            Ok(terminal) => Ok(Self {
                terminal,
                keyboard_enhancement,
            }),
            Err(error) => {
                if keyboard_enhancement {
                    let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
                }
                let _ = execute!(
                    io::stdout(),
                    LeaveAlternateScreen,
                    DisableBracketedPaste,
                    DisableFocusChange
                );
                let _ = disable_raw_mode();
                Err(error)
            }
        }
    }
}

impl TerminalSession {
    fn set_cursor_shape(&mut self, insert: bool) -> io::Result<()> {
        let style = if insert {
            SetCursorStyle::SteadyBar
        } else {
            SetCursorStyle::SteadyBlock
        };
        crossterm::execute!(self.terminal.backend_mut(), style)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.keyboard_enhancement {
            let _ = execute!(self.terminal.backend_mut(), PopKeyboardEnhancementFlags);
        }
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            DisableFocusChange,
            DisableBracketedPaste,
            SetCursorStyle::DefaultUserShape,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
        let _ = self.terminal.show_cursor();
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

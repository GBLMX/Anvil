//! Terminal lifecycle and the main event loop.

use super::*;
/// Runs the minimal terminal loop in the current directory workspace.
pub fn run(path: Option<PathBuf>) -> io::Result<()> {
    run_with_workspace(path, std::env::current_dir()?)
}

/// Runs the minimal terminal loop with an explicit workspace.
pub fn run_with_workspace(path: Option<PathBuf>, workspace: PathBuf) -> io::Result<()> {
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
            if let Some(path) = path
                && let Err(error) = model.open_path(path)
            {
                model.status = format!("E: {error}");
                model.dirty = true;
            }
            (model, Some(session))
        }
        Err(error) => {
            let mut model = Model::new(EditorState::from_text(""), None);
            model.workspace = workspace;
            model.require_gateway = true;
            model.status = format!("E: gateway unavailable: {error}");
            model.dirty = true;
            (model, None)
        }
    };
    model.gateway_session = session;
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
    let model_ready = start.elapsed();
    let mut terminal = TerminalSession::start()?;
    let terminal_ready = start.elapsed();
    let mut first_draw = true;
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
        model.ensure_cursor_visible(size.height.saturating_sub(2), size.width.saturating_sub(2));
        model.tick_status();
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
        model.poll_file_worker();
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
        if model.quit {
            return Ok(());
        }
    }
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
                let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableBracketedPaste);
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
            DisableBracketedPaste,
            SetCursorStyle::DefaultUserShape,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
        let _ = self.terminal.show_cursor();
    }
}

//! Terminal lifecycle: alternate screen, raw mode, input modes, and cleanup.
//!
//! The guard restores every mode it enabled, including when startup fails
//! partway through. Applications own the draw loop and use [`TerminalSession::terminal`].

use std::io::{self, Stdout, Write};

use crossterm::{
    cursor::SetCursorStyle,
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

/// Startup options for [`TerminalSession`].
#[derive(Clone, Copy, Debug)]
pub struct TerminalOptions {
    /// Request terminal keyboard-enhancement support when available.
    pub keyboard_enhancement: bool,
}

impl Default for TerminalOptions {
    fn default() -> Self {
        Self {
            keyboard_enhancement: true,
        }
    }
}

/// Owns the terminal modes for a TUI session.
///
/// Dropping the session restores the alternate screen, raw mode, paste/focus
/// reporting, mouse capture, keyboard enhancement, and the cursor shape.
pub struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    keyboard_enhancement: bool,
}

impl TerminalSession {
    /// Start with default options.
    pub fn start() -> io::Result<Self> {
        Self::start_with(TerminalOptions::default())
    }

    /// Start with explicit options.
    pub fn start_with(options: TerminalOptions) -> io::Result<Self> {
        enable_raw_mode()?;
        let supported = options.keyboard_enhancement
            && crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        if let Err(error) = execute!(stdout, EnableBracketedPaste) {
            restore(&mut stdout, false, false, false, true, true);
            return Err(error);
        }
        if let Err(error) = execute!(stdout, EnableFocusChange) {
            restore(&mut stdout, false, false, true, true, true);
            return Err(error);
        }
        if let Err(error) = execute!(stdout, EnableMouseCapture) {
            restore(&mut stdout, false, true, true, true, true);
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
                restore(
                    &mut io::stdout(),
                    keyboard_enhancement,
                    true,
                    true,
                    true,
                    true,
                );
                Err(error)
            }
        }
    }

    /// Underlying ratatui terminal.
    pub fn terminal(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }

    /// Set the cursor shape for normal or insert mode.
    pub fn set_cursor_shape(&mut self, insert: bool) -> io::Result<()> {
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
        restore(
            self.terminal.backend_mut(),
            self.keyboard_enhancement,
            true,
            true,
            true,
            true,
        );
        let _ = self.terminal.show_cursor();
    }
}

fn restore<W: Write>(
    out: &mut W,
    keyboard_enhancement: bool,
    mouse: bool,
    focus: bool,
    bracketed_paste: bool,
    alternate_screen: bool,
) {
    if keyboard_enhancement {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    if mouse {
        let _ = execute!(out, DisableMouseCapture);
    }
    if focus {
        let _ = execute!(out, DisableFocusChange);
    }
    if bracketed_paste {
        let _ = execute!(out, DisableBracketedPaste);
    }
    let _ = execute!(out, SetCursorStyle::DefaultUserShape);
    if alternate_screen {
        let _ = execute!(out, LeaveAlternateScreen);
    }
    let _ = disable_raw_mode();
}

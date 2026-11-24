//! Terminal lifecycle: alternate screen, raw mode, input modes, cleanup, and
//! synchronized frame delivery.

use std::io::{self, Stdout, Write};

use crossterm::{
    cursor::SetCursorStyle,
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
        SetTitle, disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{Frame, Terminal, backend::CrosstermBackend};
use std::sync::{
    OnceLock,
    atomic::{AtomicU8, Ordering},
};

/// Terminal features enabled for a session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalProfile {
    pub keyboard_enhancement: bool,
    pub synchronized_output: bool,
    pub focus_events: bool,
    pub mouse_capture: bool,
    pub bracketed_paste: bool,
    pub truecolor: bool,
}

/// Startup options for [`TerminalSession`].
#[derive(Clone, Copy, Debug)]
pub struct TerminalOptions {
    /// Request terminal keyboard-enhancement support when available.
    pub keyboard_enhancement: bool,
    /// Batch each frame with the terminal's synchronized-update mode.
    pub synchronized_output: bool,
}

impl Default for TerminalOptions {
    fn default() -> Self {
        Self {
            keyboard_enhancement: true,
            synchronized_output: synchronized_output_enabled(),
        }
    }
}

/// Owns the terminal modes for a TUI session.
///
/// Dropping the session restores the alternate screen, raw mode, paste/focus
/// reporting, mouse capture, keyboard enhancement, synchronized-update mode,
/// and the cursor shape.
pub struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    profile: TerminalProfile,
    synchronized_active: bool,
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
            restore(&mut stdout, false, false, false, false, false, true);
            return Err(error);
        }
        if let Err(error) = execute!(stdout, EnableFocusChange) {
            restore(&mut stdout, false, false, false, true, false, true);
            return Err(error);
        }
        if let Err(error) = execute!(stdout, EnableMouseCapture) {
            restore(&mut stdout, false, false, true, true, false, true);
            return Err(error);
        }
        let keyboard_enhancement = supported
            && execute!(
                stdout,
                PushKeyboardEnhancementFlags(keyboard_enhancement_flags())
            )
            .is_ok();
        let profile = TerminalProfile {
            keyboard_enhancement,
            synchronized_output: options.synchronized_output,
            focus_events: true,
            mouse_capture: true,
            bracketed_paste: true,
            truecolor: supports_truecolor(),
        };
        let backend = CrosstermBackend::new(stdout);
        match Terminal::new(backend) {
            Ok(terminal) => Ok(Self {
                terminal,
                profile,
                synchronized_active: false,
            }),
            Err(error) => {
                restore(
                    &mut io::stdout(),
                    keyboard_enhancement,
                    true,
                    true,
                    true,
                    false,
                    true,
                );
                Err(error)
            }
        }
    }

    /// Capabilities enabled for this session.
    pub fn profile(&self) -> TerminalProfile {
        self.profile
    }

    /// Underlying ratatui terminal.
    pub fn terminal(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }

    /// Draw one frame, wrapping it in synchronized output when enabled.
    pub fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut Frame),
    {
        let synchronized = self.profile.synchronized_output;
        let sync_begun = synchronized && begin_synchronized(self.terminal.backend_mut()).is_ok();
        if synchronized && !sync_begun {
            self.profile.synchronized_output = false;
        }
        self.synchronized_active = sync_begun;
        let result = self.terminal.draw(render).map(|_| ());
        if sync_begun {
            let end = end_synchronized(self.terminal.backend_mut());
            if end.is_ok() {
                self.synchronized_active = false;
            }
            if result.is_ok() {
                end?;
            }
        }
        result
    }

    /// Set the terminal window title with control characters removed.
    pub fn set_title(&mut self, title: &str) -> io::Result<()> {
        let title = sanitize_title(title);
        execute!(self.terminal.backend_mut(), SetTitle(title))
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
            self.profile.keyboard_enhancement,
            self.profile.mouse_capture,
            self.profile.focus_events,
            self.profile.bracketed_paste,
            self.synchronized_active,
            true,
        );
        let _ = self.terminal.show_cursor();
    }
}

/// Keyboard protocol flags requested from the terminal.
pub fn keyboard_enhancement_flags() -> KeyboardEnhancementFlags {
    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
}

fn sanitize_title(title: &str) -> String {
    title
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn begin_synchronized<W: Write>(out: &mut W) -> io::Result<()> {
    execute!(out, BeginSynchronizedUpdate)
}

fn end_synchronized<W: Write>(out: &mut W) -> io::Result<()> {
    execute!(out, EndSynchronizedUpdate)
}

fn restore<W: Write>(
    out: &mut W,
    keyboard_enhancement: bool,
    mouse: bool,
    focus: bool,
    bracketed_paste: bool,
    synchronized: bool,
    alternate_screen: bool,
) {
    if synchronized {
        let _ = end_synchronized(out);
    }
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

/// Whether synchronized frame delivery is enabled for this process.
pub fn synchronized_output_enabled() -> bool {
    synchronized_output_enabled_for(std::env::var_os("ANVIL_SYNCHRONIZED_OUTPUT").as_deref())
}

fn synchronized_output_enabled_for(value: Option<&std::ffi::OsStr>) -> bool {
    value != Some(std::ffi::OsStr::new("0"))
}

fn detected_truecolor() -> bool {
    static DETECTED: OnceLock<bool> = OnceLock::new();
    *DETECTED.get_or_init(|| {
        if let Some(value) = std::env::var_os("ANVIL_TRUECOLOR") {
            return value != "0";
        }
        std::env::var("COLORTERM").is_ok_and(|value| {
            value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
        }) || std::env::var("TERM")
            .is_ok_and(|value| value.contains("direct") || value.contains("truecolor"))
    })
}

static TRUECOLOR_OVERRIDE: AtomicU8 = AtomicU8::new(0);

/// Override terminal truecolor detection for the current process.
pub fn set_truecolor_override(value: Option<bool>) {
    let state = match value {
        Some(false) => 1,
        Some(true) => 2,
        None => 0,
    };
    TRUECOLOR_OVERRIDE.store(state, Ordering::Relaxed);
}

/// Whether the terminal advertises 24-bit color.
pub fn supports_truecolor() -> bool {
    match TRUECOLOR_OVERRIDE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => detected_truecolor(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_strips_control_characters() {
        assert_eq!(sanitize_title("a\u{1b}]0;bad\u{7}b"), "a ]0;bad b");
    }

    #[test]
    fn truecolor_override_takes_precedence() {
        set_truecolor_override(Some(true));
        assert!(supports_truecolor());
        set_truecolor_override(Some(false));
        assert!(!supports_truecolor());
        set_truecolor_override(None);
    }

    #[test]
    fn keyboard_flags_do_not_report_all_keys() {
        assert_eq!(
            keyboard_enhancement_flags(),
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        );
    }

    #[test]
    fn synchronized_output_can_be_disabled() {
        assert!(synchronized_output_enabled_for(None));
        assert!(!synchronized_output_enabled_for(Some(
            std::ffi::OsStr::new("0")
        )));
        assert!(synchronized_output_enabled_for(Some(std::ffi::OsStr::new(
            "1"
        ))));
    }

    #[test]
    fn synchronized_commands_bracket_frame_output() {
        let mut output = Vec::new();
        begin_synchronized(&mut output).expect("begin");
        output.extend_from_slice(b"frame");
        end_synchronized(&mut output).expect("end");
        assert_eq!(output, b"\x1b[?2026hframe\x1b[?2026l");
    }
}

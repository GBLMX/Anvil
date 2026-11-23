//! Clipboard access with a system backend and an in-memory test double.
//!
//! The system backend keeps Anvil dependency-free by using the common
//! Wayland/X11 clipboard tools. The trait makes the editor independent of
//! those commands and gives tests a deterministic implementation.

use base64::{Engine as _, engine::general_purpose};
use std::{
    error::Error as StdError,
    fmt,
    io::Write,
    process::{Command, Stdio},
    sync::Mutex,
};

const MAX_OSC52_BYTES: usize = 128 * 1024;

/// Read and write UTF-8 text on the platform clipboard.
pub trait Clipboard: Send + Sync + 'static {
    /// Read text, or `None` when the clipboard has no text value.
    fn get_text(&self) -> Result<Option<String>, ClipboardError>;

    /// Replace the clipboard contents with UTF-8 text.
    fn set_text(&self, text: &str) -> Result<(), ClipboardError>;
}

/// The host clipboard through `wl-copy`/`xclip`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClipboard;

impl SystemClipboard {
    /// Create a stateless system clipboard service.
    pub const fn new() -> Self {
        Self
    }
}

impl SystemClipboard {
    fn set_text_native(&self, text: &str) -> Result<(), ClipboardError> {
        let (program, args): (&'static str, &[&str]) =
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                ("wl-copy", &[])
            } else {
                ("xclip", &["-selection", "clipboard"])
            };
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| ClipboardError::Unavailable(program))?;
        if let Some(mut stdin) = child.stdin.take()
            && let Err(error) = stdin.write_all(text.as_bytes())
        {
            let _ = child.wait();
            return Err(ClipboardError::Backend(error.to_string()));
        }
        if child
            .wait()
            .map_err(|error| ClipboardError::Backend(error.to_string()))?
            .success()
        {
            Ok(())
        } else {
            Err(ClipboardError::CopyFailed)
        }
    }
}

fn osc52_enabled() -> bool {
    osc52_enabled_for(std::env::var_os("ANVIL_OSC52").as_deref())
}

fn osc52_enabled_for(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| value == "1" || value == "true")
}

fn osc52_sequence(text: &str) -> String {
    format!(
        "\x1b]52;c;{}\x07",
        general_purpose::STANDARD.encode(text.as_bytes())
    )
}

fn copy_with_fallback(
    text: &str,
    native: Result<(), ClipboardError>,
    fallback_enabled: bool,
    fallback: impl FnOnce(&str) -> std::io::Result<()>,
) -> Result<(), ClipboardError> {
    if native.is_ok() || !fallback_enabled || text.len() > MAX_OSC52_BYTES {
        return native;
    }
    let native_error = native.expect_err("native clipboard failed");
    match fallback(text) {
        Ok(()) => Ok(()),
        Err(_) => Err(native_error),
    }
}

fn write_osc52(text: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    let path = "/dev/tty";
    #[cfg(windows)]
    let path = "CONOUT$";
    #[cfg(not(any(unix, windows)))]
    {
        let _ = text;
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "OSC 52 requires a controlling terminal",
        ));
    }
    #[cfg(any(unix, windows))]
    {
        let mut terminal = OpenOptions::new().write(true).open(path)?;
        terminal.write_all(osc52_sequence(text).as_bytes())?;
        terminal.flush()
    }
}

#[cfg(any(unix, windows))]
use std::fs::OpenOptions;

impl Clipboard for SystemClipboard {
    fn get_text(&self) -> Result<Option<String>, ClipboardError> {
        let (program, args): (&'static str, &[&str]) =
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                ("wl-paste", &["--no-newline"])
            } else {
                ("xclip", &["-selection", "clipboard", "-o"])
            };
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|_| ClipboardError::Unavailable(program))?;
        if !output.status.success() {
            return Err(ClipboardError::PasteFailed);
        }
        String::from_utf8(output.stdout)
            .map(Some)
            .map_err(|_| ClipboardError::NotUtf8)
    }

    fn set_text(&self, text: &str) -> Result<(), ClipboardError> {
        copy_with_fallback(
            text,
            self.set_text_native(text),
            osc52_enabled(),
            write_osc52,
        )
    }
}

/// An in-memory clipboard for headless tests and non-desktop frontends.
#[derive(Debug, Default)]
pub struct MemoryClipboard {
    text: Mutex<Option<String>>,
}

impl MemoryClipboard {
    /// Create an empty in-memory clipboard.
    pub fn new() -> Self {
        Self::default()
    }

    /// Current text without going through the trait.
    pub fn snapshot(&self) -> Option<String> {
        self.text.lock().ok().and_then(|text| text.clone())
    }
}

impl Clipboard for MemoryClipboard {
    fn get_text(&self) -> Result<Option<String>, ClipboardError> {
        self.text
            .lock()
            .map_err(|_| ClipboardError::Poisoned)
            .map(|text| text.clone())
    }

    fn set_text(&self, text: &str) -> Result<(), ClipboardError> {
        *self.text.lock().map_err(|_| ClipboardError::Poisoned)? = Some(text.to_owned());
        Ok(())
    }
}

/// Clipboard access failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardError {
    /// The required clipboard program is not installed.
    Unavailable(&'static str),
    /// The clipboard contents are not UTF-8.
    NotUtf8,
    /// The copy command exited unsuccessfully.
    CopyFailed,
    /// The paste command exited unsuccessfully.
    PasteFailed,
    /// The command failed while reading or writing its pipe.
    Backend(String),
    /// The in-memory clipboard lock was poisoned after a panic.
    Poisoned,
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(program) => {
                write!(formatter, "system clipboard unavailable ({program})")
            }
            Self::NotUtf8 => formatter.write_str("clipboard is not UTF-8"),
            Self::CopyFailed => formatter.write_str("system clipboard copy failed"),
            Self::PasteFailed => formatter.write_str("system clipboard paste failed"),
            Self::Backend(error) => write!(formatter, "system clipboard: {error}"),
            Self::Poisoned => formatter.write_str("clipboard state is poisoned"),
        }
    }
}

impl StdError for ClipboardError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_clipboard_round_trips_text() {
        let clipboard = MemoryClipboard::new();
        assert_eq!(clipboard.get_text().expect("read"), None);

        clipboard.set_text("hello").expect("write");
        assert_eq!(clipboard.get_text().expect("read"), Some("hello".into()));
        assert_eq!(clipboard.snapshot(), Some("hello".into()));

        clipboard.set_text("replaced").expect("write");
        assert_eq!(clipboard.get_text().expect("read"), Some("replaced".into()));
    }

    #[test]
    fn osc52_requires_explicit_enablement() {
        assert!(!osc52_enabled_for(None));
        assert!(!osc52_enabled_for(Some(std::ffi::OsStr::new("0"))));
        assert!(osc52_enabled_for(Some(std::ffi::OsStr::new("1"))));
        assert!(osc52_enabled_for(Some(std::ffi::OsStr::new("true"))));
    }

    #[test]
    fn osc52_fallback_runs_only_after_native_failure() {
        use std::cell::Cell;

        let called = Cell::new(false);
        copy_with_fallback("text", Ok(()), true, |_| {
            called.set(true);
            Ok(())
        })
        .expect("native success");
        assert!(!called.get());

        let called = Cell::new(false);
        let error = copy_with_fallback(
            "text",
            Err(ClipboardError::Unavailable("xclip")),
            false,
            |_| {
                called.set(true);
                Ok(())
            },
        )
        .expect_err("disabled");
        assert!(!called.get());
        assert_eq!(error, ClipboardError::Unavailable("xclip"));

        let called = Cell::new(false);
        copy_with_fallback(
            "text",
            Err(ClipboardError::Unavailable("xclip")),
            true,
            |_| {
                called.set(true);
                Ok(())
            },
        )
        .expect("fallback");
        assert!(called.get());
    }

    #[test]
    fn osc52_sequence_is_base64_and_terminated() {
        assert_eq!(osc52_sequence("hello"), "\x1b]52;c;aGVsbG8=\x07");
    }

    #[test]
    fn memory_clipboard_preserves_empty_text() {
        let clipboard = MemoryClipboard::new();
        clipboard.set_text("").expect("write");
        assert_eq!(clipboard.get_text().expect("read"), Some(String::new()));
    }
}

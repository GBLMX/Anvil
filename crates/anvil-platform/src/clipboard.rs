//! Clipboard access with a system backend and an in-memory test double.
//!
//! The system backend keeps Anvil dependency-free by using the common
//! Wayland/X11 clipboard tools. The trait makes the editor independent of
//! those commands and gives tests a deterministic implementation.

use std::{
    error::Error as StdError,
    fmt,
    io::Write,
    process::{Command, Stdio},
    sync::Mutex,
};

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
    fn memory_clipboard_preserves_empty_text() {
        let clipboard = MemoryClipboard::new();
        clipboard.set_text("").expect("write");
        assert_eq!(clipboard.get_text().expect("read"), Some(String::new()));
    }
}

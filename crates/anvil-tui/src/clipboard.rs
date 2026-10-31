//! Small system-clipboard adapter using common Wayland/X11 tools.
//! No runtime dependency is added; failures are surfaced to the status line.

use std::io::Write;
use std::process::{Command, Stdio};

pub fn copy(text: &str) -> Result<(), String> {
    let (program, args): (&str, &[&str]) = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
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
        .map_err(|_| format!("system clipboard unavailable ({program})"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    if child.wait().map_err(|error| error.to_string())?.success() {
        Ok(())
    } else {
        Err("system clipboard copy failed".to_owned())
    }
}

pub fn paste() -> Result<String, String> {
    let (program, args): (&str, &[&str]) = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        ("wl-paste", &["--no-newline"])
    } else {
        ("xclip", &["-selection", "clipboard", "-o"])
    };
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|_| format!("system clipboard unavailable ({program})"))?;
    if !output.status.success() {
        return Err("system clipboard paste failed".to_owned());
    }
    String::from_utf8(output.stdout).map_err(|_| "clipboard is not UTF-8".to_owned())
}

//! Explicit workspace PTY session commands.

use super::*;
use crate::pty_terminal::{MAX_TERMINAL_OUTPUT, PtySession, default_shell};

impl Model {
    pub(crate) fn start_terminal(&mut self, shell: &str) {
        if self.read_only {
            self.status = "E: terminal is unavailable in read-only mode".to_owned();
            self.dirty = true;
            return;
        }
        if self.terminal.is_some() {
            self.status = "E: terminal already running".to_owned();
            self.dirty = true;
            return;
        }
        let program = if shell.trim().is_empty() {
            default_shell()
        } else {
            shell.trim().to_owned()
        };
        if !Path::new(&program).is_absolute() {
            self.status = "E: terminal shell must be an absolute path".to_owned();
            self.dirty = true;
            return;
        }
        match PtySession::spawn(
            &program,
            &self.workspace,
            self.viewport_width.max(20),
            self.viewport_height.max(5),
        ) {
            Ok(session) => {
                self.status = format!("terminal: {} pid {}", session.shell(), session.pid());
                self.terminal_output.clear();
                self.terminal = Some(session);
            }
            Err(error) => self.status = format!("E: terminal: {error}"),
        }
        self.dirty = true;
    }

    pub(crate) fn stop_terminal(&mut self) {
        let Some(mut terminal) = self.terminal.take() else {
            self.status = "no terminal".to_owned();
            self.dirty = true;
            return;
        };
        terminal.kill();
        self.status = "terminal stopped".to_owned();
        self.dirty = true;
    }

    pub(crate) fn send_terminal(&mut self, text: &str) {
        let Some(terminal) = self.terminal.as_mut() else {
            self.status = "no terminal".to_owned();
            self.dirty = true;
            return;
        };
        match terminal.write_all(format!("{text}\n").as_bytes()) {
            Ok(()) => {
                self.status = format!("terminal: sent {text}");
            }
            Err(error) => self.status = format!("E: terminal write: {error}"),
        }
        self.dirty = true;
    }

    pub(crate) fn poll_terminal(&mut self) {
        let Some(terminal) = self.terminal.as_mut() else {
            return;
        };
        let exited = terminal.try_wait().ok().flatten();
        match terminal.read_available() {
            Ok(bytes) if !bytes.is_empty() => {
                self.terminal_output.extend_from_slice(&bytes);
                if self.terminal_output.len() > MAX_TERMINAL_OUTPUT {
                    let excess = self.terminal_output.len() - MAX_TERMINAL_OUTPUT;
                    self.terminal_output.drain(..excess);
                }
                self.dirty = true;
            }
            Ok(_) => {}
            Err(error) => {
                self.status = format!("E: terminal read: {error}");
                self.dirty = true;
            }
        }
        if let Some(status) = exited {
            self.terminal = None;
            self.status = format!("terminal exited: {status}");
            self.dirty = true;
        }
    }

    pub(crate) fn show_terminal_output(&mut self) {
        if self.terminal_output.is_empty() {
            self.status = "no terminal output".to_owned();
            self.dirty = true;
            return;
        }
        let output = crate::pty_terminal::sanitize_terminal_output(&self.terminal_output);
        self.show_read_only(&output, "terminal output: :q returns", "terminal");
    }

    pub(crate) fn resize_terminal(&self) {
        if let Some(terminal) = self.terminal.as_ref() {
            let _ = terminal.resize(self.viewport_width.max(20), self.viewport_height.max(5));
        }
    }
}

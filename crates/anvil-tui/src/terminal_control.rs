//! Explicit workspace PTY session commands.

use super::*;
use crate::pty_terminal::{MAX_TERMINAL_OUTPUT, PtySession, default_shell};
use crossterm::event::{KeyCode, KeyModifiers};

impl Model {
    pub(crate) fn start_terminal(&mut self, shell: &str) {
        if self.read_only {
            self.status = "E: terminal is unavailable in read-only mode".to_owned();
            self.dirty = true;
            return;
        }
        if self.terminal.is_some() {
            self.terminal_panel = true;
            self.status = "terminal: panel opened".to_owned();
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
                self.terminal_panel = true;
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
        self.terminal_panel = false;
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
            self.terminal_panel = false;
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

impl Model {
    pub(crate) fn toggle_terminal_panel(&mut self) {
        if self.terminal.is_none() {
            self.status = "no terminal".to_owned();
            self.dirty = true;
            return;
        }
        self.terminal_panel = !self.terminal_panel;
        self.dirty = true;
    }
    pub(crate) fn send_terminal_text(&mut self, text: &str) {
        self.write_terminal_bytes(text.as_bytes());
    }

    fn write_terminal_bytes(&mut self, bytes: &[u8]) {
        let Some(terminal) = self.terminal.as_mut() else {
            return;
        };
        if let Err(error) = terminal.write_all(bytes) {
            self.status = format!("E: terminal write: {error}");
            self.dirty = true;
        }
    }

    pub(crate) fn send_terminal_key(&mut self, key: KeyEvent) -> bool {
        if !self.terminal_panel {
            return false;
        }
        if key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.toggle_terminal_panel();
            return true;
        }
        if let Some(bytes) = terminal_key_bytes(key) {
            self.write_terminal_bytes(&bytes);
        }
        true
    }
}

fn terminal_key_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    if key.modifiers.contains(KeyModifiers::ALT) {
        bytes.push(0x1b);
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(character) = key.code {
            if character.is_ascii_alphabetic() {
                bytes.push(character.to_ascii_lowercase() as u8 - b'a' + 1);
                return Some(bytes);
            }
            if character == '[' {
                bytes.push(0x1b);
                return Some(bytes);
            }
        }
    }
    match key.code {
        KeyCode::Char(character) => {
            let mut buffer = [0u8; 4];
            bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
        }
        KeyCode::Enter => bytes.push(b'\r'),
        KeyCode::Tab => bytes.push(b'\t'),
        KeyCode::Backspace => bytes.push(0x7f),
        KeyCode::Esc => bytes.push(0x1b),
        KeyCode::Up => bytes.extend_from_slice(b"\x1b[A"),
        KeyCode::Down => bytes.extend_from_slice(b"\x1b[B"),
        KeyCode::Right => bytes.extend_from_slice(b"\x1b[C"),
        KeyCode::Left => bytes.extend_from_slice(b"\x1b[D"),
        KeyCode::Home => bytes.extend_from_slice(b"\x1b[H"),
        KeyCode::End => bytes.extend_from_slice(b"\x1b[F"),
        KeyCode::Delete => bytes.extend_from_slice(b"\x1b[3~"),
        KeyCode::PageUp => bytes.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => bytes.extend_from_slice(b"\x1b[6~"),
        KeyCode::Insert => bytes.extend_from_slice(b"\x1b[2~"),
        _ => {}
    }
    if bytes.is_empty() { None } else { Some(bytes) }
}

pub(crate) fn render_panel(model: &Model, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    if !model.terminal_panel {
        return;
    }
    let height = (area.height / 3).max(6).min(area.height.saturating_sub(2));
    if height < 3 {
        return;
    }
    let popup = ratatui::layout::Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(height),
        width: area.width,
        height,
    };
    let text = crate::pty_terminal::sanitize_terminal_output(&model.terminal_output);
    let visible = height.saturating_sub(2) as usize;
    let mut lines: Vec<ratatui::text::Line<'static>> = text
        .lines()
        .map(|line| ratatui::text::Line::raw(line.to_owned()))
        .collect();
    if lines.len() > visible {
        lines.drain(..lines.len() - visible);
    }
    if lines.is_empty() {
        lines.push(ratatui::text::Line::styled(
            "terminal",
            crate::theme::panel().fg(crate::theme::muted()),
        ));
    }
    frame.render_widget(
        ratatui::widgets::Paragraph::new(lines)
            .style(crate::theme::panel())
            .block(crate::render::panel_block("Terminal (Ctrl+T closes)")),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    #[test]
    fn encodes_terminal_keys() {
        assert_eq!(
            terminal_key_bytes(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(vec![3])
        );
        assert_eq!(
            terminal_key_bytes(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(vec![b'\r'])
        );
        assert_eq!(
            terminal_key_bytes(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE)),
            None
        );
    }
}

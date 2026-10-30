//! Minimal ratatui frontend. TUI-1 adds insert mode, cursor-independent
//! viewport tracking, and `:w` saving. Prefix commands and gateway effects
//! remain later slices.

use std::{
    fs,
    io::{self, Stdout},
    path::{Path, PathBuf},
};

use anvil_api::EditorView;
use anvil_core::TextBuffer;
use anvil_editor::EditorState;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

/// The small state machine needed by TUI-1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    /// Navigation, insert entry, command entry.
    Normal,
    /// Text insertion at the cursor.
    Insert,
    /// Ex-style command line.
    CommandLine { buffer: String },
}

/// TUI state.
pub struct Model {
    /// Editor state and its shared text/index.
    pub editor: EditorState,
    /// Optional source path.
    pub path: Option<PathBuf>,
    /// Cursor byte offset.
    pub cursor: usize,
    /// First visible line, independent of the cursor.
    pub scroll: usize,
    /// Input state.
    pub input_mode: InputMode,
    /// Set by `q` / `:q`.
    pub quit: bool,
    /// Whether the file has unsaved changes.
    pub modified: bool,
    /// Whether the next loop iteration should render.
    pub dirty: bool,
    /// Short status line.
    pub status: String,
}

impl Model {
    /// Builds a model from text and an optional source path.
    pub fn new(editor: EditorState, path: Option<PathBuf>) -> Self {
        Self {
            editor,
            path,
            cursor: 0,
            scroll: 0,
            input_mode: InputMode::Normal,
            quit: false,
            modified: false,
            dirty: true,
            status: String::new(),
        }
    }

    /// Builds a model by reading a UTF-8 file.
    ///
    /// Invalid UTF-8 is rejected instead of being lossily replaced, because
    /// saving a lossy view would silently corrupt the original bytes.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let text = fs::read_to_string(&path)?;
        Ok(Self::new(EditorState::from_text(&text), Some(path)))
    }

    /// Moves the cursor one line while keeping the viewport independent.
    pub fn move_line(&mut self, delta: isize) {
        let Some(position) = self
            .editor
            .index()
            .position(self.editor.text(), self.cursor)
        else {
            return;
        };
        let target = position.line as isize + delta;
        if target < 0 {
            return;
        }
        let Some(offset) = self.editor.index().offset_at_display_column(
            self.editor.text(),
            target as usize,
            position.display_column,
        ) else {
            return;
        };
        self.cursor = offset;
        self.dirty = true;
    }

    /// Moves the cursor one character boundary.
    pub fn move_char(&mut self, delta: isize) {
        let mut offset = self.cursor;
        if delta < 0 {
            if offset == 0 {
                return;
            }
            offset -= 1;
            while offset > 0 && !self.editor.text().is_char_boundary(offset) {
                offset -= 1;
            }
        } else if offset < self.editor.text().len_bytes() {
            offset += 1;
            while offset < self.editor.text().len_bytes()
                && !self.editor.text().is_char_boundary(offset)
            {
                offset += 1;
            }
        }
        self.cursor = offset;
        self.dirty = true;
    }

    /// Moves to the start of the cursor line.
    pub fn move_home(&mut self) {
        if let Some(line) = self.editor.index().offset_to_line(self.cursor) {
            self.cursor = line.start;
            self.dirty = true;
        }
    }

    /// Moves to the end of the cursor line.
    pub fn move_end(&mut self) {
        if let Some(line) = self.editor.index().offset_to_line(self.cursor) {
            self.cursor = line.end;
            self.dirty = true;
        }
    }

    /// Keeps the cursor inside the viewport.
    pub fn ensure_cursor_visible(&mut self, height: u16) {
        let Some(position) = self
            .editor
            .index()
            .position(self.editor.text(), self.cursor)
        else {
            return;
        };
        let line = position.line;
        let height = usize::from(height.max(1));
        if line < self.scroll {
            self.scroll = line;
        } else if line >= self.scroll + height {
            self.scroll = line + 1 - height;
        }
    }

    /// Inserts text at the cursor.
    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.editor.set_selection_bytes(self.cursor, self.cursor);
        self.editor.insert(self.cursor, text);
        self.cursor += text.len();
        self.modified = true;
        self.dirty = true;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut start = self.cursor - 1;
        while start > 0 && !self.editor.text().is_char_boundary(start) {
            start -= 1;
        }
        self.editor.set_selection_bytes(start, self.cursor);
        self.editor.delete(start..self.cursor);
        self.cursor = start;
        self.modified = true;
        self.dirty = true;
    }

    fn delete_forward(&mut self) {
        if self.cursor >= self.editor.text().len_bytes() {
            return;
        }
        let mut end = self.cursor + 1;
        while end < self.editor.text().len_bytes() && !self.editor.text().is_char_boundary(end) {
            end += 1;
        }
        self.editor.set_selection_bytes(self.cursor, end);
        self.editor.delete(self.cursor..end);
        self.modified = true;
        self.dirty = true;
    }

    fn save(&mut self) {
        let Some(path) = self.path.as_ref() else {
            self.status = "E32: no file name".to_owned();
            self.dirty = true;
            return;
        };
        let text = self.editor.text().slice(0..self.editor.text().len_bytes());
        match fs::write(path, text) {
            Ok(()) => {
                self.modified = false;
                self.status = format!("\"{}\" written", path.display());
            }
            Err(error) => {
                self.status = format!("E: {error}");
            }
        }
        self.dirty = true;
    }

    fn execute_command(&mut self, command: &str) {
        match command {
            "w" => self.save(),
            "q" => self.quit = true,
            "wq" => {
                self.save();
                if !self.modified {
                    self.quit = true;
                }
            }
            _ => {
                self.status = format!("E: unknown command: {command}");
                self.dirty = true;
            }
        }
    }

    fn visible_lines(&self, height: u16) -> Vec<Line<'_>> {
        let index = self.editor.index();
        let text = self.editor.text();
        let total = index.len_lines();
        let end = (self.scroll + usize::from(height)).min(total);
        let mut lines = Vec::with_capacity(end.saturating_sub(self.scroll));
        for line_number in self.scroll..end {
            let Some(start) = index.line_to_offset(line_number) else {
                break;
            };
            let Some(line) = index.offset_to_line(start) else {
                break;
            };
            let spans = text
                .chunks(start..line.end)
                .map(Span::raw)
                .collect::<Vec<_>>();
            lines.push(Line::from(if spans.is_empty() {
                vec![Span::raw("")]
            } else {
                spans
            }));
        }
        if lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines
    }
}

/// Renders the current viewport and cursor.
pub fn view(model: &Model, frame: &mut Frame) {
    let area = frame.area();
    let editor_view: EditorView = model.editor.snapshot().view();
    let title = model
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "[no file]".to_owned());
    let debug_revision = std::env::var_os("ANVIL_DEBUG_REV").is_some();
    let title = if debug_revision {
        format!("{title} · rev {}", editor_view.revision)
    } else {
        title
    };
    let status = match &model.input_mode {
        InputMode::CommandLine { buffer } => format!(":{buffer}"),
        InputMode::Insert => "-- INSERT --".to_owned(),
        InputMode::Normal => {
            let position = model
                .editor
                .index()
                .position(model.editor.text(), model.cursor);
            match position {
                Some(position) => format!(
                    " Ln {}, Col {} ",
                    position.line + 1,
                    position.grapheme_column + 1
                ),
                None => " Ln 1, Col 1 ".to_owned(),
            }
        }
    };
    let block = Block::bordered()
        .title(title)
        .title_bottom(Line::raw(status));
    frame.render_widget(block, area);
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    frame.render_widget(Paragraph::new(model.visible_lines(inner.height)), inner);
    if let Some(position) = model
        .editor
        .index()
        .position(model.editor.text(), model.cursor)
    {
        let x = inner.x + position.display_column as u16;
        let y = inner.y + position.line.saturating_sub(model.scroll) as u16;
        if x < inner.right() && y < inner.bottom() {
            frame.set_cursor_position((x, y));
        }
    }
}

/// Runs the minimal terminal loop.
pub fn run(path: Option<PathBuf>) -> io::Result<()> {
    let mut model = match path {
        Some(path) => Model::open(path)?,
        None => Model::new(EditorState::from_text(""), None),
    };
    let mut terminal = TerminalSession::start()?;
    loop {
        let size = terminal.terminal.size()?;
        model.ensure_cursor_visible(size.height.saturating_sub(2));
        if model.dirty {
            terminal.terminal.draw(|frame| view(&model, frame))?;
            model.dirty = false;
        }
        handle_event(&mut model, event::read()?);
        if model.quit {
            return Ok(());
        }
    }
}

fn handle_event(model: &mut Model, event: Event) {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => handle_key(model, key),
        Event::Paste(text) => {
            if let InputMode::Insert = model.input_mode {
                model.insert_text(&text);
            } else if let InputMode::CommandLine { buffer } = &mut model.input_mode {
                buffer.push_str(&text);
                model.dirty = true;
            }
        }
        Event::Resize(_, _) => model.dirty = true,
        _ => {}
    }
}

fn handle_key(model: &mut Model, key: KeyEvent) {
    let control = key
        .modifiers
        .contains(crossterm::event::KeyModifiers::CONTROL);
    match &mut model.input_mode {
        InputMode::CommandLine { buffer } => match key.code {
            KeyCode::Esc => {
                model.input_mode = InputMode::Normal;
                model.dirty = true;
            }
            KeyCode::Enter => {
                let command = std::mem::take(buffer);
                model.input_mode = InputMode::Normal;
                model.execute_command(command.trim());
            }
            KeyCode::Backspace => {
                buffer.pop();
                model.dirty = true;
            }
            KeyCode::Char(character) if !control => {
                buffer.push(character);
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::Insert => match key.code {
            KeyCode::Esc => {
                model.input_mode = InputMode::Normal;
                model.dirty = true;
            }
            KeyCode::Enter => model.insert_text("\n"),
            KeyCode::Backspace => model.backspace(),
            KeyCode::Delete => model.delete_forward(),
            KeyCode::Left => model.move_char(-1),
            KeyCode::Right => model.move_char(1),
            KeyCode::Up => model.move_line(-1),
            KeyCode::Down => model.move_line(1),
            KeyCode::Home => model.move_home(),
            KeyCode::End => model.move_end(),
            KeyCode::Char(character) if !control => model.insert_text(&character.to_string()),
            _ => {}
        },
        InputMode::Normal => match key.code {
            KeyCode::Char('q') => model.quit = true,
            KeyCode::Char('i') => {
                model.input_mode = InputMode::Insert;
                model.dirty = true;
            }
            KeyCode::Char(':') => {
                model.input_mode = InputMode::CommandLine {
                    buffer: String::new(),
                };
                model.dirty = true;
            }
            KeyCode::Char('h') | KeyCode::Left => model.move_char(-1),
            KeyCode::Char('l') | KeyCode::Right => model.move_char(1),
            KeyCode::Char('j') | KeyCode::Down => model.move_line(1),
            KeyCode::Char('k') | KeyCode::Up => model.move_line(-1),
            KeyCode::Home => model.move_home(),
            KeyCode::End => model.move_end(),
            _ => {}
        },
    }
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn start() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        let backend = CrosstermBackend::new(stdout);
        match Terminal::new(backend) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                Err(error)
            }
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use ratatui::{Terminal, backend::TestBackend};

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn renders_visible_lines() {
        let model = Model::new(EditorState::from_text("alpha\nbeta\ngamma"), None);
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| view(&model, frame)).expect("draw");
        let buffer = terminal.backend().buffer();
        let rendered = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("alpha"));
        assert!(rendered.contains("beta"));
        assert!(rendered.contains("gamma"));
    }

    #[test]
    fn insert_mode_modifies_piece_table() {
        let mut model = Model::new(EditorState::from_text("ac"), None);
        handle_event(&mut model, key(KeyCode::Char('i')));
        handle_event(&mut model, key(KeyCode::Char('b')));
        handle_event(&mut model, key(KeyCode::Esc));
        assert_eq!(model.editor.text().slice(0..3), "bac");
        assert!(model.modified);
        assert_eq!(model.input_mode, InputMode::Normal);
    }

    #[test]
    fn command_line_saves() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("file.txt");
        fs::write(&path, "old").expect("write");
        let mut model = Model::open(&path).expect("open");
        handle_event(&mut model, key(KeyCode::Char('i')));
        handle_event(&mut model, key(KeyCode::Char('n')));
        handle_event(&mut model, key(KeyCode::Esc));
        handle_event(&mut model, key(KeyCode::Char(':')));
        handle_event(&mut model, key(KeyCode::Char('w')));
        handle_event(&mut model, key(KeyCode::Enter));
        assert!(!model.modified);
        assert_eq!(fs::read_to_string(path).expect("read"), "nold");
    }

    #[test]
    fn cursor_and_viewport_are_independent() {
        let mut model = Model::new(EditorState::from_text("a\nb\nc\nd"), None);
        model.move_line(3);
        model.ensure_cursor_visible(2);
        assert_eq!(model.scroll, 2);
        model.move_line(-1);
        model.ensure_cursor_visible(2);
        assert_eq!(model.scroll, 2);
    }

    #[test]
    fn idle_events_do_not_request_a_redraw() {
        let mut model = Model::new(EditorState::from_text("a\nb"), None);
        model.dirty = false;
        handle_event(&mut model, Event::FocusGained);
        assert!(!model.dirty);
        handle_event(&mut model, key(KeyCode::Char('x')));
        assert!(!model.dirty);
        handle_event(&mut model, key(KeyCode::Char('j')));
        assert!(model.dirty);
    }

    #[test]
    fn invalid_utf8_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("binary.bin");
        fs::write(&path, [0xff, 0xfe]).expect("write");
        assert!(Model::open(path).is_err());
    }

    #[test]
    fn paste_only_inserts_in_edit_modes() {
        let mut model = Model::new(EditorState::from_text("a"), None);
        model.dirty = false;
        handle_event(&mut model, Event::Paste("x".to_owned()));
        assert_eq!(model.editor.text().slice(0..1), "a");
        assert!(!model.dirty);

        handle_event(&mut model, key(KeyCode::Char('i')));
        handle_event(&mut model, Event::Paste("b".to_owned()));
        assert_eq!(model.editor.text().slice(0..2), "ba");
    }
}

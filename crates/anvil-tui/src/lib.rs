//! Minimal ratatui frontend. TUI-1 adds insert mode, cursor-independent
//! viewport tracking, and `:w` saving. Prefix commands and gateway effects
//! remain later slices.

use std::{
    fs,
    io::{self, Stdout},
    path::{Path, PathBuf},
};

use anvil_api::EditorView;
use anvil_core::{TextBuffer, TextDocument};
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
    /// Incremental substring search.
    Search { query: String },
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
    /// First visible display column.
    pub hscroll: usize,
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
    /// Pending normal-mode operator (`d` or `y`).
    pending: Option<char>,
    /// Last substring search.
    search_query: String,
}

impl Model {
    /// Builds a model from text and an optional source path.
    pub fn new(editor: EditorState, path: Option<PathBuf>) -> Self {
        Self {
            editor,
            path,
            cursor: 0,
            scroll: 0,
            hscroll: 0,
            input_mode: InputMode::Normal,
            quit: false,
            modified: false,
            dirty: true,
            status: String::new(),
            pending: None,
            search_query: String::new(),
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

    /// Moves the cursor one grapheme-cluster boundary.
    pub fn move_char(&mut self, delta: isize) {
        let next = if delta < 0 {
            self.editor.previous_grapheme_boundary(self.cursor)
        } else {
            self.editor.next_grapheme_boundary(self.cursor)
        };
        if let Some(offset) = next {
            self.cursor = offset;
            self.dirty = true;
        }
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
    pub fn ensure_cursor_visible(&mut self, height: u16, width: u16) {
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
        let column = position.display_column;
        let width = usize::from(width.max(1));
        if column < self.hscroll {
            self.hscroll = column;
        } else if column >= self.hscroll.saturating_add(width) {
            self.hscroll = column + 1 - width;
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

    fn sync_cursor_from_selection(&mut self) {
        self.cursor = self
            .editor
            .selection_bytes()
            .first()
            .map(|(_, head)| *head)
            .unwrap_or(0)
            .min(self.editor.text().len_bytes());
    }

    fn undo(&mut self) {
        if self.editor.undo() {
            self.sync_cursor_from_selection();
            self.modified = true;
        } else {
            self.status = "Already at oldest change".to_owned();
        }
        self.dirty = true;
    }

    fn redo(&mut self) {
        if self.editor.redo() {
            self.sync_cursor_from_selection();
            self.modified = true;
        } else {
            self.status = "Already at newest change".to_owned();
        }
        self.dirty = true;
    }

    fn delete_line(&mut self) {
        let Some(range) = self.editor.line_range_at(self.cursor) else {
            self.status = "E: no line".to_owned();
            self.dirty = true;
            return;
        };
        self.editor.set_selection_bytes(range.start, range.end);
        self.cursor = range.start;
        self.editor.delete(range);
        self.sync_cursor_from_selection();
        self.modified = true;
        self.dirty = true;
    }

    fn yank_line(&mut self) {
        let Some(range) = self.editor.line_range_at(self.cursor) else {
            self.status = "E: no line".to_owned();
            self.dirty = true;
            return;
        };
        let cursor = self.cursor;
        self.editor.set_selection_bytes(range.start, range.end);
        if self.editor.yank(range) {
            self.status = "1 line yanked".to_owned();
        } else {
            self.status = "E: yank failed".to_owned();
        }
        let _ = self.editor.set_selection_bytes(cursor, cursor);
        self.cursor = cursor;
        self.dirty = true;
    }

    /// Inserts the unnamed register at the cursor.
    fn put_register(&mut self) {
        if self.editor.put(self.cursor).is_some() {
            self.sync_cursor_from_selection();
            self.modified = true;
        } else {
            self.status = "E: register empty".to_owned();
        }
        self.dirty = true;
    }

    fn request_quit(&mut self) {
        if self.modified {
            self.status = "No write since last change (:q! to override)".to_owned();
            self.dirty = true;
        } else {
            self.quit = true;
        }
    }

    fn jump_to(&mut self, offset: usize) {
        self.cursor = offset;
        self.editor.set_selection_bytes(offset, offset);
        self.dirty = true;
    }

    fn execute_search(&mut self, query: String) {
        if query.is_empty() {
            if self.search_query.is_empty() {
                self.status = "E: no previous search".to_owned();
                self.dirty = true;
            } else {
                self.search_next(true);
            }
            return;
        }
        self.search_query = query;
        let matches = find_matches(self.editor.text(), &self.search_query);
        if matches.is_empty() {
            self.status = format!("E: pattern not found: {}", self.search_query);
            self.dirty = true;
            return;
        }
        let target = matches
            .iter()
            .copied()
            .find(|&offset| offset >= self.cursor)
            .unwrap_or(matches[0]);
        self.jump_to(target);
        self.status = format!("/{}", self.search_query);
    }

    fn search_next(&mut self, forward: bool) {
        if self.search_query.is_empty() {
            self.status = "E: no previous search".to_owned();
            self.dirty = true;
            return;
        }
        let matches = find_matches(self.editor.text(), &self.search_query);
        if matches.is_empty() {
            self.status = format!("E: pattern not found: {}", self.search_query);
            self.dirty = true;
            return;
        }
        let target = if forward {
            matches
                .iter()
                .copied()
                .find(|&offset| offset > self.cursor)
                .unwrap_or(matches[0])
        } else {
            matches
                .iter()
                .rev()
                .copied()
                .find(|&offset| offset < self.cursor)
                .unwrap_or(matches[matches.len() - 1])
        };
        self.jump_to(target);
        self.status = format!("/{}", self.search_query);
    }

    fn append(&mut self) {
        if let Some(line) = self.editor.index().offset_to_line(self.cursor)
            && self.cursor < line.end
        {
            self.move_char(1);
        }
        self.input_mode = InputMode::Insert;
        self.dirty = true;
    }

    fn append_line_end(&mut self) {
        if let Some(line) = self.editor.index().offset_to_line(self.cursor) {
            self.cursor = line.end;
        }
        self.input_mode = InputMode::Insert;
        self.dirty = true;
    }

    fn open_below(&mut self) {
        if let Some(line) = self.editor.index().offset_to_line(self.cursor) {
            self.cursor = line.end;
        }
        self.insert_text("\n");
        self.input_mode = InputMode::Insert;
    }

    fn open_above(&mut self) {
        let start = self
            .editor
            .index()
            .offset_to_line(self.cursor)
            .map_or(0, |line| line.start);
        self.cursor = start;
        self.insert_text("\n");
        self.cursor = start;
        self.input_mode = InputMode::Insert;
    }

    fn backspace(&mut self) {
        let Some(start) = self.editor.previous_grapheme_boundary(self.cursor) else {
            return;
        };
        if start == self.cursor {
            return;
        }
        self.editor.set_selection_bytes(start, self.cursor);
        self.editor.delete(start..self.cursor);
        self.cursor = start;
        self.modified = true;
        self.dirty = true;
    }

    fn delete_forward(&mut self) {
        let Some(end) = self.editor.next_grapheme_boundary(self.cursor) else {
            return;
        };
        if end == self.cursor {
            return;
        }
        self.editor.set_selection_bytes(self.cursor, end);
        self.editor.delete(self.cursor..end);
        self.modified = true;
        self.dirty = true;
    }

    fn save_to(&mut self, path: PathBuf) -> bool {
        let text = self.editor.text().slice(0..self.editor.text().len_bytes());
        let display = path.display().to_string();
        match fs::write(&path, text) {
            Ok(()) => {
                self.path = Some(path);
                self.modified = false;
                self.status = format!("\"{display}\" written");
                self.dirty = true;
                true
            }
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                false
            }
        }
    }

    fn save(&mut self) -> bool {
        let Some(path) = self.path.clone() else {
            self.status = "E32: no file name".to_owned();
            self.dirty = true;
            return false;
        };
        self.save_to(path)
    }

    fn save_as(&mut self, path: &str) -> bool {
        self.save_to(PathBuf::from(path))
    }

    fn execute_command(&mut self, command: &str) {
        let mut parts = command.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or_default();
        let argument = parts.next().unwrap_or_default().trim();
        match (name, argument) {
            ("w", "") => {
                self.save();
            }
            ("w", path) => {
                self.save_as(path);
            }
            ("q", "") => self.request_quit(),
            ("q!", "") => self.quit = true,
            ("wq", "") => {
                if self.save() {
                    self.quit = true;
                }
            }
            ("wq", path) => {
                if self.save_as(path) {
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

fn find_matches(text: &TextDocument, needle: &str) -> Vec<usize> {
    let needle = needle.as_bytes();
    if needle.is_empty() || needle.len() > text.len_bytes() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    let mut carry = Vec::new();
    let mut processed: usize = 0;
    for chunk in text.chunks(0..text.len_bytes()) {
        if chunk.is_empty() {
            continue;
        }
        let mut window = Vec::with_capacity(carry.len() + chunk.len());
        window.extend_from_slice(&carry);
        window.extend_from_slice(chunk.as_bytes());
        let window_start = processed.saturating_sub(carry.len());
        let mut from = 0;
        while from + needle.len() <= window.len() {
            let Some(local) = window[from..]
                .windows(needle.len())
                .position(|candidate| candidate == needle)
            else {
                break;
            };
            let at = from + local;
            matches.push(window_start + at);
            from = at + 1;
        }
        let keep = needle.len().saturating_sub(1).min(window.len());
        carry.clear();
        carry.extend_from_slice(&window[window.len() - keep..]);
        processed += chunk.len();
    }
    matches
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
        InputMode::Search { query } => format!("/{query}"),
        InputMode::Insert => "-- INSERT --".to_owned(),
        InputMode::Normal => {
            let position = model
                .editor
                .index()
                .position(model.editor.text(), model.cursor);
            let location = match position {
                Some(position) => format!(
                    " Ln {}, Col {} ",
                    position.line + 1,
                    position.grapheme_column + 1
                ),
                None => " Ln 1, Col 1 ".to_owned(),
            };
            if model.status.is_empty() {
                location
            } else {
                format!("{} ·{location}", model.status)
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
    frame.render_widget(
        Paragraph::new(model.visible_lines(inner.height))
            .scroll((0, u16::try_from(model.hscroll).unwrap_or(u16::MAX))),
        inner,
    );
    if let Some(position) = model
        .editor
        .index()
        .position(model.editor.text(), model.cursor)
    {
        let x = inner.x + position.display_column.saturating_sub(model.hscroll) as u16;
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
        model.ensure_cursor_visible(size.height.saturating_sub(2), size.width.saturating_sub(2));
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
            model.pending = None;
            if let InputMode::Insert = model.input_mode {
                model.insert_text(&text);
            } else if let InputMode::CommandLine { buffer } = &mut model.input_mode {
                buffer.push_str(&text);
                model.dirty = true;
            } else if let InputMode::Search { query } = &mut model.input_mode {
                query.push_str(&text);
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

    if let Some(pending) = model.pending.take() {
        model.dirty = true;
        match (pending, key.code) {
            ('d', KeyCode::Char('d')) => model.delete_line(),
            ('y', KeyCode::Char('y')) => model.yank_line(),
            _ => {}
        }
        return;
    }
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
        InputMode::Search { query } => match key.code {
            KeyCode::Esc => {
                model.input_mode = InputMode::Normal;
                model.dirty = true;
            }
            KeyCode::Enter => {
                let query = std::mem::take(query);
                model.input_mode = InputMode::Normal;
                model.execute_search(query);
            }
            KeyCode::Backspace => {
                query.pop();
                model.dirty = true;
            }
            KeyCode::Char(character) if !control => {
                query.push(character);
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
            KeyCode::Char('q') => model.request_quit(),
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
            KeyCode::Char('/') => {
                model.input_mode = InputMode::Search {
                    query: String::new(),
                };
                model.dirty = true;
            }
            KeyCode::Char('a') if !control => model.append(),
            KeyCode::Char('A') if !control => model.append_line_end(),
            KeyCode::Char('o') if !control => model.open_below(),
            KeyCode::Char('O') if !control => model.open_above(),
            KeyCode::Char('n') if !control => model.search_next(true),
            KeyCode::Char('N') if !control => model.search_next(false),
            KeyCode::Char('d') if !control => model.pending = Some('d'),
            KeyCode::Char('y') if !control => model.pending = Some('y'),
            KeyCode::Char('x') if !control => model.delete_forward(),
            KeyCode::Char('p') if !control => model.put_register(),
            KeyCode::Char('u') if !control => model.undo(),
            KeyCode::Char('r') if control => model.redo(),
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

    fn ctrl(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
    }

    fn type_text(model: &mut Model, text: &str) {
        for character in text.chars() {
            handle_event(model, key(KeyCode::Char(character)));
        }
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
        model.ensure_cursor_visible(2, 80);
        assert_eq!(model.scroll, 2);
        model.move_line(-1);
        model.ensure_cursor_visible(2, 80);
        assert_eq!(model.scroll, 2);
    }

    #[test]
    fn horizontal_scroll_follows_the_cursor() {
        let mut model = Model::new(EditorState::from_text("0123456789"), None);
        model.cursor = 8;
        model.ensure_cursor_visible(1, 4);
        assert_eq!(model.hscroll, 5);
        model.cursor = 0;
        model.ensure_cursor_visible(1, 4);
        assert_eq!(model.hscroll, 0);
    }

    #[test]
    fn horizontal_scroll_uses_display_cells() {
        let mut model = Model::new(EditorState::from_text("中中中中中"), None);
        model.cursor = model.editor.text().len_bytes();
        model.ensure_cursor_visible(1, 3);
        assert_eq!(model.hscroll, 8);
    }

    #[test]
    fn status_column_uses_grapheme_index() {
        let mut model = Model::new(EditorState::from_text("a👨‍👩‍👧‍👦b"), None);
        model.cursor = "a👨‍👩‍👧‍👦".len();
        let backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| view(&model, frame)).expect("draw");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Col 3"));
        assert!(!rendered.contains("Col 26"));
    }

    #[test]
    fn idle_events_do_not_request_a_redraw() {
        let mut model = Model::new(EditorState::from_text("a\nb"), None);
        model.dirty = false;
        handle_event(&mut model, Event::FocusGained);
        assert!(!model.dirty);
        handle_event(&mut model, key(KeyCode::Char('z')));
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

    #[test]
    fn quit_guard_requires_force_or_write() {
        let mut model = Model::new(EditorState::from_text("a"), None);
        handle_event(&mut model, key(KeyCode::Char('i')));
        handle_event(&mut model, key(KeyCode::Char('b')));
        handle_event(&mut model, key(KeyCode::Esc));

        handle_event(&mut model, key(KeyCode::Char('q')));
        assert!(!model.quit);
        assert!(model.status.contains("No write since last change"));

        handle_event(&mut model, key(KeyCode::Char(':')));
        handle_event(&mut model, key(KeyCode::Char('q')));
        handle_event(&mut model, key(KeyCode::Char('!')));
        handle_event(&mut model, key(KeyCode::Enter));
        assert!(model.quit);
    }

    #[test]
    fn undo_and_redo_are_reachable_from_normal_mode() {
        let mut model = Model::new(EditorState::from_text("a"), None);
        handle_event(&mut model, key(KeyCode::Char('i')));
        handle_event(&mut model, key(KeyCode::Char('X')));
        handle_event(&mut model, key(KeyCode::Esc));
        assert_eq!(model.editor.text().slice(0..2), "Xa");

        handle_event(&mut model, key(KeyCode::Char('u')));
        assert_eq!(model.editor.text().slice(0..1), "a");
        assert_eq!(model.cursor, 0);

        handle_event(&mut model, ctrl(KeyCode::Char('r')));
        assert_eq!(model.editor.text().slice(0..2), "Xa");
        assert_eq!(model.cursor, 1);
    }

    #[test]
    fn dd_deletes_current_line_and_keeps_line_start() {
        let mut model = Model::new(EditorState::from_text("a\nb\nc"), None);
        model.move_line(1);
        handle_event(&mut model, key(KeyCode::Char('d')));
        handle_event(&mut model, key(KeyCode::Char('d')));
        assert_eq!(model.editor.text().slice(0..3), "a\nc");
        assert_eq!(model.cursor, 2);
        assert!(model.modified);
    }

    #[test]
    fn yy_and_p_use_the_unnamed_register() {
        let mut model = Model::new(EditorState::from_text("a\nb"), None);
        model.move_line(1);
        handle_event(&mut model, key(KeyCode::Char('y')));
        handle_event(&mut model, key(KeyCode::Char('y')));
        assert_eq!(model.editor.yank_text(), "b");
        assert_eq!(model.cursor, 2);

        model.move_end();
        handle_event(&mut model, key(KeyCode::Char('p')));
        assert_eq!(model.editor.text().slice(0..4), "a\nbb");
        assert_eq!(model.cursor, 4);
    }

    #[test]
    fn x_deletes_forward() {
        let mut model = Model::new(EditorState::from_text("ab"), None);
        handle_event(&mut model, key(KeyCode::Char('x')));
        assert_eq!(model.editor.text().slice(0..1), "b");
        assert_eq!(model.cursor, 0);
        assert!(model.modified);
    }

    #[test]
    fn wq_without_a_path_does_not_quit() {
        let mut model = Model::new(EditorState::from_text("a"), None);
        for code in [
            KeyCode::Char(':'),
            KeyCode::Char('w'),
            KeyCode::Char('q'),
            KeyCode::Enter,
        ] {
            handle_event(&mut model, key(code));
        }
        assert!(!model.quit);
        assert!(model.status.contains("no file name"));
    }

    #[test]
    fn search_cycles_and_crosses_chunk_boundaries() {
        let mut model = Model::new(EditorState::from_text("foo bar foo"), None);
        handle_event(&mut model, key(KeyCode::Char('/')));
        type_text(&mut model, "foo");
        handle_event(&mut model, key(KeyCode::Enter));
        assert_eq!(model.cursor, 0);

        handle_event(&mut model, key(KeyCode::Char('n')));
        assert_eq!(model.cursor, 8);
        handle_event(&mut model, key(KeyCode::Char('N')));
        assert_eq!(model.cursor, 0);

        let mut chunked = Model::new(EditorState::from_text("hello "), None);
        chunked.cursor = 6;
        chunked.insert_text("world");
        handle_event(&mut chunked, key(KeyCode::Char('/')));
        type_text(&mut chunked, "o w");
        handle_event(&mut chunked, key(KeyCode::Enter));
        assert_eq!(chunked.cursor, 4);
    }

    #[test]
    fn w_path_saves_as_and_updates_the_current_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("saved.txt");
        let mut model = Model::new(EditorState::from_text("data"), None);
        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(&mut model, &format!("w {}", path.display()));
        handle_event(&mut model, key(KeyCode::Enter));

        assert_eq!(fs::read_to_string(&path).expect("read"), "data");
        assert_eq!(model.path.as_ref(), Some(&path));
        assert!(!model.modified);
    }

    #[test]
    fn insert_variants_append_and_open_lines() {
        let mut append = Model::new(EditorState::from_text("ac"), None);
        handle_event(&mut append, key(KeyCode::Char('a')));
        type_text(&mut append, "b");
        assert_eq!(append.editor.text().slice(0..3), "abc");

        let mut below = Model::new(EditorState::from_text("a"), None);
        handle_event(&mut below, key(KeyCode::Char('o')));
        type_text(&mut below, "b");
        assert_eq!(below.editor.text().slice(0..3), "a\nb");

        let mut above = Model::new(EditorState::from_text("a"), None);
        handle_event(&mut above, key(KeyCode::Char('O')));
        type_text(&mut above, "b");
        assert_eq!(above.editor.text().slice(0..3), "b\na");

        let mut end = Model::new(EditorState::from_text("a"), None);
        handle_event(&mut end, key(KeyCode::Char('A')));
        type_text(&mut end, "b");
        assert_eq!(end.editor.text().slice(0..2), "ab");
    }

    #[test]
    fn grapheme_motion_and_deletion_keep_emoji_whole() {
        let mut model = Model::new(EditorState::from_text("a👨‍👩‍👧‍👦b"), None);
        model.cursor = 1;
        model.move_char(1);
        assert_eq!(model.cursor, "a👨‍👩‍👧‍👦".len());
        model.move_char(-1);
        assert_eq!(model.cursor, 1);
        model.delete_forward();
        assert_eq!(model.editor.text().slice(0..2), "ab");

        let mut backspace = Model::new(EditorState::from_text("a👨‍👩‍👧‍👦b"), None);
        backspace.cursor = "a👨‍👩‍👧‍👦".len();
        backspace.backspace();
        assert_eq!(backspace.editor.text().slice(0..2), "ab");
    }
}

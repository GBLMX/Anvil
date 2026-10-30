//! Minimal ratatui frontend. This is TUI-0: read-only viewport navigation.
//!
//! Prefix state machines, editing, search, and gateway commands arrive in
//! later slices; this crate deliberately stays a single readable loop.

use std::{
    fs,
    io::{self, Stdout},
    path::{Path, PathBuf},
};

use anvil_api::EditorView;
use anvil_editor::EditorState;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
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

/// TUI state for the first read-only loop.
pub struct Model {
    /// Editor state and its shared text/index.
    pub editor: EditorState,
    /// Optional source path.
    pub path: Option<PathBuf>,
    /// First visible line.
    pub scroll: usize,
    /// Set by `q`.
    pub quit: bool,
}

impl Model {
    /// Builds a model from text and an optional source path.
    pub fn new(editor: EditorState, path: Option<PathBuf>) -> Self {
        Self {
            editor,
            path,
            scroll: 0,
            quit: false,
        }
    }

    /// Builds a model by reading a UTF-8 file, using lossy conversion for
    /// invalid bytes so the first slice always has something to render.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let bytes = fs::read(&path)?;
        let text = String::from_utf8_lossy(&bytes);
        Ok(Self::new(EditorState::from_text(&text), Some(path)))
    }

    /// Scrolls down one line, clamping at the last line.
    pub fn scroll_down(&mut self) {
        let last = self.editor.index().len_lines().saturating_sub(1);
        self.scroll = (self.scroll + 1).min(last);
    }

    /// Scrolls up one line.
    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
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

/// Renders the current viewport.
pub fn view(model: &Model, frame: &mut Frame) {
    let area = frame.area();
    let editor_view: EditorView = model.editor.snapshot().view();
    let title = model
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "[no file]".to_owned());
    let block = Block::bordered().title(format!("{title} · rev {}", editor_view.revision));
    frame.render_widget(block, area);
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    frame.render_widget(Paragraph::new(model.visible_lines(inner.height)), inner);
}

/// Runs the minimal terminal loop.
pub fn run(path: Option<PathBuf>) -> io::Result<()> {
    let mut model = match path {
        Some(path) => Model::open(path)?,
        None => Model::new(EditorState::from_text(""), None),
    };
    let mut terminal = TerminalSession::start()?;
    loop {
        terminal.terminal.draw(|frame| view(&model, frame))?;
        if event::poll(std::time::Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('q') => model.quit = true,
                    KeyCode::Char('j') | KeyCode::Down => model.scroll_down(),
                    KeyCode::Char('k') | KeyCode::Up => model.scroll_up(),
                    _ => {}
                },
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
        if model.quit {
            return Ok(());
        }
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
    use ratatui::{Terminal, backend::TestBackend};

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
    fn scrolls_and_clamps() {
        let mut model = Model::new(EditorState::from_text("a\nb\nc"), None);
        model.scroll_down();
        assert_eq!(model.scroll, 1);
        model.scroll_down();
        model.scroll_down();
        assert_eq!(model.scroll, 2);
        model.scroll_up();
        assert_eq!(model.scroll, 1);
    }
}

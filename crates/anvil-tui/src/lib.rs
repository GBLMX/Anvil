//! Minimal ratatui frontend. TUI-1 adds insert mode, cursor-independent
//! viewport tracking, and `:w` saving. File I/O runs through the workspace
//! gateway; there is no direct document fallback in the runtime path.

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    io::{self, Stdout},
    path::{Path, PathBuf},
    time::Instant,
};

use anvil_api::EditorView;
use anvil_core::{TextBuffer, TextDocument, end_word, next_word_start, previous_word_start};
use anvil_editor::EditorState;
use anvil_ipc::GatewayClient;
use crossterm::{
    cursor::SetCursorStyle,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::Rect,
    style::Modifier,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

mod clipboard;
mod gateway;
mod host_session;
pub mod lsp;
mod lsp_worker;
mod syntax;
use gateway::GatewaySession;
#[cfg(target_os = "linux")]
use host_session::{HostEvent, HostSession};
use lsp_worker::{LspEvent, LspWorker};
use syntax::{Highlight, SyntaxClass, SyntaxState};

const MAX_OPEN_BYTES: usize = 256 * 1024 * 1024;

const HELP_TEXT: &str = include_str!("help.txt");

#[derive(Clone)]
struct Buffer {
    editor: EditorState,
    path: Option<PathBuf>,
    cursor: usize,
    scroll: usize,
    hscroll: usize,
    modified: bool,
    search_query: String,
}

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

/// Vi word motion kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordMotion {
    NextStart,
    PreviousStart,
    End,
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
    /// Numeric count prefix, zero means no count.
    count: usize,
    /// Waiting for a register name after `"`.
    register_wait: bool,
    /// Register selected by `"x`.
    register: Option<char>,
    /// Named yank registers.
    registers: HashMap<char, String>,
    /// Register name waiting after `q` or `@`.
    macro_wait: Option<char>,
    /// Currently recording into a register.
    recording: Option<char>,
    /// Recorded key sequence.
    recording_keys: Vec<KeyEvent>,
    /// Recorded macros.
    macros: HashMap<char, Vec<KeyEvent>>,
    /// Last replayed macro.
    last_macro: Option<char>,
    /// Replay guard.
    replaying: bool,
    /// `c` / `y` operator waiting for an inner text object (`i`).
    object_pending: bool,
    /// Last substring search.
    search_query: String,
    /// Buffer replaced by the read-only help page.
    help_back: Option<Buffer>,
    /// All open buffers, including the active one after the first switch.
    buffers: Vec<Buffer>,
    /// Active buffer index.
    active: usize,
    /// Gateway client used for file I/O when present.
    gateway: Option<GatewayClient>,
    /// Owning child/socket for the current gateway client.
    gateway_session: Option<GatewaySession>,
    /// Second authenticated gateway connection owned by the LSP worker.
    lsp_client: Option<GatewayClient>,
    /// Active language-server worker.
    lsp_worker: Option<LspWorker>,
    /// Diagnostics keyed by workspace path.
    diagnostics: HashMap<PathBuf, Vec<lsp::Diagnostic>>,
    /// Workspace root used to resolve all file paths.
    workspace: PathBuf,
    /// Reject file operations instead of falling back when the gateway is absent.
    require_gateway: bool,
    /// Tree-sitter highlights cached by editor revision and path.
    syntax: RefCell<SyntaxState>,
    /// Debug plugin host session, when started.
    #[cfg(target_os = "linux")]
    host: Option<HostSession>,
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
            count: 0,
            register_wait: false,
            register: None,
            registers: HashMap::new(),
            macro_wait: None,
            recording: None,
            recording_keys: Vec::new(),
            macros: HashMap::new(),
            last_macro: None,
            replaying: false,
            object_pending: false,
            search_query: String::new(),
            help_back: None,
            buffers: Vec::new(),
            active: 0,
            gateway: None,
            gateway_session: None,
            lsp_client: None,
            lsp_worker: None,
            diagnostics: HashMap::new(),
            workspace: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            require_gateway: false,
            syntax: RefCell::new(SyntaxState::default()),
            #[cfg(target_os = "linux")]
            host: None,
        }
    }

    /// Builds a model that uses `gateway` for all file I/O.
    pub fn with_gateway(gateway: GatewayClient, workspace: PathBuf) -> Self {
        let mut model = Self::new(EditorState::from_text(""), None);
        model.gateway = Some(gateway);
        model.workspace = workspace;
        model.require_gateway = true;
        model
    }

    /// Replaces the active buffer with a UTF-8 file from the workspace.
    pub fn open_path(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let timing = std::env::var_os("ANVIL_OPEN_TIMING").is_some();
        let start = Instant::now();
        let absolute = self.absolute_path(path.as_ref())?;
        let text = self.read_file(&absolute)?;
        let read_done = start.elapsed();
        self.editor = EditorState::from_text(&text);
        if timing {
            eprintln!("open_path read={read_done:?} index={:?}", start.elapsed());
        }
        self.syntax.borrow_mut().invalidate();
        self.path = Some(absolute);
        self.cursor = 0;
        self.scroll = 0;
        self.hscroll = 0;
        self.modified = false;
        self.dirty = true;
        Ok(())
    }

    /// Builds a gateway-backed model by reading a UTF-8 file.
    pub fn open_with_gateway(
        path: impl AsRef<Path>,
        gateway: GatewayClient,
        workspace: PathBuf,
    ) -> Result<Self, String> {
        let mut model = Self::with_gateway(gateway, workspace);
        model.open_path(path)?;
        Ok(model)
    }

    /// Builds a model by reading a UTF-8 file.
    ///
    /// Invalid UTF-8 is rejected instead of being lossily replaced, because
    /// saving a lossy view would silently corrupt the original bytes.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let text = fs::read_to_string(&path)?;
        let path = path.canonicalize().unwrap_or(path);
        Ok(Self::new(EditorState::from_text(&text), Some(path)))
    }

    fn absolute_path(&self, path: &Path) -> Result<PathBuf, String> {
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace.join(path)
        };
        Ok(joined.canonicalize().unwrap_or(joined))
    }

    fn relative_path(&self, path: &Path) -> Result<String, String> {
        let relative = path
            .strip_prefix(&self.workspace)
            .map_err(|_| format!("path is outside workspace {}", self.workspace.display()))?;
        let relative = relative
            .to_str()
            .filter(|relative| !relative.is_empty() && *relative != ".")
            .ok_or_else(|| "invalid file path".to_owned())?;
        Ok(relative.to_owned())
    }

    fn read_file(&mut self, path: &Path) -> Result<String, String> {
        if !self.require_gateway {
            let data = fs::read(path).map_err(|error| error.to_string())?;
            return String::from_utf8(data)
                .map_err(|_| format!("{}: invalid UTF-8", path.display()));
        }
        let relative = self.relative_path(path)?;
        let gateway = self
            .gateway
            .as_mut()
            .ok_or_else(|| "gateway unavailable".to_owned())?;
        let size = gateway
            .file_size(&relative)
            .map_err(|error| error.to_string())?;
        let window = std::env::var("ANVIL_TUI_READ_WINDOW")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|window| *window > 0)
            .unwrap_or(4);
        let data = gateway
            .read_all_binary_pipelined(&relative, size, MAX_OPEN_BYTES, window)
            .map_err(|error| error.to_string())?;
        String::from_utf8(data).map_err(|_| format!("{relative}: invalid UTF-8"))
    }

    fn write_file(&mut self, path: &Path, text: &str) -> Result<(), String> {
        if !self.require_gateway {
            return fs::write(path, text).map_err(|error| error.to_string());
        }
        let relative = self.relative_path(path)?;
        let gateway = self
            .gateway
            .as_mut()
            .ok_or_else(|| "gateway unavailable".to_owned())?;
        gateway
            .write_all_binary(&relative, text.as_bytes())
            .map_err(|error| error.to_string())
    }

    /// Dispatches one terminal event through the normal input path.
    ///
    /// Used by tests and benchmarks; terminal loops read events themselves.
    pub fn dispatch(&mut self, event: Event) {
        handle_event(self, event);
    }

    fn snapshot(&self) -> Buffer {
        Buffer {
            editor: self.editor.clone(),
            path: self.path.clone(),
            cursor: self.cursor,
            scroll: self.scroll,
            hscroll: self.hscroll,
            modified: self.modified,
            search_query: self.search_query.clone(),
        }
    }

    fn sync_active(&mut self) {
        let snapshot = self.snapshot();
        if let Some(buffer) = self.buffers.get_mut(self.active) {
            *buffer = snapshot;
        }
    }

    fn load_buffer(&mut self, index: usize) {
        let Some(buffer) = self.buffers.get(index).cloned() else {
            return;
        };
        self.editor = buffer.editor;
        self.path = buffer.path;
        self.cursor = buffer.cursor;
        self.scroll = buffer.scroll;
        self.hscroll = buffer.hscroll;
        self.modified = buffer.modified;
        self.search_query = buffer.search_query;
        self.syntax.borrow_mut().invalidate();
        self.active = index;
        self.input_mode = InputMode::Normal;
        self.pending = None;
        self.dirty = true;
    }

    fn open_file(&mut self, path: &str) {
        if path.is_empty() {
            self.status = "E: file name required".to_owned();
            self.dirty = true;
            return;
        }
        if self.help_back.is_some() {
            let _ = self.close_help();
        }
        let path = match self.absolute_path(Path::new(path)) {
            Ok(path) => path,
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                return;
            }
        };
        if self.path.as_ref() == Some(&path) {
            self.status = format!("Already editing {}", path.display());
            self.dirty = true;
            return;
        }
        if let Some(index) = self
            .buffers
            .iter()
            .position(|buffer| buffer.path.as_ref() == Some(&path))
        {
            self.sync_active();
            self.load_buffer(index);
            self.status = format!(
                "buffer {}/{}: {}",
                index + 1,
                self.buffers.len(),
                path.display()
            );
            self.dirty = true;
            return;
        }
        let text = match self.read_file(&path) {
            Ok(text) => text,
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                return;
            }
        };
        self.sync_active();
        if self.buffers.is_empty() {
            self.buffers.push(self.snapshot());
        }
        self.buffers.push(Buffer {
            editor: EditorState::from_text(&text),
            path: Some(path.clone()),
            cursor: 0,
            scroll: 0,
            hscroll: 0,
            modified: false,
            search_query: String::new(),
        });
        let index = self.buffers.len() - 1;
        self.load_buffer(index);
        self.status = format!(
            "buffer {}/{}: {}",
            index + 1,
            self.buffers.len(),
            path.display()
        );
        self.dirty = true;
    }

    fn switch_buffer(&mut self, delta: isize) {
        if self.help_back.is_some() {
            let _ = self.close_help();
        }
        if self.buffers.is_empty() {
            self.status = "E: only one buffer".to_owned();
            self.dirty = true;
            return;
        }
        self.sync_active();
        let len = self.buffers.len();
        let target = (self.active as isize + delta).rem_euclid(len as isize) as usize;
        if target == self.active {
            self.status = "E: only one buffer".to_owned();
            self.dirty = true;
            return;
        }
        self.load_buffer(target);
        let name = self
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "[no file]".to_owned());
        self.status = format!("buffer {}/{}: {name}", target + 1, len);
    }

    fn push_count(&mut self, character: char) {
        self.count = self.count.saturating_mul(10) + usize::from(character as u8 - b'0');
        self.dirty = true;
    }

    fn take_count(&mut self) -> usize {
        let count = self.count.max(1);
        self.count = 0;
        count
    }

    /// Moves the cursor by repeated vi word motions.
    pub fn move_words(&mut self, motion: WordMotion) {
        let count = self.take_count();
        for _ in 0..count {
            let target = match motion {
                WordMotion::NextStart => next_word_start(self.editor.text(), self.cursor),
                WordMotion::PreviousStart => previous_word_start(self.editor.text(), self.cursor),
                WordMotion::End => end_word(self.editor.text(), self.cursor),
            };
            if target == self.cursor {
                break;
            }
            self.cursor = target;
            self.dirty = true;
        }
    }

    pub fn move_word(&mut self, motion: WordMotion) {
        self.move_words(motion);
    }

    fn change_word(&mut self, motion: WordMotion) {
        if self.reject_read_only() {
            return;
        }
        let Some(range) = self.word_range(motion) else {
            return;
        };
        let deleted = self.editor.text().slice(range.clone());
        self.editor.set_selection_bytes(range.start, range.end);
        self.editor.delete(range.clone());
        if let Some(register) = self.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.cursor = range.start;
        self.modified = true;
        self.input_mode = InputMode::Insert;
        self.dirty = true;
    }

    fn change_inner_word(&mut self) {
        if self.reject_read_only() {
            return;
        }
        let Some(range) = self.inner_word_range() else {
            self.status = "E: no word".to_owned();
            self.dirty = true;
            return;
        };
        let deleted = self.editor.text().slice(range.clone());
        self.editor.set_selection_bytes(range.start, range.end);
        self.editor.delete(range.clone());
        if let Some(register) = self.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.cursor = range.start;
        self.modified = true;
        self.input_mode = InputMode::Insert;
        self.dirty = true;
    }

    fn change_inner_quote(&mut self, quote: char) {
        if self.reject_read_only() {
            return;
        }
        let Some(range) = self.inner_quote_range(quote) else {
            self.status = "E: no quote pair".to_owned();
            self.dirty = true;
            return;
        };
        let deleted = self.editor.text().slice(range.clone());
        self.editor.set_selection_bytes(range.start, range.end);
        self.editor.delete(range.clone());
        if let Some(register) = self.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.cursor = range.start;
        self.modified = true;
        self.input_mode = InputMode::Insert;
        self.dirty = true;
    }

    fn inner_word_range(&self) -> Option<std::ops::Range<usize>> {
        let line = self.editor.line_range_at(self.cursor)?;
        let text = self.editor.text().slice(line.start..line.end);
        let local = self.cursor.saturating_sub(line.start).min(text.len());
        let class_of = |character: char| {
            if character.is_whitespace() {
                0
            } else if character.is_alphanumeric() || character == '_' {
                1
            } else {
                2
            }
        };
        let current = text[..local]
            .chars()
            .next_back()
            .or_else(|| text[local..].chars().next())
            .map(class_of)?;
        let mut start = local;
        for (index, character) in text[..local].char_indices().rev() {
            if class_of(character) != current {
                break;
            }
            start = index;
        }
        let mut end = local;
        for (index, character) in text[local..].char_indices() {
            if class_of(character) != current {
                break;
            }
            end = local + index + character.len_utf8();
        }
        (start < end).then_some(line.start + start..line.start + end)
    }

    fn inner_quote_range(&self, quote: char) -> Option<std::ops::Range<usize>> {
        let line = self.editor.line_range_at(self.cursor)?;
        let text = self.editor.text().slice(line.start..line.end);
        let local = self.cursor.saturating_sub(line.start).min(text.len());
        let left = text[..local].rfind(quote)?;
        let right = text[local..].find(quote)? + local;
        (left + quote.len_utf8() <= right)
            .then_some(line.start + left + quote.len_utf8()..line.start + right)
    }

    fn word_range(&self, motion: WordMotion) -> Option<std::ops::Range<usize>> {
        let text = self.editor.text();
        let (start, end) = match motion {
            WordMotion::NextStart => (self.cursor, next_word_start(text, self.cursor)),
            WordMotion::PreviousStart => (previous_word_start(text, self.cursor), self.cursor),
            WordMotion::End => (self.cursor, self.word_end_exclusive()),
        };
        (start < end).then_some(start..end)
    }

    fn word_end_exclusive(&self) -> usize {
        let text = self.editor.text();
        let end = end_word(text, self.cursor);
        self.editor
            .next_grapheme_boundary(end)
            .filter(|next| *next > end)
            .unwrap_or(end)
            .min(text.len_bytes())
    }

    fn delete_words(&mut self, motion: WordMotion, count: usize) {
        let mut deleted = String::new();
        for _ in 0..count {
            let Some(range) = self.word_range(motion) else {
                break;
            };
            deleted.push_str(&self.editor.text().slice(range.clone()));
            self.editor.set_selection_bytes(range.start, range.end);
            self.editor.delete(range.clone());
            self.cursor = range.start;
            self.modified = true;
        }
        if let Some(register) = self.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.dirty = true;
    }

    fn yank_words(&mut self, motion: WordMotion, count: usize) {
        for _ in 0..count {
            self.yank_word(motion);
        }
    }

    fn yank_word(&mut self, motion: WordMotion) {
        let Some(range) = self.word_range(motion) else {
            return;
        };
        let cursor = self.cursor;
        self.editor.set_selection_bytes(range.start, range.end);
        if self.editor.yank(range) {
            self.record_register();
        }
        let _ = self.editor.set_selection_bytes(cursor, cursor);
        self.dirty = true;
    }

    fn move_lines(&mut self, delta: isize) {
        let count = self.take_count();
        for _ in 0..count {
            self.move_line(delta);
        }
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

    fn move_chars(&mut self, delta: isize) {
        let count = self.take_count();
        for _ in 0..count {
            self.move_char(delta);
        }
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
        let margin = 3.min(height.saturating_sub(1) / 2);
        if line < self.scroll.saturating_add(margin) {
            self.scroll = line.saturating_sub(margin);
        } else if line + margin >= self.scroll.saturating_add(height) {
            self.scroll = line + margin + 1 - height;
        }
        let column = position.display_column;
        let width = usize::from(width.max(1));
        if column < self.hscroll {
            self.hscroll = column;
        } else if column >= self.hscroll.saturating_add(width) {
            self.hscroll = column + 1 - width;
        }
    }

    fn reject_read_only(&mut self) -> bool {
        if self.help_back.is_some() {
            self.status = "E: help is read-only".to_owned();
            self.dirty = true;
            true
        } else {
            false
        }
    }

    fn show_help(&mut self) {
        if self.help_back.is_some() {
            self.status = "Already viewing help".to_owned();
            self.dirty = true;
            return;
        }
        let editor = std::mem::replace(&mut self.editor, EditorState::from_text(HELP_TEXT));
        let saved = Buffer {
            editor,
            path: self.path.take(),
            cursor: self.cursor,
            scroll: self.scroll,
            hscroll: self.hscroll,
            modified: self.modified,
            search_query: std::mem::take(&mut self.search_query),
        };
        self.help_back = Some(saved);
        self.cursor = 0;
        self.scroll = 0;
        self.hscroll = 0;
        self.modified = false;
        self.input_mode = InputMode::Normal;
        self.pending = None;
        self.status = "Help: :q returns".to_owned();
        self.dirty = true;
    }

    fn close_help(&mut self) -> bool {
        let Some(saved) = self.help_back.take() else {
            return false;
        };
        self.editor = saved.editor;
        self.path = saved.path;
        self.cursor = saved.cursor;
        self.scroll = saved.scroll;
        self.hscroll = saved.hscroll;
        self.modified = saved.modified;
        self.search_query = saved.search_query;
        self.syntax.borrow_mut().invalidate();
        self.input_mode = InputMode::Normal;
        self.pending = None;
        self.status = "Returned from help".to_owned();
        self.dirty = true;
        true
    }

    fn force_quit(&mut self) {
        if !self.close_help() {
            self.quit = true;
        }
    }

    fn insert_here(&mut self) {
        if self.reject_read_only() {
            return;
        }
        self.input_mode = InputMode::Insert;
        self.dirty = true;
    }

    /// Inserts text at the cursor.
    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.reject_read_only() {
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
        if self.reject_read_only() {
            return;
        }
        if self.editor.undo() {
            self.sync_cursor_from_selection();
            self.modified = true;
        } else {
            self.status = "Already at oldest change".to_owned();
        }
        self.dirty = true;
    }

    fn redo(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if self.editor.redo() {
            self.sync_cursor_from_selection();
            self.modified = true;
        } else {
            self.status = "Already at newest change".to_owned();
        }
        self.dirty = true;
    }

    fn delete_lines(&mut self, count: usize) {
        if self.reject_read_only() {
            return;
        }
        let Some(position) = self
            .editor
            .index()
            .position(self.editor.text(), self.cursor)
        else {
            return;
        };
        let Some(start) = self.editor.index().line_to_offset(position.line) else {
            return;
        };
        let Some(last_start) = self
            .editor
            .index()
            .line_to_offset(position.line.saturating_add(count.saturating_sub(1)))
        else {
            return;
        };
        let Some(last) = self.editor.line_range_at(last_start) else {
            return;
        };
        let deleted = self.editor.text().slice(start..last.end);
        self.editor.set_selection_bytes(start, last.end);
        self.editor.delete(start..last.end);
        self.cursor = start;
        if let Some(register) = self.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.modified = true;
        self.dirty = true;
    }

    fn yank_lines(&mut self, count: usize) {
        let Some(position) = self
            .editor
            .index()
            .position(self.editor.text(), self.cursor)
        else {
            return;
        };
        let Some(start) = self.editor.index().line_to_offset(position.line) else {
            return;
        };
        let Some(last_start) = self
            .editor
            .index()
            .line_to_offset(position.line.saturating_add(count.saturating_sub(1)))
        else {
            return;
        };
        let Some(last) = self.editor.line_range_at(last_start) else {
            return;
        };
        if self.editor.yank(start..last.end.max(start)) {
            self.record_register();
            self.status = format!("{count} lines yanked");
        }
        self.dirty = true;
    }

    fn record_register(&mut self) {
        let Some(register) = self.register.take() else {
            return;
        };
        let text = self.editor.yank_text().to_owned();
        self.record_register_text(register, &text);
    }

    fn record_register_text(&mut self, register: char, text: &str) {
        match register {
            '_' => {}
            '+' => {
                if let Err(error) = clipboard::copy(text) {
                    self.status = format!("E: {error}");
                    self.dirty = true;
                }
            }
            _ => {
                self.registers.insert(register, text.to_owned());
            }
        }
    }

    /// Inserts the unnamed register at the cursor.
    fn put_register(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if let Some(register) = self.register.take() {
            if register == '_' {
                return;
            }
            let text = if register == '+' {
                match clipboard::paste() {
                    Ok(text) => text,
                    Err(error) => {
                        self.status = format!("E: {error}");
                        self.dirty = true;
                        return;
                    }
                }
            } else {
                let Some(text) = self.registers.get(&register).cloned() else {
                    self.status = "E: register empty".to_owned();
                    self.dirty = true;
                    return;
                };
                text
            };
            self.editor.set_selection_bytes(self.cursor, self.cursor);
            self.editor.insert(self.cursor, &text);
            self.cursor += text.len();
            self.modified = true;
            self.dirty = true;
            return;
        }
        if self.editor.put(self.cursor).is_some() {
            self.sync_cursor_from_selection();
            self.modified = true;
        } else {
            self.status = "E: register empty".to_owned();
        }
        self.dirty = true;
    }

    fn any_modified(&self) -> bool {
        self.modified
            || self
                .buffers
                .iter()
                .enumerate()
                .any(|(index, buffer)| index != self.active && buffer.modified)
    }
    fn request_quit(&mut self) {
        if self.close_help() {
            return;
        }
        if self.any_modified() {
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
        if self.reject_read_only() {
            return;
        }
        if let Some(line) = self.editor.index().offset_to_line(self.cursor)
            && self.cursor < line.end
        {
            self.move_char(1);
        }
        self.input_mode = InputMode::Insert;
        self.dirty = true;
    }

    fn append_line_end(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if let Some(line) = self.editor.index().offset_to_line(self.cursor) {
            self.cursor = line.end;
        }
        self.input_mode = InputMode::Insert;
        self.dirty = true;
    }

    fn open_below(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if let Some(line) = self.editor.index().offset_to_line(self.cursor) {
            self.cursor = line.end;
        }
        self.insert_text("\n");
        self.input_mode = InputMode::Insert;
    }

    fn open_above(&mut self) {
        if self.reject_read_only() {
            return;
        }
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
        if self.reject_read_only() {
            return;
        }
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
        if self.reject_read_only() {
            return;
        }
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
        if self.reject_read_only() {
            return false;
        }
        let path = match self.absolute_path(&path) {
            Ok(path) => path,
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                return false;
            }
        };
        let text = self.editor.text().slice(0..self.editor.text().len_bytes());
        let display = path.display().to_string();
        match self.write_file(&path, &text) {
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
        if self.reject_read_only() {
            return false;
        }
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

    fn substitute(&mut self, whole: bool, spec: &str) {
        if self.reject_read_only() {
            return;
        }
        let mut parts = spec.splitn(3, '/');
        let (Some(old), Some(new)) = (parts.next(), parts.next()) else {
            self.status = "E: usage: :s/old/new/[g]".to_owned();
            self.dirty = true;
            return;
        };
        let flags = parts.next().unwrap_or_default();
        if old.is_empty() {
            self.status = "E: empty search pattern".to_owned();
            self.dirty = true;
            return;
        }
        if !flags.is_empty() && flags != "g" {
            self.status = format!("E: unknown substitute flags: {flags}");
            self.dirty = true;
            return;
        }
        let range = if whole {
            0..self.editor.text().len_bytes()
        } else {
            let Some(line) = self.editor.index().offset_to_line(self.cursor) else {
                self.status = "E: no line".to_owned();
                self.dirty = true;
                return;
            };
            line.start..line.end
        };
        let matches = find_matches_in(self.editor.text(), old, range);
        let matches = if flags.contains('g') {
            let mut non_overlapping = Vec::new();
            let mut end = 0;
            for start in matches {
                if start >= end {
                    non_overlapping.push(start);
                    end = start + old.len();
                }
            }
            non_overlapping
        } else {
            matches.into_iter().take(1).collect()
        };
        if matches.is_empty() {
            self.status = "E: pattern not found".to_owned();
            self.dirty = true;
            return;
        }
        let first = matches[0];
        for start in matches.into_iter().rev() {
            let end = start + old.len();
            self.editor.set_selection_bytes(start, end);
            self.editor.delete(start..end);
            if !new.is_empty() {
                self.editor.insert(start, new);
            }
        }
        self.cursor = first.min(self.editor.text().len_bytes());
        self.editor.set_selection_bytes(self.cursor, self.cursor);
        self.modified = true;
        self.dirty = true;
        self.status = "substitution applied".to_owned();
    }

    #[cfg(target_os = "linux")]
    fn start_oom_host(&mut self) {
        if self.host.is_some() {
            self.status = "plugin host already running".to_owned();
            self.dirty = true;
            return;
        }
        match HostSession::start_oom(&self.workspace) {
            Ok(host) => {
                self.host = Some(host);
                self.status = "plugin host OOM probe starting".to_owned();
            }
            Err(error) => self.status = format!("E: {error}"),
        }
        self.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    fn start_oom_host(&mut self) {
        self.status = "E: plugin host is Linux-only".to_owned();
        self.dirty = true;
    }

    #[cfg(target_os = "linux")]
    fn start_host(&mut self) {
        if self.host.is_some() {
            self.status = "plugin host already running".to_owned();
            self.dirty = true;
            return;
        }
        match HostSession::start(&self.workspace) {
            Ok(host) => {
                self.host = Some(host);
                self.status = "plugin host starting".to_owned();
            }
            Err(error) => self.status = format!("E: {error}"),
        }
        self.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    fn start_host(&mut self) {
        self.status = "E: plugin host is Linux-only".to_owned();
        self.dirty = true;
    }

    #[cfg(target_os = "linux")]
    fn exec_host(&mut self) {
        let Some(host) = &self.host else {
            self.status = "E: plugin host not running".to_owned();
            self.dirty = true;
            return;
        };
        if !host.exec() {
            self.status = "E: plugin host channel closed".to_owned();
        }
        self.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    fn exec_host(&mut self) {}

    #[cfg(target_os = "linux")]
    fn stop_host(&mut self) {
        self.host = None;
        self.status = "plugin host stopped".to_owned();
        self.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    fn stop_host(&mut self) {}

    #[cfg(target_os = "linux")]
    pub fn poll_host(&mut self) {
        let Some(host) = &mut self.host else {
            return;
        };
        let mut stop = false;
        for event in host.poll() {
            match event {
                HostEvent::Ready => self.status = "plugin host connected".to_owned(),
                HostEvent::Pong => self.status = "plugin host ready".to_owned(),
                HostEvent::ExecUnsupported => {
                    self.status = "plugin sys.exec not implemented yet".to_owned();
                }
                HostEvent::Error(error) => {
                    self.status = format!("E: plugin host: {error}");
                    stop = true;
                }
                HostEvent::Exited(anvil_plugin_host::HostExit::OomKilled) => {
                    self.status = "⚠ Plugin Killed: Out of Memory".to_owned();
                }
                HostEvent::Exited(_) => {
                    self.status = "plugin host exited".to_owned();
                    stop = true;
                }
            }
            self.dirty = true;
        }
        if stop {
            self.host = None;
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn poll_host(&mut self) {}

    pub fn poll_syntax(&mut self) {
        let text = self.editor.text();
        let revision = self.editor.revision();
        let path = self.path.clone();
        if self
            .syntax
            .borrow_mut()
            .poll(text, revision, path.as_deref())
        {
            self.dirty = true;
        }
    }

    fn start_lsp(&mut self) {
        if self.lsp_worker.is_some() {
            self.status = "lsp already running".to_owned();
            self.dirty = true;
            return;
        }
        let Some(client) = self.lsp_client.take() else {
            self.status = "E: lsp gateway connection unavailable".to_owned();
            self.dirty = true;
            return;
        };
        let Some(path) = self.path.clone() else {
            self.status = "E: lsp requires an open file".to_owned();
            self.dirty = true;
            return;
        };
        let text = self.editor.text().slice(0..self.editor.text().len_bytes());
        match LspWorker::start(
            client,
            &lsp_tool(),
            Vec::new(),
            self.workspace.clone(),
            path,
            text,
        ) {
            Ok(worker) => {
                self.lsp_worker = Some(worker);
                self.status = "lsp starting".to_owned();
            }
            Err(error) => self.status = format!("E: lsp start: {error}"),
        }
        self.dirty = true;
    }

    fn stop_lsp(&mut self) {
        self.lsp_worker = None;
        self.status = "lsp stopped".to_owned();
        self.dirty = true;
    }

    fn poll_lsp(&mut self) {
        let Some(worker) = &self.lsp_worker else {
            return;
        };
        for event in worker.poll() {
            match event {
                LspEvent::Diagnostics(path, diagnostics) => {
                    self.diagnostics.insert(path, diagnostics);
                    self.status = format!(
                        "lsp diagnostics: {}",
                        self.diagnostics.values().map(Vec::len).sum::<usize>()
                    );
                }
                LspEvent::Crashed(error) => self.status = format!("E: lsp crashed: {error}"),
                LspEvent::Error(error) => self.status = format!("E: lsp: {error}"),
                LspEvent::Log(line) => self.status = format!("lsp: {line}"),
            }
            self.dirty = true;
        }
    }

    fn diagnostic_lines(&self) -> Vec<usize> {
        let Some(path) = &self.path else {
            return Vec::new();
        };
        self.diagnostics
            .get(path)
            .map(|diagnostics| {
                diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.range.start.line as usize)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn reconnect_gateway(&mut self) {
        if !self.require_gateway {
            self.status = "E: gateway is not required in this session".to_owned();
            self.dirty = true;
            return;
        }
        self.gateway = None;
        self.gateway_session = None;
        match GatewaySession::start(&self.workspace) {
            Ok((session, client, lsp_client)) => {
                self.gateway_session = Some(session);
                self.gateway = Some(client);
                self.lsp_client = Some(lsp_client);
                self.status = "gateway reconnected".to_owned();
            }
            Err(error) => {
                self.status = format!("E: reconnect failed: {error}");
            }
        }
        self.dirty = true;
    }

    fn execute_command(&mut self, command: &str) {
        if let Some(rest) = command.strip_prefix("%s/") {
            self.substitute(true, rest);
            return;
        }
        if let Some(rest) = command.strip_prefix("s/") {
            self.substitute(false, rest);
            return;
        }
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
            ("e", path) => self.open_file(path),
            ("bn", "") => self.switch_buffer(1),
            ("bp", "") => self.switch_buffer(-1),
            ("q", "") => self.request_quit(),
            ("q!", "") => self.force_quit(),
            ("help", "") => self.show_help(),
            ("reconnect", "") => self.reconnect_gateway(),
            ("lsp-start", "") => self.start_lsp(),
            ("lsp-stop", "") => self.stop_lsp(),
            ("plugin-start", "") => self.start_host(),
            ("plugin-oom", "") => self.start_oom_host(),
            ("plugin-exec", "") => self.exec_host(),
            ("plugin-stop", "") => self.stop_host(),
            ("wq", "") => {
                if self.save() {
                    if self.any_modified() {
                        self.status = "E: other buffers have unsaved changes".to_owned();
                        self.dirty = true;
                    } else {
                        self.quit = true;
                    }
                }
            }
            ("wq", path) => {
                if self.save_as(path) {
                    if self.any_modified() {
                        self.status = "E: other buffers have unsaved changes".to_owned();
                        self.dirty = true;
                    } else {
                        self.quit = true;
                    }
                }
            }
            _ => {
                self.status = format!("E: unknown command: {command}");
                self.dirty = true;
            }
        }
    }

    fn gutter_width(&self) -> u16 {
        let digits = self
            .editor
            .index()
            .len_lines()
            .max(1)
            .to_string()
            .len()
            .max(3);
        (digits + 1) as u16
    }

    fn current_line(&self) -> Option<usize> {
        self.editor
            .index()
            .position(self.editor.text(), self.cursor)
            .map(|position| position.line)
    }

    fn visible_gutter(&self, height: u16) -> Vec<Line<'static>> {
        let index = self.editor.index();
        let end = (self.scroll + usize::from(height)).min(index.len_lines());
        let current = self.current_line();
        let digits = usize::from(self.gutter_width().saturating_sub(1));
        let mut lines = Vec::with_capacity(end.saturating_sub(self.scroll));
        for line_number in self.scroll..end {
            let display = match current {
                Some(current) if current != line_number => current.abs_diff(line_number),
                _ => line_number + 1,
            };
            let number = format!("{display:>digits$} ");
            let mut style = Style::default().fg(Color::DarkGray);
            if Some(line_number) == current {
                style = style.bg(Color::Indexed(236));
            }
            lines.push(Line::from(Span::styled(number, style)));
        }
        if lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines
    }

    fn visible_lines(&self, height: u16) -> Vec<Line<'_>> {
        let index = self.editor.index();
        let text = self.editor.text();
        let total = index.len_lines();
        let end = (self.scroll + usize::from(height)).min(total);
        let current = self.current_line();
        let needle = match &self.input_mode {
            InputMode::Search { query } => query.as_str(),
            _ => self.search_query.as_str(),
        };
        let diagnostic_lines = self.diagnostic_lines();
        let mut lines = Vec::with_capacity(end.saturating_sub(self.scroll));
        for line_number in self.scroll..end {
            let Some(start) = index.line_to_offset(line_number) else {
                break;
            };
            let Some(line) = index.offset_to_line(start) else {
                break;
            };
            let mut spans = Vec::new();
            let mut base = start;
            let syntax_guard = if needle.is_empty() {
                Some(self.syntax.borrow())
            } else {
                None
            };
            let highlights = syntax_guard
                .as_deref()
                .map(SyntaxState::highlights)
                .unwrap_or(&[]);
            for chunk in text.chunks(start..line.end) {
                if needle.is_empty() {
                    push_syntax_spans(&mut spans, chunk, base, highlights);
                } else {
                    let mut from = 0;
                    for (at, _) in chunk.match_indices(needle) {
                        if at > from {
                            spans.push(Span::raw(&chunk[from..at]));
                        }
                        let style = if base + at == self.cursor {
                            Style::default().bg(Color::Yellow).fg(Color::Black)
                        } else {
                            Style::default().bg(Color::Indexed(88)).fg(Color::White)
                        };
                        spans.push(Span::styled(&chunk[at..at + needle.len()], style));
                        from = at + needle.len();
                    }
                    if from < chunk.len() {
                        spans.push(Span::raw(&chunk[from..]));
                    }
                }
                base += chunk.len();
            }
            if spans.is_empty() {
                spans.push(Span::raw(""));
            }
            let mut rendered = Line::from(spans);
            if Some(line_number) == current {
                rendered = rendered.style(Style::default().bg(Color::Indexed(236)));
            }
            if diagnostic_lines.contains(&line_number) {
                rendered = rendered.style(
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::UNDERLINED),
                );
            }
            lines.push(rendered);
        }
        if lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines
    }
}

fn push_syntax_spans<'a>(
    spans: &mut Vec<Span<'a>>,
    chunk: &'a str,
    base: usize,
    highlights: &[Highlight],
) {
    let chunk_end = base + chunk.len();
    let first = highlights.partition_point(|highlight| highlight.end <= base);
    let mut cursor = base;
    for highlight in &highlights[first..] {
        if highlight.start >= chunk_end {
            break;
        }
        let start = highlight.start.max(base).max(cursor);
        let end = highlight.end.min(chunk_end);
        if end <= start {
            continue;
        }
        if start > cursor {
            spans.push(Span::raw(&chunk[cursor - base..start - base]));
        }
        if end > start {
            spans.push(Span::styled(
                &chunk[start - base..end - base],
                syntax_style(highlight.class),
            ));
        }
        cursor = end.max(cursor);
    }
    if cursor < chunk_end {
        spans.push(Span::raw(&chunk[cursor - base..]));
    }
    if chunk.is_empty() {
        spans.push(Span::raw(""));
    }
}

fn syntax_style(class: SyntaxClass) -> Style {
    let color = match class {
        SyntaxClass::Keyword => Color::Magenta,
        SyntaxClass::String => Color::Green,
        SyntaxClass::Comment => Color::DarkGray,
        SyntaxClass::Number => Color::Yellow,
        SyntaxClass::Function => Color::Blue,
        SyntaxClass::Type => Color::Cyan,
        SyntaxClass::Property => Color::LightCyan,
        SyntaxClass::Constant => Color::Yellow,
    };
    Style::default().fg(color)
}

fn lsp_tool() -> String {
    if let Some(path) = std::env::var_os("ANVIL_LSP") {
        return path.to_string_lossy().into_owned();
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join("rust-analyzer");
            if candidate.is_file() {
                return candidate.to_string_lossy().into_owned();
            }
        }
    }
    "rust-analyzer".to_owned()
}

fn find_matches(text: &TextDocument, needle: &str) -> Vec<usize> {
    find_matches_in(text, needle, 0..text.len_bytes())
}

fn find_matches_in(text: &TextDocument, needle: &str, range: std::ops::Range<usize>) -> Vec<usize> {
    let needle = needle.as_bytes();
    if needle.is_empty() || needle.len() > range.end.saturating_sub(range.start) {
        return Vec::new();
    }
    let mut matches = Vec::new();
    let mut carry = Vec::new();
    let mut processed = range.start;
    for chunk in text.chunks(range) {
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
    let title = if model.help_back.is_some() {
        "[help]".to_owned()
    } else {
        let name = model
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "[no file]".to_owned());
        let name = if model.modified {
            format!("[+] {name}")
        } else {
            name
        };
        if model.buffers.is_empty() {
            name
        } else {
            format!("[{}/{}] {name}", model.active + 1, model.buffers.len())
        }
    };
    let debug_revision = std::env::var_os("ANVIL_DEBUG_REV").is_some();
    let title = if debug_revision {
        format!("{title} · rev {}", editor_view.revision)
    } else {
        title
    };
    let (mode_text, mode_color, status) = match &model.input_mode {
        InputMode::CommandLine { buffer } => (" CMD ", Color::Yellow, format!(":{buffer}")),
        InputMode::Search { query } => (" SRC ", Color::Yellow, format!("/{query}")),
        InputMode::Insert => (" INS ", Color::Green, String::new()),
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
            let status = if model.status.is_empty() {
                location
            } else {
                format!("{} ·{location}", model.status)
            };
            (" NOR ", Color::Blue, status)
        }
    };
    let mode = Span::styled(mode_text, Style::default().fg(Color::Black).bg(mode_color));
    let block = Block::bordered()
        .title(title)
        .title_bottom(Line::from(vec![mode, Span::raw(status)]));
    frame.render_widget(block, area);
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    let gutter_width = model.gutter_width().min(inner.width);
    let gutter_area = Rect {
        width: gutter_width,
        ..inner
    };
    let text_area = Rect {
        x: inner.x + gutter_width,
        width: inner.width.saturating_sub(gutter_width),
        ..inner
    };
    frame.render_widget(
        Paragraph::new(model.visible_gutter(inner.height)),
        gutter_area,
    );
    frame.render_widget(
        Paragraph::new(model.visible_lines(inner.height))
            .scroll((0, u16::try_from(model.hscroll).unwrap_or(u16::MAX))),
        text_area,
    );
    if let Some(position) = model
        .editor
        .index()
        .position(model.editor.text(), model.cursor)
    {
        let x = text_area.x + position.display_column.saturating_sub(model.hscroll) as u16;
        let y = inner.y + position.line.saturating_sub(model.scroll) as u16;
        if x < inner.right() && y < inner.bottom() {
            frame.set_cursor_position((x, y));
        }
    }
}

/// Runs the minimal terminal loop in the current directory workspace.
pub fn run(path: Option<PathBuf>) -> io::Result<()> {
    run_with_workspace(path, std::env::current_dir()?)
}

/// Runs the minimal terminal loop with an explicit workspace.
pub fn run_with_workspace(path: Option<PathBuf>, workspace: PathBuf) -> io::Result<()> {
    let timing = std::env::var_os("ANVIL_OPEN_TIMING").is_some();
    let start = Instant::now();
    let workspace = workspace.canonicalize()?;
    let (mut model, session) = match GatewaySession::start(&workspace) {
        Ok((session, client, lsp_client)) => {
            let mut model = Model::with_gateway(client, workspace.clone());
            model.lsp_client = Some(lsp_client);
            if let Some(path) = path
                && let Err(error) = model.open_path(path)
            {
                model.status = format!("E: {error}");
                model.dirty = true;
            }
            (model, Some(session))
        }
        Err(error) => {
            let mut model = Model::new(EditorState::from_text(""), None);
            model.workspace = workspace;
            model.require_gateway = true;
            model.status = format!("E: gateway unavailable: {error}");
            model.dirty = true;
            (model, None)
        }
    };
    model.gateway_session = session;
    let model_ready = start.elapsed();
    let mut terminal = TerminalSession::start()?;
    let terminal_ready = start.elapsed();
    let mut first_draw = true;
    let mut insert_cursor: Option<bool> = None;
    loop {
        let wants_bar = matches!(model.input_mode, InputMode::Insert);
        if insert_cursor != Some(wants_bar) {
            terminal.set_cursor_shape(wants_bar)?;
            insert_cursor = Some(wants_bar);
        }
        let size = terminal.terminal.size()?;
        model.ensure_cursor_visible(size.height.saturating_sub(2), size.width.saturating_sub(2));
        if model.dirty {
            terminal.terminal.draw(|frame| view(&model, frame))?;
            model.dirty = false;
            if first_draw {
                first_draw = false;
                if timing {
                    eprintln!(
                        "open_timing model={:?} terminal={:?} first_frame={:?}",
                        model_ready,
                        terminal_ready,
                        start.elapsed()
                    );
                }
            }
        }
        model.poll_host();
        model.poll_syntax();
        model.poll_lsp();
        if event::poll(std::time::Duration::from_millis(50))? {
            handle_event(&mut model, event::read()?);
        }
        if model.quit {
            return Ok(());
        }
    }
}

fn handle_event(model: &mut Model, event: Event) {
    model.syntax.borrow_mut().note_activity();
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

    if let Some(waiting) = model.macro_wait.take() {
        match waiting {
            'q' => {
                model.recording = Some(match key.code {
                    KeyCode::Char(register) => register,
                    _ => 'q',
                });
                model.recording_keys.clear();
            }
            '@' => {
                let register = match key.code {
                    KeyCode::Char('@') => model.last_macro,
                    KeyCode::Char(register) => Some(register),
                    _ => None,
                };
                if let Some(register) = register
                    && let Some(keys) = model.macros.get(&register).cloned()
                {
                    model.last_macro = Some(register);
                    model.replaying = true;
                    for key in keys {
                        handle_key(model, key);
                    }
                    model.replaying = false;
                }
            }
            _ => {}
        }
        return;
    }

    if let Some(register) = model.recording {
        if key.code == KeyCode::Char('q') {
            model.recording = None;
            model
                .macros
                .insert(register, std::mem::take(&mut model.recording_keys));
            model.dirty = true;
            return;
        }
        model.recording_keys.push(key);
    }

    if model.register_wait {
        if let KeyCode::Char(character) = key.code {
            model.register = Some(character);
            model.register_wait = false;
            model.dirty = true;
        }
        return;
    }
    if let Some(pending) = model.pending.take() {
        model.dirty = true;
        if model.object_pending {
            model.object_pending = false;
            if pending == 'c' {
                match key.code {
                    KeyCode::Char('w') => model.change_inner_word(),
                    KeyCode::Char('"') => model.change_inner_quote('"'),
                    KeyCode::Char('\'') => model.change_inner_quote('\''),
                    _ => {}
                }
            }
            return;
        }
        if pending == 'c' && key.code == KeyCode::Char('i') {
            model.pending = Some('c');
            model.object_pending = true;
            return;
        }
        if let KeyCode::Char(character) = key.code
            && character.is_ascii_digit()
        {
            model.count = model.count.saturating_mul(10) + usize::from(character as u8 - b'0');
            model.pending = Some(pending);
            return;
        }
        let count = model.take_count();
        match (pending, key.code) {
            ('c', KeyCode::Char('w')) => model.change_word(WordMotion::NextStart),
            ('c', KeyCode::Char('e')) => model.change_word(WordMotion::End),
            ('c', KeyCode::Char('b')) => model.change_word(WordMotion::PreviousStart),
            ('d', KeyCode::Char('d')) => model.delete_lines(count),
            ('y', KeyCode::Char('y')) => model.yank_lines(count),
            ('d', KeyCode::Char('w')) => model.delete_words(WordMotion::NextStart, count),
            ('d', KeyCode::Char('e')) => model.delete_words(WordMotion::End, count),
            ('d', KeyCode::Char('b')) => model.delete_words(WordMotion::PreviousStart, count),
            ('y', KeyCode::Char('w')) => model.yank_words(WordMotion::NextStart, count),
            ('y', KeyCode::Char('e')) => model.yank_words(WordMotion::End, count),
            ('y', KeyCode::Char('b')) => model.yank_words(WordMotion::PreviousStart, count),
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
            KeyCode::Char('q') if !control => {
                model.macro_wait = Some('q');
                model.dirty = true;
            }
            KeyCode::Char('@') if !control => {
                model.macro_wait = Some('@');
                model.dirty = true;
            }
            KeyCode::Char('i') => model.insert_here(),
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
            KeyCode::Char('c') if !control => model.pending = Some('c'),
            KeyCode::Char('"') if !control => {
                model.register_wait = true;
                model.dirty = true;
            }
            KeyCode::Char('x') if !control => {
                let count = model.take_count();
                for _ in 0..count {
                    model.delete_forward();
                }
            }
            KeyCode::Char('p') if !control => model.put_register(),
            KeyCode::Char('u') if !control => model.undo(),
            KeyCode::Char('r') if control => model.redo(),
            KeyCode::Char(character) if !control && character.is_ascii_digit() => {
                model.push_count(character);
            }
            KeyCode::Char('0') if !control && model.count == 0 => model.move_home(),
            KeyCode::Char('w') if !control => model.move_words(WordMotion::NextStart),
            KeyCode::Char('b') if !control => model.move_words(WordMotion::PreviousStart),
            KeyCode::Char('e') if !control => model.move_words(WordMotion::End),
            KeyCode::Char('h') | KeyCode::Left => model.move_chars(-1),
            KeyCode::Char('l') | KeyCode::Right => model.move_chars(1),
            KeyCode::Char('j') | KeyCode::Down => model.move_lines(1),
            KeyCode::Char('k') | KeyCode::Up => model.move_lines(-1),
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

impl TerminalSession {
    fn set_cursor_shape(&mut self, insert: bool) -> io::Result<()> {
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
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            SetCursorStyle::DefaultUserShape,
            LeaveAlternateScreen
        );
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
    fn syntax_keyword_is_rendered() {
        let mut model = Model::new(
            EditorState::from_text("fn main() {}"),
            Some(PathBuf::from("sample.rs")),
        );
        for _ in 0..200 {
            model.poll_syntax();
            if !model.syntax.borrow().highlights().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            !model.syntax.borrow().highlights().is_empty(),
            "no syntax highlights produced"
        );
        let backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| view(&model, frame)).expect("draw");
        let colored = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.fg == Color::Magenta);
        assert!(colored, "keyword span was not colored");
        model.dirty = false;
    }

    #[test]
    fn diagnostics_underline_matching_line() {
        let mut model = Model::new(
            EditorState::from_text("first\nsecond"),
            Some(PathBuf::from("diag.rs")),
        );
        model.diagnostics.insert(
            PathBuf::from("diag.rs"),
            vec![lsp::Diagnostic {
                range: lsp::Range {
                    start: lsp::Position {
                        line: 1,
                        character: 0,
                    },
                    end: lsp::Position {
                        line: 1,
                        character: 3,
                    },
                },
                severity: Some(1),
                message: "error".to_owned(),
                source: Some("test".to_owned()),
            }],
        );
        let backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| view(&model, frame)).expect("draw");
        let red = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.fg == Color::Red);
        assert!(red, "diagnostic line was not rendered red");
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

        for code in [KeyCode::Char(':'), KeyCode::Char('q'), KeyCode::Enter] {
            handle_event(&mut model, key(code));
        }
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
    fn delete_into_named_register() {
        let mut model = Model::new(EditorState::from_text("alpha beta"), None);
        for key_code in [
            KeyCode::Char('"'),
            KeyCode::Char('a'),
            KeyCode::Char('d'),
            KeyCode::Char('w'),
        ] {
            handle_event(&mut model, key(key_code));
        }
        assert_eq!(
            model.registers.get(&'a').map(String::as_str),
            Some("alpha ")
        );
        for key_code in [KeyCode::Char('"'), KeyCode::Char('a'), KeyCode::Char('p')] {
            handle_event(&mut model, key(key_code));
        }
        assert_eq!(model.editor.text().slice(0..10), "alpha beta");
    }

    #[test]
    fn named_register_yank_and_put() {
        let mut model = Model::new(EditorState::from_text("alpha\nbeta"), None);
        for key_code in [
            KeyCode::Char('"'),
            KeyCode::Char('a'),
            KeyCode::Char('y'),
            KeyCode::Char('y'),
        ] {
            handle_event(&mut model, key(key_code));
        }
        assert_eq!(
            model.registers.get(&'a').map(String::as_str),
            Some("alpha\n")
        );
        model.cursor = 0;
        for key_code in [KeyCode::Char('"'), KeyCode::Char('a'), KeyCode::Char('p')] {
            handle_event(&mut model, key(key_code));
        }
        assert_eq!(model.editor.text().slice(0..12), "alpha\nalpha\n");
    }

    #[test]
    fn macro_records_and_replays() {
        let mut model = Model::new(EditorState::from_text(""), None);
        for code in [
            KeyCode::Char('q'),
            KeyCode::Char('a'),
            KeyCode::Char('i'),
            KeyCode::Char('X'),
            KeyCode::Esc,
            KeyCode::Char('q'),
        ] {
            handle_event(&mut model, key(code));
        }
        assert_eq!(model.macros.get(&'a').map(Vec::len), Some(3));
        for code in [KeyCode::Char('@'), KeyCode::Char('a')] {
            handle_event(&mut model, key(code));
        }
        assert_eq!(model.editor.text().slice(0..2), "XX");
    }

    #[test]
    fn change_inner_word_and_quote() {
        let mut model = Model::new(EditorState::from_text("alpha beta"), None);
        for key_code in [KeyCode::Char('c'), KeyCode::Char('i'), KeyCode::Char('w')] {
            handle_event(&mut model, key(key_code));
        }
        handle_event(&mut model, key(KeyCode::Char('X')));
        handle_event(&mut model, key(KeyCode::Esc));
        assert_eq!(model.editor.text().slice(0..6), "X beta");

        let mut model = Model::new(EditorState::from_text("say \"hello\" now"), None);
        model.cursor = 6;
        for key_code in [KeyCode::Char('c'), KeyCode::Char('i'), KeyCode::Char('"')] {
            handle_event(&mut model, key(key_code));
        }
        handle_event(&mut model, key(KeyCode::Char('X')));
        handle_event(&mut model, key(KeyCode::Esc));
        assert_eq!(model.editor.text().slice(0..11), "say \"X\" now");
    }

    #[test]
    fn counts_repeat_motions_and_operators() {
        let mut model = Model::new(EditorState::from_text("a b c d"), None);
        handle_event(&mut model, key(KeyCode::Char('3')));
        handle_event(&mut model, key(KeyCode::Char('w')));
        assert_eq!(model.cursor, 6);

        let mut model = Model::new(EditorState::from_text("a b c d"), None);
        handle_event(&mut model, key(KeyCode::Char('d')));
        handle_event(&mut model, key(KeyCode::Char('2')));
        handle_event(&mut model, key(KeyCode::Char('w')));
        assert_eq!(model.editor.text().slice(0..3), "c d");
    }

    #[test]
    fn word_motions_follow_vi_boundaries() {
        let mut model = Model::new(EditorState::from_text("alpha beta.gamma"), None);
        handle_event(&mut model, key(KeyCode::Char('w')));
        assert_eq!(model.cursor, 6);
        handle_event(&mut model, key(KeyCode::Char('w')));
        assert_eq!(model.cursor, 10);
        handle_event(&mut model, key(KeyCode::Char('b')));
        assert_eq!(model.cursor, 6);
        handle_event(&mut model, key(KeyCode::Char('e')));
        assert_eq!(model.cursor, 9);
    }

    #[test]
    fn substitute_is_literal_and_line_scoped() {
        let mut model = Model::new(EditorState::from_text("foo foo\nfoo"), None);
        model.execute_command("s/foo/bar/");
        assert_eq!(model.editor.text().slice(0..11), "bar foo\nfoo");
        assert!(model.modified);

        let mut model = Model::new(EditorState::from_text("foo foo\nfoo"), None);
        model.execute_command("%s/foo/bar/g");
        assert_eq!(model.editor.text().slice(0..11), "bar bar\nbar");
    }

    #[test]
    fn substitute_uses_non_overlapping_matches() {
        let mut model = Model::new(EditorState::from_text("aaa"), None);
        model.execute_command("%s/aa/X/g");
        assert_eq!(model.editor.text().slice(0..2), "Xa");
    }

    #[test]
    fn substitute_updates_offsets_after_length_change() {
        let mut model = Model::new(EditorState::from_text("a a"), None);
        model.execute_command("%s/a/aa/g");
        assert_eq!(model.editor.text().slice(0..5), "aa aa");
    }

    #[test]
    fn word_end_keeps_multibyte_boundary() {
        let mut model = Model::new(EditorState::from_text("café"), None);
        handle_event(&mut model, key(KeyCode::Char('e')));
        assert_eq!(model.cursor, 3);
        assert!(model.editor.text().is_char_boundary(model.cursor));
    }

    #[test]
    fn dw_deletes_to_next_word_start_and_undoes() {
        let mut model = Model::new(EditorState::from_text("alpha beta"), None);
        handle_event(&mut model, key(KeyCode::Char('d')));
        handle_event(&mut model, key(KeyCode::Char('w')));
        assert_eq!(model.editor.text().slice(0..4), "beta");
        assert_eq!(model.cursor, 0);
        handle_event(&mut model, key(KeyCode::Char('u')));
        assert_eq!(model.editor.text().slice(0..10), "alpha beta");
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
    #[cfg(unix)]
    fn open_resolves_symlink_to_canonical_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("target.txt");
        let link = dir.path().join("link.txt");
        fs::write(&target, "first\nsecond").expect("write");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let mut model = Model::open(&link).expect("open");
        assert_eq!(model.path.as_deref(), Some(target.as_path()));
        model.diagnostics.insert(
            model.path.clone().expect("path"),
            vec![lsp::Diagnostic {
                range: lsp::Range {
                    start: lsp::Position {
                        line: 1,
                        character: 0,
                    },
                    end: lsp::Position {
                        line: 1,
                        character: 3,
                    },
                },
                severity: Some(1),
                message: "error".to_owned(),
                source: Some("test".to_owned()),
            }],
        );
        assert_eq!(model.diagnostic_lines(), vec![1]);
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

    #[test]
    fn help_is_read_only_and_returns_to_the_previous_buffer() {
        let original_path = PathBuf::from("file.txt");
        let mut model = Model::new(
            EditorState::from_text("original"),
            Some(original_path.clone()),
        );
        model.cursor = 3;
        model.scroll = 1;
        model.hscroll = 2;
        model.modified = true;

        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(&mut model, "help");
        handle_event(&mut model, key(KeyCode::Enter));
        assert!(model.help_back.is_some());
        assert_eq!(model.editor.text().slice(0..HELP_TEXT.len()), HELP_TEXT);
        assert_eq!(model.input_mode, InputMode::Normal);

        handle_event(&mut model, key(KeyCode::Char('i')));
        type_text(&mut model, "Z");
        assert_eq!(model.input_mode, InputMode::Normal);
        assert!(model.status.contains("read-only"));
        assert_eq!(model.editor.text().slice(0..HELP_TEXT.len()), HELP_TEXT);

        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(&mut model, "q");
        handle_event(&mut model, key(KeyCode::Enter));
        assert!(model.help_back.is_none());
        assert_eq!(model.editor.text().slice(0..8), "original");
        assert_eq!(model.path, Some(original_path));
        assert_eq!(model.cursor, 3);
        assert_eq!(model.scroll, 1);
        assert_eq!(model.hscroll, 2);
        assert!(model.modified);
    }

    #[test]
    fn help_refuses_writes() {
        let mut model = Model::new(EditorState::from_text("data"), None);
        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(&mut model, "help");
        handle_event(&mut model, key(KeyCode::Enter));

        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(&mut model, "w");
        handle_event(&mut model, key(KeyCode::Enter));
        assert!(model.help_back.is_some());
        assert!(model.status.contains("read-only"));
        assert_eq!(model.editor.text().slice(0..HELP_TEXT.len()), HELP_TEXT);
    }

    #[test]
    fn multi_buffer_switch_preserves_per_buffer_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("first.txt");
        let second = dir.path().join("second.txt");
        fs::write(&first, "first").expect("write first");
        fs::write(&second, "second").expect("write second");

        let mut model = Model::open(&first).expect("open first");
        model.cursor = 2;
        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(&mut model, &format!("e {}", second.display()));
        handle_event(&mut model, key(KeyCode::Enter));
        assert_eq!(model.path.as_ref(), Some(&second));
        assert_eq!(model.editor.text().slice(0..6), "second");

        model.cursor = 1;
        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(&mut model, "bn");
        handle_event(&mut model, key(KeyCode::Enter));
        assert_eq!(model.path.as_ref(), Some(&first));
        assert_eq!(model.cursor, 2);

        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(&mut model, "bn");
        handle_event(&mut model, key(KeyCode::Enter));
        assert_eq!(model.path.as_ref(), Some(&second));
        assert_eq!(model.cursor, 1);

        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(&mut model, &format!("e {}", first.display()));
        handle_event(&mut model, key(KeyCode::Enter));
        assert_eq!(model.path.as_ref(), Some(&first));
        assert_eq!(model.buffers.len(), 2);
    }

    #[test]
    fn buffer_commands_require_a_second_buffer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("only.txt");
        fs::write(&path, "only").expect("write");
        let mut model = Model::open(&path).expect("open");

        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(&mut model, "bn");
        handle_event(&mut model, key(KeyCode::Enter));
        assert_eq!(model.path.as_ref(), Some(&path));
        assert_eq!(model.status, "E: only one buffer");
    }

    #[test]
    fn quit_guard_checks_inactive_buffers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("first.txt");
        let second = dir.path().join("second.txt");
        fs::write(&first, "first").expect("write first");
        fs::write(&second, "second").expect("write second");

        let mut model = Model::open(&first).expect("open");
        handle_event(&mut model, key(KeyCode::Char('i')));
        type_text(&mut model, "X");
        handle_event(&mut model, key(KeyCode::Esc));
        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(&mut model, &format!("e {}", second.display()));
        handle_event(&mut model, key(KeyCode::Enter));
        assert_eq!(model.path.as_ref(), Some(&second));

        for code in [KeyCode::Char(':'), KeyCode::Char('q'), KeyCode::Enter] {
            handle_event(&mut model, key(code));
        }
        assert!(!model.quit);
        assert!(model.status.contains("No write since last change"));

        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(&mut model, "q!");
        handle_event(&mut model, key(KeyCode::Enter));
        assert!(model.quit);
    }

    #[test]
    fn gateway_required_refuses_file_operations_without_client() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("file.txt");
        let mut model = Model::new(EditorState::from_text("data"), None);
        model.require_gateway = true;
        model.workspace = directory.path().to_path_buf();
        model.path = Some(path);
        assert!(!model.save());
        assert!(model.status.contains("gateway unavailable"));

        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(&mut model, "e other.txt");
        handle_event(&mut model, key(KeyCode::Enter));
        assert!(model.status.contains("gateway unavailable"));
    }

    #[test]
    fn gateway_refuses_paths_outside_workspace() {
        let directory = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside");
        let mut model = Model::new(EditorState::from_text(""), None);
        model.require_gateway = true;
        model.workspace = directory.path().to_path_buf();
        handle_event(&mut model, key(KeyCode::Char(':')));
        type_text(
            &mut model,
            &format!("e {}", outside.path().join("x").display()),
        );
        handle_event(&mut model, key(KeyCode::Enter));
        assert!(
            model.status.contains("outside workspace"),
            "{}",
            model.status
        );
    }
}

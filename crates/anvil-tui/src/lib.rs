//! Minimal ratatui frontend. TUI-1 adds insert mode, cursor-independent
//! viewport tracking, and `:w` saving. File I/O runs through the workspace
//! gateway; there is no direct document fallback in the runtime path.

use std::{
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
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

mod gateway;
use gateway::GatewaySession;

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
    /// Workspace root used to resolve all file paths.
    workspace: PathBuf,
    /// Reject file operations instead of falling back when the gateway is absent.
    require_gateway: bool,
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
            help_back: None,
            buffers: Vec::new(),
            active: 0,
            gateway: None,
            workspace: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            require_gateway: false,
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
        Ok(Self::new(EditorState::from_text(&text), Some(path)))
    }

    fn absolute_path(&self, path: &Path) -> Result<PathBuf, String> {
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            Ok(self.workspace.join(path))
        }
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

    /// Moves the cursor by a vi word motion.
    pub fn move_word(&mut self, motion: WordMotion) {
        let target = match motion {
            WordMotion::NextStart => next_word_start(self.editor.text(), self.cursor),
            WordMotion::PreviousStart => previous_word_start(self.editor.text(), self.cursor),
            WordMotion::End => end_word(self.editor.text(), self.cursor),
        };
        if target != self.cursor {
            self.cursor = target;
            self.dirty = true;
        }
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

    fn delete_word(&mut self, motion: WordMotion) {
        if self.reject_read_only() {
            return;
        }
        let Some(range) = self.word_range(motion) else {
            return;
        };
        self.editor.set_selection_bytes(range.start, range.end);
        self.editor.delete(range.clone());
        self.cursor = range.start;
        self.modified = true;
        self.dirty = true;
    }

    fn yank_word(&mut self, motion: WordMotion) {
        let Some(range) = self.word_range(motion) else {
            return;
        };
        let cursor = self.cursor;
        self.editor.set_selection_bytes(range.start, range.end);
        let _ = self.editor.yank(range);
        let _ = self.editor.set_selection_bytes(cursor, cursor);
        self.dirty = true;
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

    fn delete_line(&mut self) {
        if self.reject_read_only() {
            return;
        }
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
        if self.reject_read_only() {
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
        let mut matches = find_matches_in(self.editor.text(), old, range);
        if !flags.contains('g') {
            matches.truncate(1);
        }
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
            for chunk in text.chunks(start..line.end) {
                if needle.is_empty() {
                    spans.push(Span::raw(chunk));
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
            lines.push(rendered);
        }
        if lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines
    }
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
    let (mut model, _session) = match GatewaySession::start(&workspace) {
        Ok((session, client)) => {
            let mut model = Model::with_gateway(client, workspace.clone());
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
            ('d', KeyCode::Char('w')) => model.delete_word(WordMotion::NextStart),
            ('d', KeyCode::Char('e')) => model.delete_word(WordMotion::End),
            ('d', KeyCode::Char('b')) => model.delete_word(WordMotion::PreviousStart),
            ('y', KeyCode::Char('w')) => model.yank_word(WordMotion::NextStart),
            ('y', KeyCode::Char('e')) => model.yank_word(WordMotion::End),
            ('y', KeyCode::Char('b')) => model.yank_word(WordMotion::PreviousStart),
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
            KeyCode::Char('x') if !control => model.delete_forward(),
            KeyCode::Char('p') if !control => model.put_register(),
            KeyCode::Char('u') if !control => model.undo(),
            KeyCode::Char('r') if control => model.redo(),
            KeyCode::Char('w') if !control => model.move_word(WordMotion::NextStart),
            KeyCode::Char('b') if !control => model.move_word(WordMotion::PreviousStart),
            KeyCode::Char('e') if !control => model.move_word(WordMotion::End),
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

        handle_event(&mut model, key(KeyCode::Char('q')));
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

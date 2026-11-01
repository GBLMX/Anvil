//! Ratatui editor shell and shared model.
//!
//! LSP behavior, file-tree state, rendering, and key dispatch live in focused
//! sibling modules; this module keeps buffer/model state and composition.

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    io::{self, Stdout},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anvil_core::{TextBuffer, TextDocument, end_word, next_word_start, previous_word_start};
use anvil_editor::EditorState;
use anvil_ipc::GatewayClient;
use crossterm::{
    cursor::SetCursorStyle,
    event::{self, Event, KeyEvent},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    style::{Modifier, Style},
    text::{Line, Span},
};

mod buffer;
mod clipboard;
mod commands;
mod editing;
mod file_io;
mod file_tree_ui;
mod file_worker;
mod gateway;
mod git_gutter;
mod host_session;
mod inlay_hints;
pub mod lsp;
mod lsp_ui;
mod splits;
use lsp_ui::lsp_settings;
mod keys;
mod lsp_worker;
mod render;
mod theme;
use workspace_edit::WorkspaceEdit;
mod workspace_edit;
pub(crate) use keys::handle_event;
pub use render::view;
use render::{
    apply_diagnostic_underlines, display_column, expand_tabs, push_syntax_spans, utf16_to_byte,
};
mod syntax;
use file_worker::FileWorker;
use gateway::GatewaySession;
#[cfg(target_os = "linux")]
use host_session::{HostEvent, HostSession};
use lsp_worker::LspConfig;
use lsp_worker::{LspEvent, LspWorker};
use syntax::SyntaxState;

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

struct Pane {
    buffer: Buffer,
}

/// Kind of entry created from the file tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateKind {
    File,
    Directory,
}

/// The small state machine needed by TUI-1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    /// Navigation, insert entry, command entry.
    Normal,
    /// Text insertion at the cursor.
    Insert,
    /// Rename symbol input.
    Rename { name: String },
    /// Ex-style command line.
    CommandLine { buffer: String },
    /// Incremental substring search.
    Search { query: String },
    /// Create a file or directory in the tree.
    Create {
        directory: PathBuf,
        name: String,
        kind: CreateKind,
    },
    /// Rename a tree entry.
    RenameEntry { path: PathBuf, name: String },
    /// Confirm deletion of a tree entry.
    ConfirmDelete { path: PathBuf },
}

/// Vi word motion kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordMotion {
    NextStart,
    PreviousStart,
    End,
}

/// TUI state.
#[derive(Debug, Clone)]
struct FuzzyState {
    query: String,
    entries: Vec<String>,
    filtered: Vec<usize>,
    selected: usize,
}

#[derive(Debug, Clone)]
struct CompletionItem {
    label: String,
    insert: String,
    detail: String,
    replace: Option<(usize, usize, usize, usize)>,
}

#[derive(Debug, Clone)]
struct CompletionState {
    id: u64,
    items: Vec<CompletionItem>,
    filtered: Vec<usize>,
    query: String,
    selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LspRequestKind {
    Completion,
    Definition,
    Hover,
    Rename,
    CodeAction,
    Formatting,
    DocumentSymbol,
    SignatureHelp,
    WorkspaceSymbol,
    InlayHint,
}

#[derive(Debug, Clone)]
struct CodeActionItem {
    title: String,
    edit: Option<WorkspaceEdit>,
    disabled: Option<String>,
}

#[derive(Debug, Clone)]
struct CodeActionState {
    items: Vec<CodeActionItem>,
    selected: usize,
}

#[derive(Debug, Clone)]
struct SymbolItem {
    name: String,
    detail: Option<String>,
    line: usize,
    character: usize,
    depth: usize,
}

#[derive(Debug, Clone)]
struct SymbolState {
    items: Vec<SymbolItem>,
    selected: usize,
}

#[derive(Debug, Clone)]
struct SignatureItem {
    label: String,
    documentation: String,
}

#[derive(Debug, Clone)]
struct SignatureHelpState {
    signatures: Vec<SignatureItem>,
    active_signature: usize,
    active_parameter: usize,
}

#[derive(Debug, Clone)]
struct WorkspaceSymbolItem {
    name: String,
    detail: String,
    path: PathBuf,
    line: usize,
    character: usize,
}

#[derive(Debug, Clone)]
struct WorkspaceSymbolState {
    items: Vec<WorkspaceSymbolItem>,
    filtered: Vec<usize>,
    query: String,
    selected: usize,
}

#[derive(Debug, Clone)]
struct InlayHint {
    line: usize,
    character: usize,
    label: String,
}

#[derive(Debug, Clone)]
struct TreeEntry {
    path: PathBuf,
    depth: usize,
    is_dir: bool,
    expanded: bool,
}

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
    /// Full-editor start dashboard, shown until the first interaction or file open.
    welcome: bool,
    /// Dashboard state to restore after closing the help page.
    welcome_back: bool,
    /// Buffer replaced by the read-only help page.
    help_back: Option<Buffer>,
    /// All open buffers, including the active one after the first switch.
    buffers: Vec<Buffer>,
    /// Active buffer index.
    active: usize,
    /// Background directory and file enumerator.
    file_worker: Option<FileWorker>,
    /// Git gutter marks keyed by zero-based line.
    git_marks: HashMap<usize, char>,
    /// Requested git diff path and revision.
    git_pending: Option<(PathBuf, u64)>,
    /// Last requested git diff path and revision.
    git_checked: Option<(PathBuf, u64)>,
    /// Debounce deadline for the next git diff request.
    git_due: Option<Instant>,
    /// LSP requests waiting for responses.
    pending_lsp: HashMap<u64, LspRequestKind>,
    pending_edit_revision: HashMap<u64, u64>,
    pending_edit_path: HashMap<u64, PathBuf>,
    next_lsp_request: u64,
    completion: Option<CompletionState>,
    hover: Option<String>,
    diagnostic_popup: Option<String>,
    code_actions: Option<CodeActionState>,
    document_symbols: Option<SymbolState>,
    signature_help: Option<SignatureHelpState>,
    workspace_symbols: Option<WorkspaceSymbolState>,
    inlay_hints: Vec<InlayHint>,
    inlay_pending: Option<(u64, PathBuf, u64, usize, usize)>,
    inlay_checked: Option<(PathBuf, u64, usize, usize)>,
    /// Secondary pane when a vertical split is open.
    split: Option<Pane>,
    /// Whether the active pane is the right-hand split.
    split_active: bool,
    /// Flattened workspace tree, when open.
    tree: Vec<TreeEntry>,
    tree_selected: Option<usize>,
    tree_visible: bool,
    /// Entry to select after the next tree refresh.
    tree_pending_select: Option<PathBuf>,
    tree_focus: bool,
    show_hidden: bool,
    fuzzy: Option<FuzzyState>,
    /// Gateway client used for file I/O when present.
    gateway: Option<GatewayClient>,
    /// Owning child/socket for the current gateway client.
    gateway_session: Option<GatewaySession>,
    /// Second authenticated gateway connection owned by the LSP worker.
    lsp_client: Option<GatewayClient>,
    /// Active language-server worker.
    lsp_worker: Option<LspWorker>,
    /// Tool and language id currently owned by `lsp_worker`.
    lsp_config: Option<(String, String)>,
    /// Number of automatic LSP restarts for this buffer.
    lsp_retries: u8,
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
        let welcome = path.is_none() && editor.text().len_bytes() == 0;
        Self {
            editor,
            path,
            welcome,
            welcome_back: false,
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
            file_worker: None,
            git_marks: HashMap::new(),
            git_pending: None,
            git_checked: None,
            git_due: None,
            pending_lsp: HashMap::new(),
            pending_edit_revision: HashMap::new(),
            pending_edit_path: HashMap::new(),
            next_lsp_request: 2,
            completion: None,
            hover: None,
            diagnostic_popup: None,
            code_actions: None,
            document_symbols: None,
            signature_help: None,
            workspace_symbols: None,
            inlay_hints: Vec::new(),
            inlay_pending: None,
            inlay_checked: None,
            split: None,
            split_active: false,
            tree: Vec::new(),
            tree_selected: None,
            tree_visible: false,
            tree_focus: false,
            tree_pending_select: None,
            show_hidden: false,
            fuzzy: None,
            gateway: None,
            gateway_session: None,
            lsp_client: None,
            lsp_worker: None,
            lsp_config: None,
            lsp_retries: 0,
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

    /// Opens a workspace file, preserving an edited active buffer as a snapshot.
    /// Builds a gateway-backed model by reading a UTF-8 file.
    /// Builds a model by reading a UTF-8 file.
    ///
    /// Invalid UTF-8 is rejected instead of being lossily replaced, because
    /// saving a lossy view would silently corrupt the original bytes.
    /// Dispatches one terminal event through the normal input path.
    ///
    /// Used by tests and benchmarks; terminal loops read events themselves.
    pub fn dispatch(&mut self, event: Event) {
        handle_event(self, event);
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
    /// Moves the cursor one line while keeping the viewport independent.
    /// Moves the cursor one grapheme-cluster boundary.
    /// Moves to the start of the cursor line.
    /// Moves to the end of the cursor line.
    /// Keeps the cursor inside the viewport.
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
        self.welcome_back = self.welcome;
        self.welcome = false;
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
        self.welcome = std::mem::take(&mut self.welcome_back);
        self.pending = None;
        self.status = "Returned from help".to_owned();
        self.dirty = true;
        true
    }

    /// Inserts text at the cursor.
    /// Inserts the unnamed register at the cursor.
    fn jump_to(&mut self, offset: usize) {
        self.cursor = offset;
        self.editor.set_selection_bytes(offset, offset);
        self.dirty = true;
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

    pub(crate) fn sync_lsp_active(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let Some(settings) = lsp_settings(&path) else {
            if self.lsp_worker.is_some() {
                self.stop_lsp();
            }
            return;
        };
        let desired = (settings.tool.clone(), settings.language_id.clone());
        if self.lsp_worker.is_some() {
            if self.lsp_config.as_ref() != Some(&desired) {
                self.stop_lsp();
                self.start_lsp();
                return;
            }
            let text = self.editor.text().slice(0..self.editor.text().len_bytes());
            if let Some(worker) = self.lsp_worker.as_ref() {
                let _ = worker.sync(&path, text);
            }
        }
    }

    fn start_lsp(&mut self) {
        let Some(path) = self.path.clone() else {
            self.status = "E: lsp requires an open file".to_owned();
            self.dirty = true;
            return;
        };
        let Some(settings) = lsp_settings(&path) else {
            self.status = format!("E: no language server for {}", path.display());
            self.dirty = true;
            return;
        };
        let desired = (settings.tool.clone(), settings.language_id.clone());
        if self.lsp_worker.is_some() {
            if self.lsp_config.as_ref() == Some(&desired) {
                self.sync_lsp_active();
                self.status = "lsp already running".to_owned();
            } else {
                self.stop_lsp();
                self.start_lsp();
            }
            self.dirty = true;
            return;
        }
        let Some(client) = self.lsp_client.take().or_else(|| {
            self.gateway_session
                .as_mut()
                .and_then(|session| session.connect_lsp().ok())
        }) else {
            self.status = "E: lsp gateway connection unavailable".to_owned();
            self.dirty = true;
            return;
        };
        let text = self.editor.text().slice(0..self.editor.text().len_bytes());
        let config = LspConfig {
            tool: settings.tool,
            args: settings.args,
            language_id: settings.language_id,
            root_markers: settings
                .root_markers
                .iter()
                .map(|marker| (*marker).to_owned())
                .collect(),
        };
        match LspWorker::start(client, config, self.workspace.clone(), path, text) {
            Ok(worker) => {
                self.lsp_worker = Some(worker);
                self.lsp_config = Some(desired);
                self.clear_lsp_requests();
                self.completion = None;
                self.hover = None;
                self.diagnostic_popup = None;
                self.code_actions = None;
                self.document_symbols = None;
                self.status = if settings.label == "jdtls" {
                    "jdtls starting (indexing workspace)".to_owned()
                } else {
                    format!("{} starting", settings.label)
                };
            }
            Err(error) => self.status = format!("E: lsp start: {error}"),
        }
        self.dirty = true;
    }

    fn stop_lsp(&mut self) {
        self.lsp_worker = None;
        self.lsp_config = None;
        self.lsp_retries = 0;
        self.clear_lsp_requests();
        self.completion = None;
        self.hover = None;
        self.diagnostic_popup = None;
        self.code_actions = None;
        self.document_symbols = None;
        self.status = "lsp stopped".to_owned();
        self.dirty = true;
    }

    fn poll_lsp(&mut self) {
        let events = self
            .lsp_worker
            .as_ref()
            .map(|worker| worker.poll())
            .unwrap_or_default();
        for event in events {
            match event {
                LspEvent::Diagnostics(path, diagnostics) => {
                    self.diagnostics.insert(path, diagnostics);
                    self.status = format!(
                        "lsp diagnostics: {}",
                        self.diagnostics.values().map(Vec::len).sum::<usize>()
                    );
                }
                LspEvent::Crashed(error) => {
                    self.lsp_worker = None;
                    self.lsp_config = None;
                    self.clear_lsp_requests();
                    self.completion = None;
                    self.hover = None;
                    self.diagnostic_popup = None;
                    self.code_actions = None;
                    self.document_symbols = None;
                    if self.lsp_retries < 3 {
                        self.lsp_retries += 1;
                        if self.lsp_client.is_none() {
                            self.lsp_client = self
                                .gateway_session
                                .as_mut()
                                .and_then(|session| session.connect_lsp().ok());
                        }
                        self.status = format!("lsp crashed, restarting ({}/3)", self.lsp_retries);
                        self.start_lsp();
                    } else {
                        self.status = format!("E: lsp crashed: {error}");
                    }
                }
                LspEvent::Response { id, result, error } => {
                    self.handle_lsp_response(id, result, error);
                }
                LspEvent::StartupError(error) => {
                    self.lsp_worker = None;
                    self.lsp_config = None;
                    self.clear_lsp_requests();
                    self.completion = None;
                    self.hover = None;
                    self.diagnostic_popup = None;
                    self.code_actions = None;
                    self.document_symbols = None;
                    self.status = format!("E: lsp startup failed: {error}");
                }
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

    fn diagnostic_ranges(
        &self,
        line_number: usize,
        line_start: usize,
        line_end: usize,
    ) -> Vec<(usize, usize)> {
        let Some(path) = &self.path else {
            return Vec::new();
        };
        let Some(diagnostics) = self.diagnostics.get(path) else {
            return Vec::new();
        };
        let line = self.editor.text().slice(line_start..line_end);
        diagnostics
            .iter()
            .filter_map(|diagnostic| {
                if diagnostic.range.start.line as usize > line_number
                    || (diagnostic.range.end.line as usize) < line_number
                {
                    return None;
                }
                let start_units = if diagnostic.range.start.line as usize == line_number {
                    diagnostic.range.start.character as usize
                } else {
                    0
                };
                let end_units = if diagnostic.range.end.line as usize == line_number {
                    diagnostic.range.end.character as usize
                } else {
                    line.chars().map(char::len_utf16).sum()
                };
                let start_byte = utf16_to_byte(&line, start_units);
                let end_byte = utf16_to_byte(&line, end_units)
                    .max(start_byte + 1)
                    .min(line.len());
                let start = display_column(&line[..start_byte]);
                let end = display_column(&line[..end_byte]).max(start + 1);
                Some((start, end))
            })
            .collect()
    }

    fn reconnect_gateway(&mut self) {
        if !self.require_gateway {
            self.status = "E: gateway is not required in this session".to_owned();
            self.dirty = true;
            return;
        }
        self.file_worker = None;
        self.lsp_worker = None;
        self.lsp_client = None;
        self.lsp_config = None;
        self.clear_lsp_requests();
        self.completion = None;
        self.hover = None;
        self.diagnostic_popup = None;
        self.code_actions = None;
        self.document_symbols = None;
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

    fn gutter_digits(&self) -> usize {
        self.editor
            .index()
            .len_lines()
            .max(1)
            .to_string()
            .len()
            .max(3)
    }

    fn gutter_width(&self) -> u16 {
        (self.gutter_digits() + 2) as u16
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
        let digits = self.gutter_digits();
        let mut lines = Vec::with_capacity(end.saturating_sub(self.scroll));
        for line_number in self.scroll..end {
            let display = match current {
                Some(current) if current != line_number => current.abs_diff(line_number),
                _ => line_number + 1,
            };
            let marker = self.git_marker(line_number).unwrap_or(' ');
            let number = format!("{display:>digits$} ");
            let mut style = Style::default().fg(theme::MUTED);
            if Some(line_number) == current {
                style = theme::current_line();
            }
            let marker_style = match marker {
                '+' => Style::default().fg(theme::SUCCESS),
                '-' => Style::default().fg(theme::ERROR),
                '~' => Style::default().fg(theme::WARNING),
                _ => Style::default(),
            };
            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), marker_style),
                Span::styled(number, style),
            ]));
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
                            theme::mode_badge(theme::WARNING)
                        } else {
                            theme::selected()
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
            self.insert_inlay_hints(line_number, start, line.end, &mut spans);
            expand_tabs(&mut spans);
            let ranges = self.diagnostic_ranges(line_number, start, line.end);
            apply_diagnostic_underlines(&mut spans, &ranges);
            let mut rendered = Line::from(spans);
            if Some(line_number) == current {
                rendered = rendered.style(theme::current_line());
            }
            if diagnostic_lines.contains(&line_number) && ranges.is_empty() {
                rendered = rendered.style(
                    Style::default()
                        .fg(theme::ERROR)
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

/// Renders the current viewport and cursor.
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
        model.poll_file_worker();
        model.poll_inlay_hints(size.height.saturating_sub(2));
        model.poll_git();
        if event::poll(std::time::Duration::from_millis(50))? {
            handle_event(&mut model, event::read()?);
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
mod tests;

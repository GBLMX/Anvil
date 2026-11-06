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

use anvil_core::{TextBuffer, end_word, next_word_start, previous_word_start};
use anvil_editor::EditorState;
use anvil_ipc::GatewayClient;
use anvil_session::DocumentSession;
use anvil_session::effect::{Effect, EffectQueue};
use anvil_session::keymap::KeymapContext;
use anvil_session::ui::{InputMode, UiState};
use crossterm::{
    cursor::SetCursorStyle,
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyEvent, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
        supports_keyboard_enhancement,
    },
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    style::{Modifier, Style},
    text::{Line, Span},
};

mod buffer;
mod clipboard;
mod command_palette;
mod commands;
mod editing;
mod file_io;
mod file_tree_ui;
mod file_worker;
mod gateway;
mod git_gutter;
mod icon_theme;
mod inlay_hints;
pub mod lsp;
mod lsp_ui;
mod modal;
mod mouse;
mod overlays;
mod pkg_install;
mod plugin_activation;
mod plugin_discovery;
mod plugin_document;
mod plugins;
mod repeat;
mod search;
mod search_ui;
mod splits;
mod status_line;
use lsp_ui::lsp_settings;
mod input;
mod keymap;
mod keys;
mod lsp_worker;
mod render;
mod theme;
use workspace_edit::WorkspaceEdit;
mod workspace_edit;
pub(crate) use keys::handle_event;
pub use render::view;
use render::{
    apply_diagnostic_underlines, apply_search_highlights, display_column, expand_tabs,
    push_syntax_spans, utf16_to_byte,
};
mod layout;
mod syntax;
mod terminal;
#[cfg(target_os = "linux")]
use anvil_session::plugin_session::{DEBUG_PERMISSIONS, DEBUG_PLUGIN_ID};
#[cfg(target_os = "linux")]
use anvil_session::plugin_supervisor::{
    ExecOutcome, HostState, PluginSupervisor, StartMode, StartOutcome, SupervisorEvent,
};
use file_worker::FileWorker;
use gateway::GatewaySession;
use lsp_worker::LspConfig;
use lsp_worker::{LspEvent, LspWorker};
#[cfg(target_os = "linux")]
use plugin_activation::ActivationEvent;
use syntax::SyntaxState;
pub use terminal::{run, run_with_workspace};

const MAX_OPEN_BYTES: usize = 256 * 1024 * 1024;
const EFFECT_QUEUE_CAPACITY: usize = 32;

const HELP_TEXT: &str = include_str!("help.txt");

#[derive(Clone)]
struct Buffer {
    editor: DocumentSession,
    path: Option<PathBuf>,
    cursor: usize,
    scroll: usize,
    hscroll: usize,
    modified: bool,
    missing: bool,
    visual_anchor: Option<usize>,
    visual_line: bool,
    search_query: String,
}

struct Pane {
    buffer: Buffer,
}

/// Vi word motion kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordMotion {
    NextStart,
    PreviousStart,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LastChange {
    DeleteWords {
        motion: WordMotion,
        count: usize,
    },
    DeleteLines {
        count: usize,
    },
    ChangeWord {
        motion: WordMotion,
        inserted: String,
    },
    ChangeInnerWord {
        inserted: String,
    },
    ChangeInnerQuote {
        quote: char,
        inserted: String,
    },
    ChangeAroundQuote {
        quote: char,
        inserted: String,
    },
    ChangeBracket {
        open: char,
        around: bool,
        inserted: String,
    },
    ChangeParagraph {
        around: bool,
        inserted: String,
    },
    ChangeTag {
        around: bool,
        inserted: String,
    },
    InsertAtCursor {
        text: String,
    },
    Append {
        text: String,
    },
    AppendLineEnd {
        text: String,
    },
    OpenBelow {
        text: String,
    },
    OpenAbove {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InsertOrigin {
    InsertHere,
    Append,
    AppendLineEnd,
    OpenBelow,
    OpenAbove,
    ChangeWord(WordMotion),
    ChangeInnerWord,
    ChangeInnerQuote(char),
    ChangeAroundQuote(char),
    ChangeBracket { open: char, around: bool },
    ChangeParagraph { around: bool },
    ChangeTag { around: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InsertSession {
    origin: InsertOrigin,
    text: String,
    start: usize,
    valid: bool,
}

/// TUI state.
#[derive(Debug, Clone)]
struct FuzzyState {
    query: String,
    entries: Vec<String>,
    filtered: Vec<usize>,
    selected: usize,
}

#[derive(Debug, Clone, Default)]
struct PreviewState {
    path: Option<String>,
    lines: Vec<String>,
    message: Option<String>,
    loading: bool,
    truncated: bool,
}

#[derive(Debug, Clone)]
struct CommandPaletteItem {
    id: String,
    label: String,
}

#[derive(Debug, Clone)]
struct CommandPaletteState {
    query: String,
    items: Vec<CommandPaletteItem>,
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
    ino: u64,
    expanded: bool,
}

#[cfg(target_os = "linux")]
fn startup_activation_allowed() -> bool {
    std::env::var_os("ANVIL_PLUGIN_ALLOW_STARTUP")
        .is_some_and(|value| value == "1" || value == "true")
}

#[cfg(target_os = "linux")]
fn workspace_activation_enabled() -> bool {
    std::env::var_os("ANVIL_PLUGIN_DISABLE_WORKSPACE_ACTIVATION").is_none()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModalLayer {
    Surround,
}

pub struct Model {
    /// Editor state and its shared text/index.
    pub editor: DocumentSession,
    /// Optional source path.
    pub path: Option<PathBuf>,
    /// Cursor byte offset.
    pub cursor: usize,
    /// Anchor for the selection-first visual adapter.
    visual_anchor: Option<usize>,
    /// Whether the visual adapter selects whole lines.
    visual_line: bool,
    modal_layer: Option<ModalLayer>,
    /// First visible line, independent of the cursor.
    pub scroll: usize,
    /// First visible display column.
    pub hscroll: usize,
    /// Input state.
    pub ui: UiState,
    /// Last successful semantic change for `.`.
    last_change: Option<LastChange>,
    /// Text captured by the current insert session.
    insert_session: Option<InsertSession>,
    /// Typed effects produced by command dispatch.
    effects: EffectQueue,
    /// In-memory keymap overrides.
    keymap: keymap::KeymapConfig,
    /// Set by `q` / `:q`.
    pub quit: bool,
    /// Whether the file has unsaved changes.
    pub modified: bool,
    /// Whether the active file disappeared outside the editor.
    pub missing: bool,
    /// Whether the next loop iteration should render.
    pub dirty: bool,
    /// Short status line.
    pub status: String,
    /// Background file operation label, if any.
    busy: Option<String>,
    busy_progress: Option<u64>,
    /// Spinner frame for the busy label.
    spinner: usize,
    viewport_height: u16,
    viewport_width: u16,
    split_ratio: u16,
    pointer: mouse::PointerState,
    /// Pending normal-mode operator (`d` or `y`).
    pending: Option<char>,
    /// Deadline for a pending Space leader.
    leader_deadline: Option<Instant>,
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
    search: search::SearchState,
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
    code_actions: Option<CodeActionState>,
    document_symbols: Option<SymbolState>,
    signature_help: Option<SignatureHelpState>,
    workspace_symbols: Option<WorkspaceSymbolState>,
    inlay_hints: Vec<InlayHint>,
    inlay_pending: Option<(u64, PathBuf, u64, usize, usize)>,
    inlay_checked: Option<(PathBuf, u64, usize, usize)>,
    inlay_support: HashMap<(String, String), bool>,
    /// Secondary pane when a vertical split is open.
    split: Option<Pane>,
    /// Flattened workspace tree, when open.
    tree: Vec<TreeEntry>,
    tree_selected: Option<usize>,
    tree_visible: bool,
    /// Entry to select after the next tree refresh.
    tree_pending_select: Option<PathBuf>,
    tree_stale: bool,
    tree_watch_limited: bool,
    tree_refresh_in_flight: bool,
    tree_refresh_queued: bool,
    tree_refresh_generation: u64,
    show_hidden: bool,
    fuzzy: Option<FuzzyState>,
    preview: PreviewState,
    preview_generation: u64,
    command_palette: Option<CommandPaletteState>,
    command_palette_recents: Vec<String>,
    /// Gateway client used for file I/O when present.
    gateway: Option<GatewayClient>,
    /// Owning child/socket for the current gateway client.
    gateway_session: Option<GatewaySession>,
    /// Second authenticated gateway connection owned by the LSP worker.
    lsp_client: Option<GatewayClient>,
    /// Language-server workers keyed by `(tool, language_id)`.
    lsp_workers: HashMap<(String, String), LspWorker>,
    /// Active language-server key used by the UI.
    lsp_config: Option<(String, String)>,
    /// Automatic restart counts per language server.
    lsp_retries: HashMap<(String, String), u8>,
    /// Diagnostics keyed by workspace path.
    diagnostics: HashMap<PathBuf, Vec<lsp::Diagnostic>>,
    diagnostic_count_shown: usize,
    diagnostic_count_pending: Option<(usize, Instant)>,
    diagnostic_path: Option<PathBuf>,
    git_branch: Option<String>,
    git_branch_checked: bool,
    /// Workspace root used to resolve all file paths.
    workspace: PathBuf,
    /// Reject file operations instead of falling back when the gateway is absent.
    require_gateway: bool,
    /// Tree-sitter highlights cached by editor revision and path.
    syntax: RefCell<SyntaxState>,
    /// Discovered local plugins.
    #[cfg(target_os = "linux")]
    plugins: Vec<plugin_discovery::PluginDescriptor>,
    /// Plugin host supervisor keyed by plugin id.
    #[cfg(target_os = "linux")]
    plugin_supervisor: PluginSupervisor,
    pkg_install: Option<pkg_install::PkgInstallState>,
}

impl Model {
    /// Builds a model from text and an optional source path.
    pub fn new(editor: EditorState, path: Option<PathBuf>) -> Self {
        let welcome = path.is_none() && editor.text().len_bytes() == 0;
        Self {
            editor: DocumentSession::from(editor),
            path,
            welcome,
            welcome_back: false,
            cursor: 0,
            visual_anchor: None,
            visual_line: false,
            modal_layer: None,
            scroll: 0,
            hscroll: 0,
            ui: UiState::default(),
            last_change: None,
            insert_session: None,
            effects: EffectQueue::new(EFFECT_QUEUE_CAPACITY),
            keymap: keymap::KeymapConfig::default(),
            quit: false,
            modified: false,
            missing: false,
            dirty: true,
            status: String::new(),
            busy: None,
            busy_progress: None,
            spinner: 0,
            viewport_height: 24,
            viewport_width: 80,
            split_ratio: 50,
            pointer: mouse::PointerState::Idle,
            pending: None,
            leader_deadline: None,
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
            search: search::SearchState::default(),
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
            code_actions: None,
            document_symbols: None,
            signature_help: None,
            workspace_symbols: None,
            inlay_hints: Vec::new(),
            inlay_pending: None,
            inlay_checked: None,
            inlay_support: HashMap::new(),
            split: None,
            tree: Vec::new(),
            tree_selected: None,
            tree_visible: false,
            tree_pending_select: None,
            tree_stale: false,
            tree_watch_limited: false,
            tree_refresh_in_flight: false,
            tree_refresh_queued: false,
            tree_refresh_generation: 0,
            show_hidden: false,
            fuzzy: None,
            preview: PreviewState::default(),
            preview_generation: 0,
            command_palette: None,
            command_palette_recents: Vec::new(),
            gateway: None,
            gateway_session: None,
            lsp_client: None,
            lsp_workers: HashMap::new(),
            lsp_config: None,
            lsp_retries: HashMap::new(),
            diagnostics: HashMap::new(),
            diagnostic_count_shown: 0,
            diagnostic_count_pending: None,
            diagnostic_path: None,
            git_branch: None,
            git_branch_checked: false,
            workspace: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            require_gateway: false,
            syntax: RefCell::new(SyntaxState::default()),
            #[cfg(target_os = "linux")]
            plugins: Vec::new(),
            #[cfg(target_os = "linux")]
            plugin_supervisor: PluginSupervisor::new(),
            pkg_install: None,
        }
    }

    pub(crate) fn load_keymap_config_from(&mut self, path: &Path) -> Result<(), String> {
        let config = keymap::KeymapConfig::from_path(path)?;
        self.keymap = config;
        Ok(())
    }

    pub(crate) fn load_keymap_config(&mut self) -> Result<(), String> {
        if let Some(path) = keymap::KeyChord::default_path()
            && path.exists()
        {
            return self.load_keymap_config_from(&path);
        }
        if let Some(preset) = std::env::var_os("ANVIL_KEYMAP_PRESET") {
            self.keymap = keymap::KeymapConfig::from_preset(&preset.to_string_lossy())?;
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn load_plugins(&mut self) -> Result<(), String> {
        self.plugins = plugin_discovery::discover()?;
        Ok(())
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

    fn begin_pending(&mut self, pending: char) {
        self.pending = Some(pending);
        self.leader_deadline =
            (pending == ' ').then(|| Instant::now() + Duration::from_millis(600));
        self.dirty = true;
    }

    fn expire_leader(&mut self) {
        let Some(deadline) = self.leader_deadline else {
            return;
        };
        if self.pending == Some(' ') && Instant::now() >= deadline {
            self.pending = None;
            self.leader_deadline = None;
            self.dirty = true;
        }
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
        let editor = std::mem::replace(&mut self.editor, EditorState::from_text(HELP_TEXT).into());
        let saved = Buffer {
            editor,
            path: self.path.take(),
            cursor: self.cursor,
            scroll: self.scroll,
            hscroll: self.hscroll,
            visual_anchor: self.visual_anchor,
            visual_line: self.visual_line,
            modified: self.modified,
            missing: self.missing,
            search_query: std::mem::take(&mut self.search_query),
        };
        self.help_back = Some(saved);
        self.cursor = 0;
        self.scroll = 0;
        self.hscroll = 0;
        self.modified = false;
        self.welcome_back = self.welcome;
        self.welcome = false;
        self.ui.input_mode = InputMode::Normal;
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
        self.syntax
            .borrow_mut()
            .switch_path(self.path.as_deref(), false);
        self.ui.input_mode = InputMode::Normal;
        self.welcome = std::mem::take(&mut self.welcome_back);
        self.pending = None;
        self.status = "Returned from help".to_owned();
        self.dirty = true;
        true
    }

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

    fn clear_lsp_ui(&mut self) {
        self.clear_lsp_requests();
        self.completion = None;
        self.hover = None;
        self.ui.diagnostic_peek = None;
        self.code_actions = None;
        self.document_symbols = None;
    }

    fn active_lsp_worker(&self) -> Option<&LspWorker> {
        self.lsp_config
            .as_ref()
            .and_then(|key| self.lsp_workers.get(key))
    }

    fn lsp_key(path: &Path) -> Option<(String, String)> {
        lsp_settings(path).map(|settings| (settings.tool, settings.language_id))
    }

    pub(crate) fn sync_lsp_active(&mut self) {
        let Some(path) = self.path.clone() else {
            self.lsp_config = None;
            self.clear_lsp_ui();
            return;
        };
        let Some(settings) = lsp_settings(&path) else {
            self.lsp_config = None;
            self.clear_lsp_ui();
            return;
        };
        self.activate_plugins(ActivationEvent::Language(settings.language_id.clone()));
        let desired = (settings.tool.clone(), settings.language_id.clone());
        if !self.lsp_workers.contains_key(&desired) {
            self.start_lsp();
            return;
        }
        if self.lsp_config.as_ref() != Some(&desired) {
            self.lsp_config = Some(desired.clone());
            self.clear_lsp_ui();
        }
        let text = self.editor.text().slice(0..self.editor.text().len_bytes());
        if let Some(worker) = self.active_lsp_worker() {
            let _ = worker.sync(&path, text);
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
        if self.lsp_workers.contains_key(&desired) {
            self.lsp_config = Some(desired.clone());
            self.clear_lsp_ui();
            let text = self.editor.text().slice(0..self.editor.text().len_bytes());
            if let Some(worker) = self.lsp_workers.get(&desired) {
                let _ = worker.sync(&path, text);
            }
            self.status = "lsp already running".to_owned();
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
                self.lsp_workers.insert(desired.clone(), worker);
                self.lsp_config = Some(desired.clone());
                self.lsp_retries.entry(desired).or_insert(0);
                self.clear_lsp_ui();
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
        if let Some(key) = self.lsp_config.take() {
            self.lsp_workers.remove(&key);
            self.lsp_retries.remove(&key);
            self.inlay_support.remove(&key);
        }
        self.clear_lsp_ui();
        self.status = "lsp stopped".to_owned();
        self.dirty = true;
    }

    fn handle_lsp_event(&mut self, key: &(String, String), event: LspEvent) {
        let active = self.lsp_config.as_ref() == Some(key);
        match event {
            LspEvent::Capabilities { inlay_hints } => {
                self.inlay_support.insert(key.clone(), inlay_hints);
                if active {
                    self.inlay_hints.clear();
                    self.inlay_pending = None;
                    self.inlay_checked = None;
                }
            }
            LspEvent::Diagnostics(path, diagnostics) => {
                self.diagnostics.insert(path, diagnostics);
                self.status = format!(
                    "lsp diagnostics: {}",
                    self.diagnostics.values().map(Vec::len).sum::<usize>()
                );
                self.rebuild_problems();
            }
            LspEvent::Crashed(error) => {
                self.lsp_workers.remove(key);
                if active {
                    self.lsp_config = None;
                    self.clear_lsp_ui();
                    let retries = {
                        let retries = self.lsp_retries.entry(key.clone()).or_insert(0);
                        *retries += 1;
                        *retries
                    };
                    if retries <= 3 {
                        self.status = format!("lsp crashed, restarting ({retries}/3)");
                        self.start_lsp();
                    } else {
                        self.status = format!("E: lsp crashed: {error}");
                        self.notify(
                            anvil_session::ui::NotificationSeverity::Error,
                            "lsp",
                            format!("crashed: {error}"),
                        );
                    }
                }
            }
            LspEvent::Response { id, result, error } => {
                self.handle_lsp_response(id, result, error);
            }
            LspEvent::StartupError(error) => {
                self.lsp_workers.remove(key);
                if active {
                    self.lsp_config = None;
                    self.clear_lsp_ui();
                    self.status = format!("E: lsp startup failed: {error}");
                    self.notify(
                        anvil_session::ui::NotificationSeverity::Error,
                        "lsp",
                        format!("startup failed: {error}"),
                    );
                }
            }
            LspEvent::Error(error) => {
                if active {
                    self.status = format!("E: lsp: {error}");
                    self.notify(
                        anvil_session::ui::NotificationSeverity::Error,
                        "lsp",
                        error.clone(),
                    );
                }
            }
            LspEvent::Log(line) => {
                if active {
                    self.status = format!("lsp: {line}");
                }
            }
        }
        self.dirty = true;
    }

    fn poll_lsp(&mut self) {
        let keys: Vec<(String, String)> = self.lsp_workers.keys().cloned().collect();
        for key in keys {
            let events = self
                .lsp_workers
                .get(&key)
                .map(LspWorker::poll)
                .unwrap_or_default();
            for event in events {
                self.handle_lsp_event(&key, event);
            }
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
        self.lsp_workers.clear();
        self.lsp_client = None;
        self.lsp_config = None;
        self.lsp_retries.clear();
        self.clear_lsp_ui();
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

    fn active_keymap_contexts(&self) -> Vec<KeymapContext> {
        let mut contexts = vec![KeymapContext::Global];
        contexts.push(if self.ui.focus.tree {
            KeymapContext::FileTree
        } else {
            KeymapContext::Editor
        });
        let input = match &self.ui.input_mode {
            InputMode::CommandLine { .. } => Some(KeymapContext::CommandLine),
            InputMode::Search { .. } => Some(KeymapContext::Search),
            InputMode::Rename { .. } => Some(KeymapContext::Rename),
            InputMode::Create { .. }
            | InputMode::RenameEntry { .. }
            | InputMode::ConfirmDelete { .. }
            | InputMode::ConfirmClose { .. }
            | InputMode::ConfirmReload { .. }
            | InputMode::TransferEntry { .. } => Some(KeymapContext::Popup),
            InputMode::Normal | InputMode::Insert => None,
        };
        if let Some(context) = input
            && contexts.last() != Some(&context)
        {
            contexts.push(context);
        }
        let overlay = self.command_palette.is_some()
            || self.fuzzy.is_some()
            || self.workspace_symbols.is_some()
            || self.document_symbols.is_some()
            || self.code_actions.is_some()
            || self.completion.is_some()
            || self.hover.is_some()
            || self.ui.diagnostic_peek.is_some()
            || self.ui.problems.visible
            || self.help_back.is_some();
        if overlay && contexts.last() != Some(&KeymapContext::Popup) {
            contexts.push(KeymapContext::Popup);
        }
        if self.visual_anchor.is_some() {
            contexts.push(KeymapContext::Modal);
        }
        contexts
    }

    fn keymap_context(&self) -> KeymapContext {
        self.active_keymap_contexts()
            .last()
            .copied()
            .unwrap_or(KeymapContext::Global)
    }

    fn pending_hints(&self) -> Vec<(String, String)> {
        self.pending
            .map(|prefix| self.keymap.pending_actions(prefix, self.object_pending))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;

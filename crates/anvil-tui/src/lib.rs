//! Ratatui editor shell and shared model.
//!
//! LSP behavior, file-tree state, rendering, and key dispatch live in focused
//! sibling modules; this module keeps buffer/model state and composition.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
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
use anvil_session::ui::{InputBuffer, InputMode, Problem, UiState};
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
mod command_line;
mod command_palette;
mod commands;
mod editing;
mod file_io;
mod file_tree_ui;
mod file_worker;
mod folding;
use folding::{FoldAction, FoldRange};
mod gateway;
mod git_diff;
mod git_gutter;
mod icon_theme;
use command_line::CommandCompletion;
mod code_lens;
mod inlay_hints;
pub mod lsp;
mod lsp_ui;
mod modal;
mod mouse;
mod overlays;
mod pipe_locations;
mod remote;
mod selection_table;
mod session;
mod theme_picker;
use selection_table::{SelectionItem, SelectionTable};
mod pkg_install;
mod plugin_activation;
mod plugin_discovery;
mod plugin_document;
#[cfg(target_os = "linux")]
mod plugin_manager;
mod plugins;
mod project_replace;
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
mod settings;
mod theme;
mod theme_config;
use settings::Settings;
mod settings_ui;
use settings_ui::SettingsOverlayState;
mod snippet;
use snippet::{Snippet, SnippetSession};
use workspace_edit::WorkspaceEdit;
mod semantic_tokens;
mod workspace_edit;
pub(crate) use keys::handle_event;
pub use render::view;
use render::{
    apply_diagnostic_underlines, apply_document_highlights, apply_search_highlights,
    apply_semantic_tokens, display_column, expand_tabs, push_syntax_spans, utf16_to_byte,
};
mod layout;
mod syntax;
mod task_runner;
mod task_ui;
mod terminal;
mod terminal_background;
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
pub use terminal::{run, run_with_input, run_with_workspace, run_with_workspace_status};

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
    syntax: crate::syntax::SyntaxCache,
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
    plugin_id: Option<String>,
    theme: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct CommandPaletteState {
    table: SelectionTable<CommandPaletteItem>,
    theme_picker: theme_picker::ThemePicker,
}

impl SelectionItem for CommandPaletteItem {
    fn label(&self) -> &str {
        &self.label
    }
}

#[derive(Debug, Clone)]
struct CompletionItem {
    label: String,
    filter_text: String,
    sort_text: String,
    preselect: bool,
    insert: String,
    snippet: Option<Snippet>,
    detail: String,
    replace: Option<(usize, usize, usize, usize)>,
    documentation: Option<String>,
    raw: serde_json::Value,
    resolved: bool,
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
struct SemanticToken {
    start: usize,
    end: usize,
    type_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LspRequestKind {
    Completion,
    CompletionResolve,
    SemanticTokens,
    Definition,
    References,
    DocumentHighlight,
    CallHierarchyPrepare(CallHierarchyDirection),
    CallHierarchyIncoming,
    CallHierarchyOutgoing,
    FoldingRange,
    Hover,
    Rename,
    CodeAction,
    Formatting,
    DocumentSymbol,
    SignatureHelp,
    WorkspaceSymbol,
    InlayHint,
    ExecuteCommand,
    CodeLens,
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
struct ReferenceItem {
    path: PathBuf,
    line: usize,
    character: usize,
}

#[derive(Debug, Clone)]
struct ReferenceState {
    items: Vec<ReferenceItem>,
    selected: usize,
}

#[derive(Debug, Clone)]
struct DocumentHighlightItem {
    start: usize,
    end: usize,
    kind: u8,
}

#[derive(Debug, Clone)]
struct DocumentHighlightState {
    revision: u64,
    items: Vec<DocumentHighlightItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallHierarchyDirection {
    Incoming,
    Outgoing,
}

#[derive(Debug, Clone)]
struct CallHierarchyItem {
    name: String,
    path: PathBuf,
    line: usize,
    character: usize,
}

#[derive(Debug, Clone)]
struct CallHierarchyState {
    direction: CallHierarchyDirection,
    items: Vec<CallHierarchyItem>,
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
    /// Short title for the current read-only view.
    read_only_title: Option<String>,
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
    /// Latest requested read-only Git diff generation.
    git_diff_generation: u64,
    /// Pending Git diff generation, if a request is in flight.
    git_diff_pending: Option<u64>,
    /// Parsed hunks for the active Git diff view.
    git_diff_view: Option<git_diff::GitDiffView>,
    /// Latest requested hunk-apply generation.
    git_apply_generation: u64,
    /// Pending hunk-apply generation, if a request is in flight.
    git_apply_pending: Option<u64>,
    /// Latest requested Git blame generation.
    git_blame_generation: u64,
    /// Pending Git blame generation, if a request is in flight.
    git_blame_pending: Option<u64>,
    /// Latest requested Git conflict-list generation.
    git_conflicts_generation: u64,
    /// Pending Git conflict-list generation, if a request is in flight.
    git_conflicts_pending: Option<u64>,
    /// Latest requested conflict-stage generation.
    git_stage_generation: u64,
    /// Pending conflict-stage generation, if a request is in flight.
    git_stage_pending: Option<u64>,
    /// Latest requested conflict-resolution generation.
    git_resolve_generation: u64,
    /// Pending conflict-resolution generation, if a request is in flight.
    git_resolve_pending: Option<u64>,
    /// LSP folding ranges for the active buffer.
    folds: Vec<FoldRange>,
    /// Start lines of closed folds.
    folded: HashSet<usize>,
    fold_revision: Option<u64>,
    pending_fold_action: Option<FoldAction>,
    /// LSP requests waiting for responses.
    pending_lsp: HashMap<u64, LspRequestKind>,
    pending_edit_revision: HashMap<u64, u64>,
    pending_edit_path: HashMap<u64, PathBuf>,
    pending_edit_snapshot: HashMap<u64, HashMap<PathBuf, u64>>,
    /// Last LSP document version sent for each path.
    lsp_versions: HashMap<PathBuf, u64>,
    next_lsp_request: u64,
    completion: Option<CompletionState>,
    completion_resolve: Option<(u64, usize)>,
    code_lens: Option<code_lens::CodeLensState>,
    snippet: Option<SnippetSession>,
    semantic_legend: Vec<String>,
    semantic_tokens: Vec<SemanticToken>,
    semantic_revision: Option<u64>,
    hover: Option<String>,
    code_actions: Option<CodeActionState>,
    document_symbols: Option<SymbolState>,
    references: Option<ReferenceState>,
    document_highlights: Option<DocumentHighlightState>,
    call_hierarchy: Option<CallHierarchyState>,
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
    command_completion: Option<CommandCompletion>,
    persist_theme: bool,
    settings: Settings,
    settings_overlay: Option<SettingsOverlayState>,
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
    /// Pending language-server restart after crash backoff.
    lsp_retry: Option<((String, String), Instant)>,
    /// Diagnostics keyed by workspace path.
    diagnostics: HashMap<PathBuf, Vec<lsp::Diagnostic>>,
    diagnostic_count_shown: usize,
    diagnostic_count_pending: Option<(usize, Instant)>,
    diagnostic_path: Option<PathBuf>,
    /// Dirty swap files that must survive a source conflict.
    conflict_swaps: HashMap<PathBuf, PathBuf>,
    /// Paths changed by a multi-file replace whose buffer reload failed.
    stale_paths: HashSet<PathBuf>,
    git_branch: Option<String>,
    git_branch_checked: bool,
    /// Workspace root used to resolve all file paths.
    workspace: PathBuf,
    /// Reject file operations instead of falling back when the gateway is absent.
    require_gateway: bool,
    /// The active buffer was loaded from stdin and has no file path.
    stdin_buffer: bool,
    /// Reject mutating commands for a read-only input buffer.
    read_only: bool,
    /// File locations parsed from the stdin buffer.
    stdin_locations: Vec<pipe_locations::PipeLocation>,
    stdin_location_selected: Option<usize>,
    /// Title for the generic location panel.
    location_title: String,
    /// Latest requested project-search generation.
    project_search_generation: u64,
    /// Pending project-search generation, if a request is in flight.
    project_search_pending: Option<u64>,
    /// Pending reviewed project replace plan.
    project_replace_plan: Option<file_worker::ProjectReplacePlan>,
    /// Latest requested project-replace generation.
    project_replace_generation: u64,
    /// Pending project-replace generation, if a request is in flight.
    project_replace_pending: Option<u64>,
    stdin_location_scroll: usize,
    /// Whether the process was started as an external editor.
    editor_mode: bool,
    /// Process exit code accumulated by editor-mode failures or `:cq`.
    exit_code: i32,
    /// Tree-sitter highlights cached by editor revision and path.
    syntax: RefCell<SyntaxState>,
    /// Discovered local plugins.
    #[cfg(target_os = "linux")]
    plugins: Vec<plugin_discovery::PluginDescriptor>,
    /// Plugin host supervisor keyed by plugin id.
    #[cfg(target_os = "linux")]
    plugin_supervisor: PluginSupervisor,
    #[cfg(target_os = "linux")]
    plugin_manager: Option<plugin_manager::PluginManagerState>,
    #[cfg(target_os = "linux")]
    plugin_store_size: u64,
    #[cfg(target_os = "linux")]
    installer: Option<(GatewaySession, GatewayClient)>,
    pkg_install: Option<pkg_install::PkgInstallState>,
    task_worker: Option<task_runner::TaskWorker>,
    task_output: Option<String>,
    task_problems: Vec<Problem>,
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
            read_only_title: None,
            buffers: Vec::new(),
            active: 0,
            file_worker: None,
            git_marks: HashMap::new(),
            git_pending: None,
            git_checked: None,
            git_due: None,
            git_diff_generation: 0,
            git_diff_pending: None,
            git_diff_view: None,
            git_apply_generation: 0,
            git_apply_pending: None,
            git_blame_generation: 0,
            git_blame_pending: None,
            git_conflicts_generation: 0,
            git_conflicts_pending: None,
            git_stage_generation: 0,
            git_stage_pending: None,
            git_resolve_generation: 0,
            git_resolve_pending: None,
            folds: Vec::new(),
            folded: HashSet::new(),
            fold_revision: None,
            pending_fold_action: None,
            pending_lsp: HashMap::new(),
            pending_edit_revision: HashMap::new(),
            pending_edit_path: HashMap::new(),
            pending_edit_snapshot: HashMap::new(),
            lsp_versions: HashMap::new(),
            next_lsp_request: 2,
            completion: None,
            code_lens: None,
            completion_resolve: None,
            snippet: None,
            semantic_legend: Vec::new(),
            semantic_tokens: Vec::new(),
            semantic_revision: None,
            hover: None,
            code_actions: None,
            document_symbols: None,
            references: None,
            document_highlights: None,
            call_hierarchy: None,
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
            command_completion: None,
            persist_theme: !cfg!(test),
            settings: Settings::default(),
            settings_overlay: None,
            command_palette_recents: Vec::new(),
            gateway: None,
            gateway_session: None,
            lsp_client: None,
            lsp_workers: HashMap::new(),
            lsp_config: None,
            lsp_retries: HashMap::new(),
            lsp_retry: None,
            diagnostics: HashMap::new(),
            diagnostic_count_shown: 0,
            diagnostic_count_pending: None,
            diagnostic_path: None,
            conflict_swaps: HashMap::new(),
            stale_paths: HashSet::new(),
            git_branch: None,
            git_branch_checked: false,
            workspace: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            require_gateway: false,
            stdin_buffer: false,
            read_only: false,
            stdin_locations: Vec::new(),
            stdin_location_selected: None,
            stdin_location_scroll: 0,
            location_title: "Locations".to_owned(),
            project_search_generation: 0,
            project_search_pending: None,
            project_replace_plan: None,
            project_replace_generation: 0,
            project_replace_pending: None,
            editor_mode: false,
            exit_code: 0,
            syntax: RefCell::new(SyntaxState::default()),
            #[cfg(target_os = "linux")]
            plugins: Vec::new(),
            #[cfg(target_os = "linux")]
            plugin_supervisor: PluginSupervisor::new(),
            #[cfg(target_os = "linux")]
            plugin_manager: None,
            #[cfg(target_os = "linux")]
            plugin_store_size: 0,
            #[cfg(target_os = "linux")]
            installer: None,
            pkg_install: None,
            task_worker: None,
            task_output: None,
            task_problems: Vec::new(),
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
        let mut plugins = plugin_discovery::discover_bundled()?;
        let local = plugin_discovery::discover()?;
        let mut conflicts = plugin_discovery::merge_local(&mut plugins, local);
        let managed = if let Some(gateway) = self.gateway.as_mut() {
            match gateway.call("pkg.installed", serde_json::json!({})) {
                Ok(value) => {
                    self.plugin_store_size = value
                        .get("store_size")
                        .and_then(|size| size.as_u64())
                        .unwrap_or(0);
                    plugin_discovery::discover_managed(&value)
                }
                Err(error) => Err(format!("pkg.installed: {error}")),
            }
        } else {
            Ok(Vec::new())
        };
        let managed_conflicts = match managed {
            Ok(managed) => plugin_discovery::merge_managed(&mut plugins, managed),
            Err(error) => {
                self.plugins = plugins;
                return Err(error);
            }
        };
        conflicts.extend(managed_conflicts);
        self.plugins = plugins;
        if let Some(manager) = self.plugin_manager.as_mut() {
            manager.selected = manager.selected.min(self.plugins.len().saturating_sub(1));
        }
        if !conflicts.is_empty() {
            return Err(format!("duplicate plugin ids: {}", conflicts.join(", ")));
        }
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
        if self.read_only {
            self.status = "E: read-only buffer".to_owned();
            self.dirty = true;
            true
        } else if self.help_back.is_some() {
            self.status = "E: read-only view".to_owned();
            self.dirty = true;
            true
        } else {
            false
        }
    }

    fn clear_git_gutter(&mut self) {
        self.git_marks.clear();
        self.git_pending = None;
        self.git_checked = None;
        self.git_due = None;
    }

    fn show_help(&mut self) {
        self.show_read_only(HELP_TEXT, "Help: :q returns", "help");
    }

    fn show_read_only(&mut self, text: &str, status: &str, title: &str) -> bool {
        if self.help_back.is_some() {
            self.status = "already viewing a read-only view".to_owned();
            self.dirty = true;
            return false;
        }
        let editor = std::mem::replace(&mut self.editor, EditorState::from_text(text).into());
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
            syntax: self.syntax.borrow().active_cache(),
        };
        self.read_only_title = Some(title.to_owned());
        self.help_back = Some(saved);
        self.clear_git_gutter();
        self.syntax.borrow_mut().switch_path(None, true);
        self.cursor = 0;
        self.scroll = 0;
        self.hscroll = 0;
        self.modified = false;
        self.welcome_back = self.welcome;
        self.welcome = false;
        self.ui.input_mode = InputMode::Normal;
        self.pending = None;
        self.status = status.to_owned();
        self.dirty = true;
        true
    }

    fn replace_read_only(&mut self, text: &str, status: &str, title: &str) {
        if self.help_back.is_none() {
            self.show_read_only(text, status, title);
            return;
        }
        self.clear_git_gutter();
        self.editor = EditorState::from_text(text).into();
        self.syntax.borrow_mut().switch_path(None, true);
        self.cursor = 0;
        self.scroll = 0;
        self.hscroll = 0;
        self.modified = false;
        self.read_only_title = Some(title.to_owned());
        self.status = status.to_owned();
        self.dirty = true;
    }

    fn close_help(&mut self) -> bool {
        let Some(saved) = self.help_back.take() else {
            return false;
        };
        let title = self
            .read_only_title
            .take()
            .unwrap_or_else(|| "help".to_owned());
        self.git_diff_view = None;
        self.git_diff_pending = None;
        self.git_apply_pending = None;
        self.git_blame_pending = None;
        self.git_conflicts_pending = None;
        self.git_stage_pending = None;
        self.git_resolve_pending = None;
        let syntax = saved.syntax.clone();
        self.editor = saved.editor;
        self.path = saved.path;
        self.cursor = saved.cursor;
        self.scroll = saved.scroll;
        self.hscroll = saved.hscroll;
        self.modified = saved.modified;
        self.search_query = saved.search_query;
        self.syntax
            .borrow_mut()
            .switch_path_with(self.path.as_deref(), false, Some(syntax));
        self.ui.input_mode = InputMode::Normal;
        self.welcome = std::mem::take(&mut self.welcome_back);
        self.pending = None;
        self.status = format!("Returned from {title}");
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
        self.code_lens = None;
        self.document_symbols = None;
        self.references = None;
        self.document_highlights = None;
        self.call_hierarchy = None;
        self.semantic_legend.clear();
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
            if self.active_lsp_worker().is_some() {
                self.request_folding_ranges(None);
                self.request_code_lens();
            }
            return;
        }
        if self.lsp_config.as_ref() != Some(&desired) {
            self.lsp_config = Some(desired.clone());
            self.clear_lsp_ui();
        }
        let text = self.editor.text().slice(0..self.editor.text().len_bytes());
        let has_worker = self.sync_lsp_document(&path, text).is_ok();
        if has_worker {
            self.request_semantic_tokens();
            self.request_folding_ranges(None);
            self.request_code_lens();
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
            let _ = self.sync_lsp_document(&path, text);
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
            initialization_options: settings.initialization_options,
        };
        match LspWorker::start(client, config, self.workspace.clone(), path.clone(), text) {
            Ok(worker) => {
                self.lsp_config = Some(desired.clone());
                self.lsp_workers.insert(desired.clone(), worker);
                self.lsp_versions.insert(path.clone(), 1);
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
            LspEvent::Capabilities {
                inlay_hints,
                semantic_token_types,
                server_info,
            } => {
                self.lsp_retries.remove(key);
                self.lsp_retry = None;
                self.inlay_support.insert(key.clone(), inlay_hints);
                if active {
                    if let Some(info) = server_info {
                        self.status = format!("lsp: {info}");
                    }
                    self.inlay_hints.clear();
                    self.inlay_pending = None;
                    self.inlay_checked = None;
                    self.semantic_legend = semantic_token_types;
                    self.semantic_tokens.clear();
                    self.semantic_revision = None;
                    self.request_semantic_tokens();
                }
            }
            LspEvent::Diagnostics(path, diagnostics) => {
                self.diagnostics.insert(path, diagnostics);
                self.status = format!(
                    "lsp diagnostics: {}",
                    self.diagnostics.values().map(Vec::len).sum::<usize>()
                );
                self.rebuild_problems();
                if active
                    && !self.semantic_legend.is_empty()
                    && self.semantic_revision != Some(self.editor.revision())
                {
                    self.request_semantic_tokens();
                }
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
                        let delay_ms = 250u64 << (retries.saturating_sub(1).min(3));
                        self.lsp_retry = Some((
                            key.clone(),
                            Instant::now() + Duration::from_millis(delay_ms),
                        ));
                        self.status =
                            format!("lsp crashed; restarting in {delay_ms}ms ({retries}/3)");
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

    pub(crate) fn poll_lsp_retry(&mut self) {
        let Some((key, deadline)) = self.lsp_retry.clone() else {
            return;
        };
        if Instant::now() < deadline {
            return;
        }
        self.lsp_retry = None;
        if !self.lsp_workers.contains_key(&key) {
            self.start_lsp();
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
            || self.references.is_some()
            || self.call_hierarchy.is_some()
            || self.settings_overlay.is_some()
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

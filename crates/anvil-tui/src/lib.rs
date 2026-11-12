//! Ratatui editor shell and shared model.
//!
//! LSP behavior, file-tree state, rendering, and key dispatch live in focused
//! sibling modules; this module keeps buffer/model state and composition.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    io::{self, Stdout},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant, SystemTime},
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
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyEvent, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
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

mod bracket;
mod breadcrumbs;
mod buffer;
mod clipboard;
mod command_line;
mod command_palette;
mod commands;
mod comment;
mod debug_panel;
mod debugger;
mod editing;
mod file_io;
mod file_tree_ui;
mod file_worker;
mod folding;
mod jump;
mod sticky;
use folding::{FoldAction, FoldRange};
mod gateway;
mod git_diff;
mod git_gutter;
mod icon_theme;
use command_line::CommandCompletion;
mod code_lens;
pub mod dap;
mod inlay_hints;
pub mod lsp;
mod lsp_ui;
mod modal;
mod mouse;
mod overlays;
mod pipe_locations;
#[cfg(unix)]
pub mod pty_terminal;
mod recents;
mod remote;
mod selection_table;
mod session;
mod theme_browser;
mod theme_picker;
mod workspace_control;
pub mod workspace_roots;
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
    apply_bracket_highlights, apply_diagnostic_underlines, apply_document_highlights,
    apply_search_highlights, apply_semantic_tokens, display_column, expand_tabs, push_syntax_spans,
    utf16_to_byte,
};
mod layout;
mod syntax;
mod task_runner;
mod task_ui;
mod terminal;
mod terminal_background;
#[cfg(unix)]
mod terminal_control;
mod test_explorer;
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
    generation: u64,
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
    title: String,
    detail: String,
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

struct LocationPanelState {
    stdin_locations: Vec<pipe_locations::PipeLocation>,
    stdin_location_selected: Option<usize>,
    stdin_location_scroll: usize,
    location_title: String,
}

impl Default for LocationPanelState {
    fn default() -> Self {
        Self {
            stdin_locations: Vec::new(),
            stdin_location_selected: None,
            stdin_location_scroll: 0,
            location_title: "Locations".to_owned(),
        }
    }
}

#[derive(Default)]
struct DapState {
    breakpoints: HashMap<PathBuf, BTreeSet<u32>>,
    client: Option<crate::dap::DapClient>,
    attach: Option<mpsc::Receiver<Result<crate::dap::DapClient, String>>>,
    attach_started: Option<Instant>,
    request: Option<
        mpsc::Receiver<(
            crate::dap::DapClient,
            Result<debug_panel::DebugSnapshot, String>,
        )>,
    >,
    request_started: Option<Instant>,
}

#[derive(Default)]
struct DiagnosticsState {
    by_path: HashMap<PathBuf, Vec<lsp::Diagnostic>>,
    count_shown: usize,
    count_pending: Option<(usize, Instant)>,
    path: Option<PathBuf>,
}

#[derive(Default)]
struct LspState {
    client: Option<GatewayClient>,
    workers: HashMap<(String, String), LspWorker>,
    config: Option<(String, String)>,
    retries: HashMap<(String, String), u8>,
    retry: Option<((String, String), Instant)>,
    pending: HashMap<u64, LspRequestKind>,
    pending_edit_revision: HashMap<u64, u64>,
    pending_edit_path: HashMap<u64, PathBuf>,
    pending_edit_snapshot: HashMap<u64, HashMap<PathBuf, u64>>,
    versions: HashMap<PathBuf, u64>,
    next_request: u64,
}

#[derive(Default)]
struct LspResultsState {
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
}

#[derive(Default)]
struct GitState {
    /// Git gutter marks keyed by zero-based line.
    marks: HashMap<usize, char>,
    /// Workspace git status keyed by absolute path.
    status: HashMap<PathBuf, char>,
    /// Latest requested tree git-status generation.
    status_generation: u64,
    /// Pending tree git-status generation.
    status_pending: Option<u64>,
    /// Number of roots expected to answer the pending git-status request.
    status_expected: usize,
    /// Number of roots that answered the pending git-status request.
    status_received: usize,
    /// Requested git diff path and revision.
    diff_request: Option<(PathBuf, u64)>,
    /// Last requested git diff path and revision.
    diff_checked: Option<(PathBuf, u64)>,
    /// Debounce deadline for the next git diff request.
    diff_due: Option<Instant>,
    /// Latest requested read-only Git diff generation.
    diff_generation: u64,
    /// Pending Git diff generation, if a request is in flight.
    diff_pending: Option<u64>,
    /// Parsed hunks for the active Git diff view.
    diff_view: Option<git_diff::GitDiffView>,
    /// Latest requested hunk-apply generation.
    apply_generation: u64,
    /// Pending hunk-apply generation, if a request is in flight.
    apply_pending: Option<u64>,
    /// Latest requested Git blame generation.
    blame_generation: u64,
    /// Pending Git blame generation, if a request is in flight.
    blame_pending: Option<u64>,
    /// Latest requested Git conflict-list generation.
    conflicts_generation: u64,
    /// Pending Git conflict-list generation, if a request is in flight.
    conflicts_pending: Option<u64>,
    /// Latest requested conflict-stage generation.
    stage_generation: u64,
    /// Pending conflict-stage generation, if a request is in flight.
    stage_pending: Option<u64>,
    /// Latest requested conflict-resolution generation.
    resolve_generation: u64,
    /// Pending conflict-resolution generation, if a request is in flight.
    resolve_pending: Option<u64>,
    branch: Option<String>,
    branch_checked: bool,
}

#[derive(Default)]
struct LanguageState {
    dap: DapState,
    diagnostics: DiagnosticsState,
    lsp: LspState,
}

#[derive(Default)]
struct OverlayState {
    command_palette: Option<CommandPaletteState>,
    settings_overlay: Option<SettingsOverlayState>,
    theme_browser: Option<theme_browser::ThemeBrowser>,
    fuzzy: Option<FuzzyState>,
    fuzzy_generation: u64,
    preview: PreviewState,
    preview_generation: u64,
    debug_panel: Option<debug_panel::DebugPanelState>,
    test_explorer: Option<test_explorer::TestExplorerState>,
    command_completion: Option<CommandCompletion>,
    command_palette_recents: Vec<String>,
    location: LocationPanelState,
    lsp_results: LspResultsState,
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
    StickySymbols,
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
    /// Bounded back/forward navigation history.
    jump_list: jump::JumpList,
    /// Bounded back/forward edit-position history.
    change_list: jump::JumpList,
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
    /// Register name waiting after `q` or `@`.
    macro_wait: Option<char>,
    /// Currently recording into a register.
    recording: Option<char>,
    /// Recorded key sequence.
    recording_keys: Vec<KeyEvent>,
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
    /// Background workers for trusted non-primary workspace roots.
    root_file_workers: BTreeMap<PathBuf, FileWorker>,
    /// Language services and debugger runtime.
    language: LanguageState,
    /// Git workspace, request, and view state.
    git: GitState,
    /// Document symbols used for the sticky scroll header.
    sticky_symbols: Vec<SymbolItem>,
    /// Path and revision those symbols describe.
    sticky_symbols_path: Option<PathBuf>,
    sticky_symbols_revision: Option<u64>,
    /// Whether a sticky-symbol request is in flight.
    sticky_pending: bool,
    /// Debounce for the next sticky-symbol request.
    sticky_checked_at: Option<Instant>,
    /// LSP folding ranges for the active buffer.
    folds: Vec<FoldRange>,
    /// Start lines of closed folds.
    folded: HashSet<usize>,
    fold_revision: Option<u64>,
    pending_fold_action: Option<FoldAction>,
    /// Whether a format-on-save request is in flight.
    format_on_save_pending: bool,
    /// Suppress a second format request while saving formatted text.
    suppress_format_on_save: bool,
    /// Secondary pane when a vertical split is open.
    split: Option<Pane>,
    /// Flattened workspace tree, when open.
    tree: Vec<TreeEntry>,
    /// Quick-open usage counts keyed by the entry string.
    fuzzy_frecency: HashMap<String, u32>,
    /// Recently opened workspaces and files, newest first.
    recent_workspaces: Vec<PathBuf>,
    recent_files: Vec<PathBuf>,
    /// Whether recent lists are written to the state directory.
    persist_recent: bool,
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
    overlays: OverlayState,
    persist_theme: bool,
    settings: Settings,
    /// Project-local icon overrides from `.anvil.toml`.
    icons: icon_theme::IconTheme,
    /// Whether the icon renderer uses ASCII fallbacks.
    ascii_icons: bool,
    /// Last seen mtime for workspace icon overrides.
    icons_mtime: Option<SystemTime>,
    /// Next allowed mtime check for icon overrides.
    icons_checked_at: Option<Instant>,
    /// Text-area width used by the soft-wrap projection.
    wrap_width: usize,
    /// Wrapped segment offset within the first visible source line.
    scroll_subrow: usize,
    /// Gateway client used for file I/O when present.
    gateway: Option<GatewayClient>,
    /// Owning child/socket for the current gateway client.
    gateway_session: Option<GatewaySession>,
    /// Dirty swap files that must survive a source conflict.
    conflict_swaps: HashMap<PathBuf, PathBuf>,
    /// Paths changed by a multi-file replace whose buffer reload failed.
    stale_paths: HashSet<PathBuf>,
    /// Workspace root used to resolve all file paths.
    workspace: PathBuf,
    workspace_roots: workspace_roots::WorkspaceRoots,
    root_gateways: HashMap<PathBuf, (GatewaySession, GatewayClient)>,
    /// Reject file operations instead of falling back when the gateway is absent.
    require_gateway: bool,
    /// The active buffer was loaded from stdin and has no file path.
    stdin_buffer: bool,
    /// Reject mutating commands for a read-only input buffer.
    read_only: bool,
    /// File locations parsed from the stdin buffer.
    /// Latest requested project-search generation.
    project_search_generation: u64,
    /// Pending project-search generation, if a request is in flight.
    project_search_pending: Option<u64>,
    /// Number of workers expected to answer the pending project search.
    project_search_expected: usize,
    /// Number of workers that answered the pending project search.
    project_search_received: usize,
    /// Accumulated project-search results for the pending generation.
    project_search_results: Vec<pipe_locations::PipeLocation>,
    /// Whether any project-search worker truncated its output.
    project_search_truncated: bool,
    /// First project-search worker error for the pending generation.
    project_search_error: Option<String>,
    /// Pending reviewed project replace plan.
    project_replace_plan: Option<file_worker::ProjectReplacePlan>,
    /// Latest requested project-replace generation.
    project_replace_generation: u64,
    /// Pending project-replace generation, if a request is in flight.
    project_replace_pending: Option<u64>,
    /// Whether the process was started as an external editor.
    editor_mode: bool,
    /// Process exit code accumulated by editor-mode failures or `:cq`.
    exit_code: i32,
    /// Tree-sitter highlights cached by editor revision and path.
    syntax: RefCell<SyntaxState>,
    /// Matching bracket cached by editor revision, path, and cursor.
    bracket_cache: RefCell<bracket::BracketCache>,
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
    task_is_tests: bool,
    #[cfg(unix)]
    terminal: Option<pty_terminal::PtySession>,
    #[cfg(unix)]
    terminal_output: Vec<u8>,
    #[cfg(unix)]
    terminal_panel: bool,
}

impl Model {
    /// Builds a model from text and an optional source path.
    pub fn new(editor: EditorState, path: Option<PathBuf>) -> Self {
        let welcome = path.is_none() && editor.text().len_bytes() == 0;
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let workspace_roots = workspace_roots::WorkspaceRoots::new(&workspace).unwrap_or_default();
        Self {
            editor: DocumentSession::from(editor),
            path,
            welcome,
            welcome_back: false,
            cursor: 0,
            jump_list: jump::JumpList::default(),
            change_list: jump::JumpList::default(),
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
            macro_wait: None,
            recording: None,
            recording_keys: Vec::new(),
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
            root_file_workers: BTreeMap::new(),
            language: LanguageState::default(),
            git: GitState::default(),
            sticky_symbols: Vec::new(),
            sticky_symbols_path: None,
            sticky_symbols_revision: None,
            sticky_pending: false,
            sticky_checked_at: None,
            folds: Vec::new(),
            folded: HashSet::new(),
            fold_revision: None,
            pending_fold_action: None,
            format_on_save_pending: false,
            suppress_format_on_save: false,
            split: None,
            tree: Vec::new(),
            fuzzy_frecency: HashMap::new(),
            recent_workspaces: Vec::new(),
            recent_files: Vec::new(),
            persist_recent: !cfg!(test),
            tree_selected: None,
            tree_visible: false,
            tree_pending_select: None,
            tree_stale: false,
            tree_watch_limited: false,
            tree_refresh_in_flight: false,
            tree_refresh_queued: false,
            tree_refresh_generation: 0,
            show_hidden: false,
            overlays: OverlayState::default(),
            persist_theme: !cfg!(test),
            settings: Settings::default(),
            icons: icon_theme::IconTheme::default(),
            ascii_icons: icon_theme::ascii_mode(),
            icons_mtime: None,
            icons_checked_at: None,
            wrap_width: 80,
            scroll_subrow: 0,
            gateway: None,
            gateway_session: None,
            conflict_swaps: HashMap::new(),
            stale_paths: HashSet::new(),
            workspace,
            workspace_roots,
            root_gateways: HashMap::new(),
            require_gateway: false,
            stdin_buffer: false,
            read_only: false,
            project_search_generation: 0,
            project_search_pending: None,
            project_search_expected: 0,
            project_search_received: 0,
            project_search_results: Vec::new(),
            project_search_truncated: false,
            project_search_error: None,
            project_replace_plan: None,
            project_replace_generation: 0,
            project_replace_pending: None,
            editor_mode: false,
            exit_code: 0,
            syntax: RefCell::new(SyntaxState::default()),
            bracket_cache: RefCell::new(bracket::BracketCache::default()),
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
            task_is_tests: false,
            #[cfg(unix)]
            terminal: None,
            #[cfg(unix)]
            terminal_output: Vec::new(),
            #[cfg(unix)]
            terminal_panel: false,
        }
    }

    pub(crate) fn reload_config(&mut self) {
        let loaded = (|| -> Result<_, String> {
            let settings = settings::load()?;
            let keymap = keymap::KeymapConfig::load_default()?;
            let theme = theme_config::read_choice()?;
            Ok((settings, keymap, theme))
        })();
        match loaded {
            Ok((settings, keymap, theme)) => {
                if let Err(error) = self.apply_loaded_config(settings, keymap, theme) {
                    self.status = format!("E: config-reload: {error}");
                    self.dirty = true;
                }
            }
            Err(error) => {
                self.status = format!("E: config-reload: {error}");
                self.dirty = true;
            }
        }
    }

    fn apply_loaded_config(
        &mut self,
        settings: settings::Settings,
        keymap: keymap::KeymapConfig,
        theme: Option<theme::Choice>,
    ) -> Result<(), String> {
        if let Some(choice) = theme {
            theme::apply_choice(choice, false)?;
        }
        self.apply_settings(settings);
        self.keymap = keymap;
        self.status = "configuration reloaded".to_owned();
        self.dirty = true;
        Ok(())
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
        model.workspace_roots =
            workspace_roots::WorkspaceRoots::new(&model.workspace).unwrap_or_default();
        model.root_gateways.clear();
        model.root_file_workers.clear();
        model.reload_icons();
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
        self.git.marks.clear();
        self.git.diff_request = None;
        self.git.diff_checked = None;
        self.git.diff_due = None;
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
        self.git.diff_view = None;
        self.git.diff_pending = None;
        self.git.apply_pending = None;
        self.git.blame_pending = None;
        self.git.conflicts_pending = None;
        self.git.stage_pending = None;
        self.git.resolve_pending = None;
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
        self.overlays.lsp_results.completion = None;
        self.overlays.lsp_results.hover = None;
        self.ui.diagnostic_peek = None;
        self.overlays.lsp_results.code_actions = None;
        self.overlays.lsp_results.code_lens = None;
        self.overlays.lsp_results.document_symbols = None;
        self.overlays.lsp_results.references = None;
        self.overlays.lsp_results.document_highlights = None;
        self.overlays.lsp_results.call_hierarchy = None;
        self.overlays.lsp_results.semantic_legend.clear();
    }

    fn active_lsp_worker(&self) -> Option<&LspWorker> {
        self.language
            .lsp
            .config
            .as_ref()
            .and_then(|key| self.language.lsp.workers.get(key))
    }

    fn lsp_key(path: &Path) -> Option<(String, String)> {
        lsp_settings(path).map(|settings| (settings.tool, settings.language_id))
    }

    pub(crate) fn sync_lsp_active(&mut self) {
        let Some(path) = self.path.clone() else {
            self.language.lsp.config = None;
            self.clear_lsp_ui();
            return;
        };
        let Some(settings) = lsp_settings(&path) else {
            self.language.lsp.config = None;
            self.clear_lsp_ui();
            return;
        };
        self.activate_plugins(ActivationEvent::Language(settings.language_id.clone()));
        let desired = (settings.tool.clone(), settings.language_id.clone());
        if !self.language.lsp.workers.contains_key(&desired) {
            self.start_lsp();
            if self.active_lsp_worker().is_some() {
                self.request_folding_ranges(None);
                self.request_code_lens();
            }
            return;
        }
        if self.language.lsp.config.as_ref() != Some(&desired) {
            self.language.lsp.config = Some(desired.clone());
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
        if self.language.lsp.workers.contains_key(&desired) {
            self.language.lsp.config = Some(desired.clone());
            self.clear_lsp_ui();
            let text = self.editor.text().slice(0..self.editor.text().len_bytes());
            let _ = self.sync_lsp_document(&path, text);
            self.status = "lsp already running".to_owned();
            self.dirty = true;
            return;
        }
        let Some(client) = self.language.lsp.client.take().or_else(|| {
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
                self.language.lsp.config = Some(desired.clone());
                self.language.lsp.workers.insert(desired.clone(), worker);
                self.language.lsp.versions.insert(path.clone(), 1);
                self.language.lsp.retries.entry(desired).or_insert(0);
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
        if let Some(key) = self.language.lsp.config.take() {
            self.language.lsp.workers.remove(&key);
            self.language.lsp.retries.remove(&key);
            self.overlays.lsp_results.inlay_support.remove(&key);
        }
        self.clear_lsp_ui();
        self.status = "lsp stopped".to_owned();
        self.dirty = true;
    }

    fn handle_lsp_event(&mut self, key: &(String, String), event: LspEvent) {
        let active = self.language.lsp.config.as_ref() == Some(key);
        match event {
            LspEvent::Capabilities {
                inlay_hints,
                semantic_token_types,
                server_info,
            } => {
                self.language.lsp.retries.remove(key);
                self.language.lsp.retry = None;
                self.overlays
                    .lsp_results
                    .inlay_support
                    .insert(key.clone(), inlay_hints);
                if active {
                    if let Some(info) = server_info {
                        self.status = format!("lsp: {info}");
                    }
                    self.overlays.lsp_results.inlay_hints.clear();
                    self.overlays.lsp_results.inlay_pending = None;
                    self.overlays.lsp_results.inlay_checked = None;
                    self.overlays.lsp_results.semantic_legend = semantic_token_types;
                    self.overlays.lsp_results.semantic_tokens.clear();
                    self.overlays.lsp_results.semantic_revision = None;
                    self.request_semantic_tokens();
                }
            }
            LspEvent::Diagnostics(path, diagnostics) => {
                self.language.diagnostics.by_path.insert(path, diagnostics);
                self.status = format!(
                    "lsp diagnostics: {}",
                    self.language
                        .diagnostics
                        .by_path
                        .values()
                        .map(Vec::len)
                        .sum::<usize>()
                );
                self.rebuild_problems();
                if active
                    && !self.overlays.lsp_results.semantic_legend.is_empty()
                    && self.overlays.lsp_results.semantic_revision != Some(self.editor.revision())
                {
                    self.request_semantic_tokens();
                }
            }
            LspEvent::Crashed(error) => {
                self.language.lsp.workers.remove(key);
                if active {
                    self.language.lsp.config = None;
                    self.clear_lsp_ui();
                    let retries = {
                        let retries = self.language.lsp.retries.entry(key.clone()).or_insert(0);
                        *retries += 1;
                        *retries
                    };
                    if retries <= 3 {
                        let delay_ms = 250u64 << (retries.saturating_sub(1).min(3));
                        self.language.lsp.retry = Some((
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
                self.language.lsp.workers.remove(key);
                if active {
                    self.language.lsp.config = None;
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
        let keys: Vec<(String, String)> = self.language.lsp.workers.keys().cloned().collect();
        for key in keys {
            let events = self
                .language
                .lsp
                .workers
                .get(&key)
                .map(LspWorker::poll)
                .unwrap_or_default();
            for event in events {
                self.handle_lsp_event(&key, event);
            }
        }
    }

    pub(crate) fn poll_lsp_retry(&mut self) {
        let Some((key, deadline)) = self.language.lsp.retry.clone() else {
            return;
        };
        if Instant::now() < deadline {
            return;
        }
        self.language.lsp.retry = None;
        if !self.language.lsp.workers.contains_key(&key) {
            self.start_lsp();
        }
    }

    fn diagnostic_lines(&self) -> Vec<usize> {
        let Some(path) = &self.path else {
            return Vec::new();
        };
        self.language
            .diagnostics
            .by_path
            .get(path)
            .map(|diagnostics| {
                diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.range.start.line as usize)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn inline_diagnostic(&self, line_number: usize) -> Option<(String, u32)> {
        let path = self.path.as_ref()?;
        let diagnostic = self
            .language
            .diagnostics
            .by_path
            .get(path)?
            .iter()
            .filter(|diagnostic| {
                diagnostic.range.start.line as usize <= line_number
                    && diagnostic.range.end.line as usize >= line_number
            })
            .min_by_key(|diagnostic| diagnostic.severity.unwrap_or(99))?;
        let message = diagnostic
            .message
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        (!message.is_empty()).then_some((message, diagnostic.severity.unwrap_or(3)))
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
        let Some(diagnostics) = self.language.diagnostics.by_path.get(path) else {
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
        self.language.lsp.workers.clear();
        self.language.lsp.client = None;
        self.language.lsp.config = None;
        self.language.lsp.retries.clear();
        self.clear_lsp_ui();
        self.gateway = None;
        self.gateway_session = None;
        match GatewaySession::start(&self.workspace) {
            Ok((session, client, lsp_client)) => {
                self.gateway_session = Some(session);
                self.gateway = Some(client);
                self.language.lsp.client = Some(lsp_client);
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
        let overlay = self.overlays.command_palette.is_some()
            || self.overlays.fuzzy.is_some()
            || self.overlays.lsp_results.workspace_symbols.is_some()
            || self.overlays.lsp_results.document_symbols.is_some()
            || self.overlays.lsp_results.references.is_some()
            || self.overlays.lsp_results.call_hierarchy.is_some()
            || self.overlays.settings_overlay.is_some()
            || self.overlays.lsp_results.code_actions.is_some()
            || self.overlays.lsp_results.completion.is_some()
            || self.overlays.lsp_results.hover.is_some()
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

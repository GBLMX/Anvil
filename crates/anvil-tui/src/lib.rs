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
mod lsp_runtime;
mod lsp_worker;
mod model;
mod readonly;
mod render;
#[cfg(test)]
use readonly::HELP_TEXT;
use readonly::ReadOnlyState;
mod settings;
mod theme;
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
struct LspTransportState {
    client: Option<GatewayClient>,
    workers: HashMap<(String, String), LspWorker>,
    config: Option<(String, String)>,
    retries: HashMap<(String, String), u8>,
    retry: Option<((String, String), Instant)>,
}

#[derive(Default)]
struct LspRequestsState {
    pending: HashMap<u64, LspRequestKind>,
    pending_edit_revision: HashMap<u64, u64>,
    pending_edit_path: HashMap<u64, PathBuf>,
    pending_edit_snapshot: HashMap<u64, HashMap<PathBuf, u64>>,
    versions: HashMap<PathBuf, u64>,
    next_request: u64,
    /// Whether a format-on-save request is in flight.
    format_on_save_pending: bool,
    /// Suppress a second format request while saving formatted text.
    suppress_format_on_save: bool,
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
}

#[derive(Default)]
struct WorkspaceState {
    /// Workspace root used to resolve all file paths.
    path: PathBuf,
    roots: workspace_roots::WorkspaceRoots,
    /// Gateway client used for file I/O when present.
    gateway: Option<GatewayClient>,
    /// Owning child/socket for the current gateway client.
    gateway_session: Option<GatewaySession>,
    root_gateways: HashMap<PathBuf, (GatewaySession, GatewayClient)>,
    /// Reject file operations instead of falling back when the gateway is absent.
    require_gateway: bool,
    /// Background directory and file enumerator.
    file_worker: Option<FileWorker>,
    /// Background workers for trusted non-primary workspace roots.
    root_file_workers: BTreeMap<PathBuf, FileWorker>,
    /// Dirty swap files that must survive a source conflict.
    conflict_swaps: HashMap<PathBuf, PathBuf>,
    /// Paths changed by a multi-file replace whose buffer reload failed.
    stale_paths: HashSet<PathBuf>,
}

#[derive(Default)]
struct ProjectState {
    /// Latest requested project-search generation.
    search_generation: u64,
    /// Pending project-search generation, if a request is in flight.
    search_pending: Option<u64>,
    /// Number of workers expected to answer the pending project search.
    search_expected: usize,
    /// Number of workers that answered the pending project search.
    search_received: usize,
    /// Accumulated project-search results for the pending generation.
    search_results: Vec<pipe_locations::PipeLocation>,
    /// Whether any project-search worker truncated its output.
    search_truncated: bool,
    /// First project-search worker error for the pending generation.
    search_error: Option<String>,
    /// Pending reviewed project replace plan.
    replace_plan: Option<file_worker::ProjectReplacePlan>,
    /// Latest requested project-replace generation.
    replace_generation: u64,
    /// Pending project-replace generation, if a request is in flight.
    replace_pending: Option<u64>,
}

pub struct ViewState {
    /// First visible line, independent of the cursor.
    pub scroll: usize,
    /// First visible display column.
    pub hscroll: usize,
    viewport_height: u16,
    viewport_width: u16,
    split_ratio: u16,
    pointer: mouse::PointerState,
    /// Text-area width used by the soft-wrap projection.
    wrap_width: usize,
    /// Wrapped segment offset within the first visible source line.
    scroll_subrow: usize,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            scroll: 0,
            hscroll: 0,
            viewport_height: 24,
            viewport_width: 80,
            split_ratio: 50,
            pointer: mouse::PointerState::Idle,
            wrap_width: 80,
            scroll_subrow: 0,
        }
    }
}

struct PluginState {
    /// Discovered local plugins.
    #[cfg(target_os = "linux")]
    entries: Vec<plugin_discovery::PluginDescriptor>,
    /// Plugin host supervisor keyed by plugin id.
    #[cfg(target_os = "linux")]
    supervisor: PluginSupervisor,
    #[cfg(target_os = "linux")]
    manager: Option<plugin_manager::PluginManagerState>,
    #[cfg(target_os = "linux")]
    store_size: u64,
    #[cfg(target_os = "linux")]
    installer: Option<(GatewaySession, GatewayClient)>,
    pkg_install: Option<pkg_install::PkgInstallState>,
}

impl Default for PluginState {
    fn default() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            entries: Vec::new(),
            #[cfg(target_os = "linux")]
            supervisor: PluginSupervisor::new(),
            #[cfg(target_os = "linux")]
            manager: None,
            #[cfg(target_os = "linux")]
            store_size: 0,
            #[cfg(target_os = "linux")]
            installer: None,
            pkg_install: None,
        }
    }
}

pub struct EditorSessionState {
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
}

#[derive(Default)]
struct EditSessionState {
    /// Anchor for the selection-first visual adapter.
    visual_anchor: Option<usize>,
    /// Whether the visual adapter selects whole lines.
    visual_line: bool,
    modal_layer: Option<ModalLayer>,
    /// Last successful semantic change for `.`.
    last_change: Option<LastChange>,
    /// Text captured by the current insert session.
    insert_session: Option<InsertSession>,
}

#[derive(Default)]
pub struct StatusState {
    /// Short status line.
    text: String,
    /// Background file operation label, if any.
    busy: Option<String>,
    busy_progress: Option<u64>,
    /// Spinner frame for the busy label.
    spinner: usize,
}

struct ConfigState {
    /// Typed effects produced by command dispatch.
    effects: EffectQueue,
    /// In-memory keymap overrides.
    keymap: keymap::KeymapConfig,
    settings: Settings,
}

#[derive(Default)]
pub struct FileState {
    /// Set by `q` / `:q`.
    pub quit: bool,
    /// Whether the file has unsaved changes.
    pub modified: bool,
    /// Whether the active file disappeared outside the editor.
    pub missing: bool,
    /// Whether the next loop iteration should render.
    pub dirty: bool,
    /// The active buffer was loaded from stdin and has no file path.
    stdin_buffer: bool,
    /// Reject mutating commands for a read-only input buffer.
    read_only: bool,
    /// Whether the process was started as an external editor.
    editor_mode: bool,
    /// Process exit code accumulated by editor-mode failures or `:cq`.
    exit_code: i32,
}

#[derive(Default)]
struct StartPageState {
    /// Full-editor start dashboard, shown until the first interaction or file open.
    welcome: bool,
    /// Dashboard state to restore after closing the help page.
    welcome_back: bool,
}

struct SyntaxCacheState {
    /// Tree-sitter highlights cached by editor revision and path.
    highlights: RefCell<SyntaxState>,
    /// Matching bracket cached by editor revision, path, and cursor.
    bracket: RefCell<bracket::BracketCache>,
}

#[derive(Default)]
struct InputState {
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
}

#[derive(Default)]
struct SearchSession {
    /// Last substring search.
    query: String,
    state: search::SearchState,
}

#[derive(Default)]
struct AppearanceState {
    /// Project-local icon overrides from `.anvil.toml`.
    icons: icon_theme::IconTheme,
    /// Whether the icon renderer uses ASCII fallbacks.
    ascii_icons: bool,
    /// Last seen mtime for workspace icon overrides.
    icons_mtime: Option<SystemTime>,
    /// Next allowed mtime check for icon overrides.
    icons_checked_at: Option<Instant>,
    /// Whether the theme choice is persisted.
    persist_theme: bool,
}

#[derive(Default)]
struct BufferState {
    /// All open buffers, including the active one after the first switch.
    entries: Vec<Buffer>,
    /// Active buffer index.
    active: usize,
    /// Secondary pane when a vertical split is open.
    split: Option<Pane>,
}

#[derive(Default)]
struct RuntimeState {
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

#[derive(Default)]
struct RecentState {
    /// Quick-open usage counts keyed by the entry string.
    frecency: HashMap<String, u32>,
    /// Recently opened workspaces and files, newest first.
    workspaces: Vec<PathBuf>,
    files: Vec<PathBuf>,
    /// Whether recent lists are written to the state directory.
    persist: bool,
}

#[derive(Default)]
struct TreeState {
    /// Flattened workspace tree, when open.
    entries: Vec<TreeEntry>,
    selected: Option<usize>,
    visible: bool,
    /// Entry to select after the next tree refresh.
    pending_select: Option<PathBuf>,
    stale: bool,
    watch_limited: bool,
    refresh_in_flight: bool,
    refresh_queued: bool,
    refresh_generation: u64,
    show_hidden: bool,
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
    lsp: LspTransportState,
    lsp_requests: LspRequestsState,
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
    /// Editor document, source path, cursor, and navigation history.
    pub session: EditorSessionState,
    /// Visual selection, modal layer, and insert-session state.
    edit: EditSessionState,
    /// Viewport, scrolling, wrap, pointer, and split ratio.
    pub view: ViewState,
    /// Input state.
    pub ui: UiState,
    /// Keymap, settings, and typed command effects.
    config: ConfigState,
    /// File flags and process exit state.
    pub file: FileState,
    /// Status text and background activity state.
    pub status: StatusState,
    /// Operator, register, macro, and replay input state.
    input: InputState,
    /// Substring search query and state.
    search: SearchSession,
    /// Start dashboard and help return state.
    start_page: StartPageState,
    /// Read-only page replacing the active buffer.
    pager: ReadOnlyState,
    /// Open buffers and active pane.
    buffers: BufferState,
    /// Workspace root, gateways, and file workers.
    workspace: WorkspaceState,
    /// Language services and debugger runtime.
    language: LanguageState,
    /// Git workspace, request, and view state.
    git: GitState,
    /// Recent workspaces, files, and quick-open ranking.
    recent: RecentState,
    /// Workspace tree view and refresh state.
    tree: TreeState,
    overlays: OverlayState,
    /// Icon and theme persistence state.
    appearance: AppearanceState,
    /// Project search and replace runtime state.
    project: ProjectState,
    /// Tree-sitter and bracket caches.
    syntax_cache: SyntaxCacheState,
    /// Plugin discovery, host, manager, installer, and package install state.
    plugins: PluginState,
    /// Task and terminal runtime workers.
    runtime: RuntimeState,
}

#[cfg(test)]
mod tests;

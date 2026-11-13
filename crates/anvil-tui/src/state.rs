//! TUI state groups and view/editor snapshots.

use super::*;

#[derive(Clone)]
pub(crate) struct Buffer {
    pub(crate) editor: DocumentSession,
    pub(crate) path: Option<PathBuf>,
    pub(crate) cursor: usize,
    pub(crate) scroll: usize,
    pub(crate) hscroll: usize,
    pub(crate) modified: bool,
    pub(crate) missing: bool,
    pub(crate) visual_anchor: Option<usize>,
    pub(crate) visual_line: bool,
    pub(crate) search_query: String,
    pub(crate) syntax: crate::syntax::SyntaxCache,
}

pub(crate) struct Pane {
    pub(crate) buffer: Buffer,
}

/// Vi word motion kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordMotion {
    NextStart,
    PreviousStart,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LastChange {
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
pub(crate) enum InsertOrigin {
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
pub(crate) struct InsertSession {
    pub(crate) origin: InsertOrigin,
    pub(crate) text: String,
    pub(crate) start: usize,
    pub(crate) valid: bool,
}

/// TUI state.
#[derive(Debug, Clone)]
pub(crate) struct FuzzyState {
    pub(crate) generation: u64,
    pub(crate) query: String,
    pub(crate) entries: Vec<String>,
    pub(crate) filtered: Vec<usize>,
    pub(crate) selected: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct BufferPickerState {
    pub(crate) query: String,
    pub(crate) entries: Vec<usize>,
    pub(crate) filtered: Vec<usize>,
    pub(crate) selected: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PreviewState {
    pub(crate) path: Option<String>,
    pub(crate) lines: Vec<String>,
    pub(crate) message: Option<String>,
    pub(crate) loading: bool,
    pub(crate) truncated: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct CommandPaletteItem {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) plugin_id: Option<String>,
    pub(crate) theme: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub(crate) struct CommandPaletteState {
    pub(crate) table: SelectionTable<CommandPaletteItem>,
    pub(crate) theme_picker: theme_picker::ThemePicker,
}

impl SelectionItem for CommandPaletteItem {
    fn label(&self) -> &str {
        &self.label
    }
}

pub(crate) struct LocationPanelState {
    pub(crate) stdin_locations: Vec<pipe_locations::PipeLocation>,
    pub(crate) stdin_location_selected: Option<usize>,
    pub(crate) stdin_location_scroll: usize,
    pub(crate) location_title: String,
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
pub(crate) struct DapState {
    pub(crate) breakpoints: HashMap<PathBuf, BTreeSet<u32>>,
    pub(crate) client: Option<crate::dap::DapClient>,
    pub(crate) attach: Option<mpsc::Receiver<Result<crate::dap::DapClient, String>>>,
    pub(crate) attach_started: Option<Instant>,
    pub(crate) request: Option<
        mpsc::Receiver<(
            crate::dap::DapClient,
            Result<debug_panel::DebugSnapshot, String>,
        )>,
    >,
    pub(crate) request_started: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct DiagnosticsState {
    pub(crate) by_path: HashMap<PathBuf, Vec<lsp::Diagnostic>>,
    pub(crate) count_shown: usize,
    pub(crate) count_pending: Option<(usize, Instant)>,
    pub(crate) path: Option<PathBuf>,
}

#[derive(Default)]
pub(crate) struct LspTransportState {
    pub(crate) client: Option<GatewayClient>,
    pub(crate) workers: HashMap<(String, String), LspWorker>,
    pub(crate) config: Option<(String, String)>,
    pub(crate) retries: HashMap<(String, String), u8>,
    pub(crate) retry: Option<((String, String), Instant)>,
}

#[derive(Default)]
pub(crate) struct LspRequestsState {
    pub(crate) pending: HashMap<u64, LspRequestKind>,
    pub(crate) pending_edit_revision: HashMap<u64, u64>,
    pub(crate) pending_edit_path: HashMap<u64, PathBuf>,
    pub(crate) pending_edit_snapshot: HashMap<u64, HashMap<PathBuf, u64>>,
    pub(crate) versions: HashMap<PathBuf, u64>,
    pub(crate) next_request: u64,
    /// Whether a format-on-save request is in flight.
    pub(crate) format_on_save_pending: bool,
    /// Suppress a second format request while saving formatted text.
    pub(crate) suppress_format_on_save: bool,
}

#[derive(Default)]
pub(crate) struct LspResultsState {
    pub(crate) completion: Option<CompletionState>,
    pub(crate) completion_resolve: Option<(u64, usize)>,
    pub(crate) code_lens: Option<code_lens::CodeLensState>,
    pub(crate) snippet: Option<SnippetSession>,
    pub(crate) semantic_legend: Vec<String>,
    pub(crate) semantic_tokens: Vec<SemanticToken>,
    pub(crate) semantic_revision: Option<u64>,
    pub(crate) hover: Option<String>,
    pub(crate) code_actions: Option<CodeActionState>,
    pub(crate) document_symbols: Option<SymbolState>,
    pub(crate) references: Option<ReferenceState>,
    pub(crate) document_highlights: Option<DocumentHighlightState>,
    pub(crate) call_hierarchy: Option<CallHierarchyState>,
    pub(crate) signature_help: Option<SignatureHelpState>,
    pub(crate) workspace_symbols: Option<WorkspaceSymbolState>,
    pub(crate) inlay_hints: Vec<InlayHint>,
    pub(crate) inlay_pending: Option<(u64, PathBuf, u64, usize, usize)>,
    pub(crate) inlay_checked: Option<(PathBuf, u64, usize, usize)>,
    pub(crate) inlay_support: HashMap<(String, String), bool>,
    /// Document symbols used for the sticky scroll header.
    pub(crate) sticky_symbols: Vec<SymbolItem>,
    /// Path and revision those symbols describe.
    pub(crate) sticky_symbols_path: Option<PathBuf>,
    pub(crate) sticky_symbols_revision: Option<u64>,
    /// Whether a sticky-symbol request is in flight.
    pub(crate) sticky_pending: bool,
    /// Debounce for the next sticky-symbol request.
    pub(crate) sticky_checked_at: Option<Instant>,
    /// LSP folding ranges for the active buffer.
    pub(crate) folds: Vec<FoldRange>,
    /// Start lines of closed folds.
    pub(crate) folded: HashSet<usize>,
    pub(crate) fold_revision: Option<u64>,
    pub(crate) pending_fold_action: Option<FoldAction>,
}

#[derive(Default)]
pub(crate) struct WorkspaceState {
    /// Workspace root used to resolve all file paths.
    pub(crate) path: PathBuf,
    pub(crate) roots: workspace_roots::WorkspaceRoots,
    /// Gateway client used for file I/O when present.
    pub(crate) gateway: Option<GatewayClient>,
    /// Owning child/socket for the current gateway client.
    pub(crate) gateway_session: Option<GatewaySession>,
    pub(crate) root_gateways: HashMap<PathBuf, (GatewaySession, GatewayClient)>,
    /// Reject file operations instead of falling back when the gateway is absent.
    pub(crate) require_gateway: bool,
    /// Background directory and file enumerator.
    pub(crate) file_worker: Option<FileWorker>,
    /// Background workers for trusted non-primary workspace roots.
    pub(crate) root_file_workers: BTreeMap<PathBuf, FileWorker>,
    /// Dirty swap files that must survive a source conflict.
    pub(crate) conflict_swaps: HashMap<PathBuf, PathBuf>,
    /// Paths changed by a multi-file replace whose buffer reload failed.
    pub(crate) stale_paths: HashSet<PathBuf>,
}

#[derive(Default)]
pub(crate) struct ProjectState {
    /// Latest requested project-search generation.
    pub(crate) search_generation: u64,
    /// Pending project-search generation, if a request is in flight.
    pub(crate) search_pending: Option<u64>,
    /// Number of workers expected to answer the pending project search.
    pub(crate) search_expected: usize,
    /// Number of workers that answered the pending project search.
    pub(crate) search_received: usize,
    /// Accumulated project-search results for the pending generation.
    pub(crate) search_results: Vec<pipe_locations::PipeLocation>,
    /// Whether any project-search worker truncated its output.
    pub(crate) search_truncated: bool,
    /// First project-search worker error for the pending generation.
    pub(crate) search_error: Option<String>,
    /// Pending reviewed project replace plan.
    pub(crate) replace_plan: Option<file_worker::ProjectReplacePlan>,
    /// Latest requested project-replace generation.
    pub(crate) replace_generation: u64,
    /// Pending project-replace generation, if a request is in flight.
    pub(crate) replace_pending: Option<u64>,
}

pub struct ViewState {
    /// First visible line, independent of the cursor.
    pub scroll: usize,
    /// First visible display column.
    pub hscroll: usize,
    pub(crate) viewport_height: u16,
    pub(crate) viewport_width: u16,
    pub(crate) split_ratio: u16,
    pub(crate) pointer: mouse::PointerState,
    /// Text-area width used by the soft-wrap projection.
    pub(crate) wrap_width: usize,
    /// Wrapped segment offset within the first visible source line.
    pub(crate) scroll_subrow: usize,
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

pub(crate) struct PluginState {
    /// Discovered local plugins.
    #[cfg(target_os = "linux")]
    pub(crate) entries: Vec<plugin_discovery::PluginDescriptor>,
    /// Plugin host supervisor keyed by plugin id.
    #[cfg(target_os = "linux")]
    pub(crate) supervisor: PluginSupervisor,
    #[cfg(target_os = "linux")]
    pub(crate) manager: Option<plugin_manager::PluginManagerState>,
    #[cfg(target_os = "linux")]
    pub(crate) store_size: u64,
    #[cfg(target_os = "linux")]
    pub(crate) installer: Option<(GatewaySession, GatewayClient)>,
    pub(crate) pkg_install: Option<pkg_install::PkgInstallState>,
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
    pub(crate) jump_list: jump::JumpList,
    /// Bounded back/forward edit-position history.
    pub(crate) change_list: jump::JumpList,
}

#[derive(Default)]
pub(crate) struct EditSessionState {
    /// Anchor for the selection-first visual adapter.
    pub(crate) visual_anchor: Option<usize>,
    /// Whether the visual adapter selects whole lines.
    pub(crate) visual_line: bool,
    pub(crate) modal_layer: Option<ModalLayer>,
    /// Last successful semantic change for `.`.
    pub(crate) last_change: Option<LastChange>,
    /// Text captured by the current insert session.
    pub(crate) insert_session: Option<InsertSession>,
}

#[derive(Default)]
pub struct StatusState {
    /// Short status line.
    pub(crate) text: String,
    /// Background file operation label, if any.
    pub(crate) busy: Option<String>,
    pub(crate) busy_progress: Option<u64>,
    /// Spinner frame for the busy label.
    pub(crate) spinner: usize,
}

pub(crate) struct ConfigState {
    /// Typed effects produced by command dispatch.
    pub(crate) effects: EffectQueue,
    /// In-memory keymap overrides.
    pub(crate) keymap: keymap::KeymapConfig,
    pub(crate) settings: Settings,
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
    pub(crate) stdin_buffer: bool,
    /// Reject mutating commands for a read-only input buffer.
    pub(crate) read_only: bool,
    /// Whether the process was started as an external editor.
    pub(crate) editor_mode: bool,
    /// Process exit code accumulated by editor-mode failures or `:cq`.
    pub(crate) exit_code: i32,
}

#[derive(Default)]
pub(crate) struct StartPageState {
    /// Full-editor start dashboard, shown until the first interaction or file open.
    pub(crate) welcome: bool,
    /// Dashboard state to restore after closing the help page.
    pub(crate) welcome_back: bool,
}

pub(crate) struct SyntaxCacheState {
    /// Tree-sitter highlights cached by editor revision and path.
    pub(crate) highlights: RefCell<SyntaxState>,
    /// Matching bracket cached by editor revision, path, and cursor.
    pub(crate) bracket: RefCell<bracket::BracketCache>,
}

#[derive(Default)]
pub(crate) struct InputState {
    /// Pending normal-mode operator (`d` or `y`).
    pub(crate) pending: Option<char>,
    /// Deadline for a pending Space leader.
    pub(crate) leader_deadline: Option<Instant>,
    /// Numeric count prefix, zero means no count.
    pub(crate) count: usize,
    /// Waiting for a register name after `"`.
    pub(crate) register_wait: bool,
    /// Register selected by `"x`.
    pub(crate) register: Option<char>,
    /// Register name waiting after `q` or `@`.
    pub(crate) macro_wait: Option<char>,
    /// Currently recording into a register.
    pub(crate) recording: Option<char>,
    /// Recorded key sequence.
    pub(crate) recording_keys: Vec<KeyEvent>,
    /// Last replayed macro.
    pub(crate) last_macro: Option<char>,
    /// Replay guard.
    pub(crate) replaying: bool,
    /// `c` / `y` operator waiting for an inner text object (`i`).
    pub(crate) object_pending: bool,
}

#[derive(Default)]
pub(crate) struct SearchSession {
    /// Last substring search.
    pub(crate) query: String,
    pub(crate) state: search::SearchState,
}

#[derive(Default)]
pub(crate) struct AppearanceState {
    /// Project-local icon overrides from `.anvil.toml`.
    pub(crate) icons: icon_theme::IconTheme,
    /// Whether the icon renderer uses ASCII fallbacks.
    pub(crate) ascii_icons: bool,
    /// Last seen mtime for workspace icon overrides.
    pub(crate) icons_mtime: Option<SystemTime>,
    /// Next allowed mtime check for icon overrides.
    pub(crate) icons_checked_at: Option<Instant>,
    /// Whether the theme choice is persisted.
    pub(crate) persist_theme: bool,
}

#[derive(Default)]
pub(crate) struct BufferState {
    /// All open buffers, including the active one after the first switch.
    pub(crate) entries: Vec<Buffer>,
    /// Active buffer index.
    pub(crate) active: usize,
    /// Secondary pane when a vertical split is open.
    pub(crate) split: Option<Pane>,
    /// Internal buffers for unopened files changed by workspace edits.
    pub(crate) background: HashMap<PathBuf, Buffer>,
}

#[derive(Default)]
pub(crate) struct RuntimeState {
    pub(crate) task_worker: Option<task_runner::TaskWorker>,
    pub(crate) task_output: Option<String>,
    pub(crate) task_problems: Vec<Problem>,
    pub(crate) task_is_tests: bool,
    #[cfg(unix)]
    pub(crate) terminal: Option<pty_terminal::PtySession>,
    #[cfg(unix)]
    pub(crate) terminal_output: Vec<u8>,
    #[cfg(unix)]
    pub(crate) terminal_panel: bool,
}

#[derive(Default)]
pub(crate) struct RecentState {
    /// Quick-open usage counts keyed by the entry string.
    pub(crate) frecency: HashMap<String, u32>,
    /// Recently opened workspaces and files, newest first.
    pub(crate) workspaces: Vec<PathBuf>,
    pub(crate) files: Vec<PathBuf>,
    /// Whether recent lists are written to the state directory.
    pub(crate) persist: bool,
}

#[derive(Default)]
pub(crate) struct TreeState {
    /// Flattened workspace tree, when open.
    pub(crate) entries: Vec<TreeEntry>,
    pub(crate) selected: Option<usize>,
    pub(crate) visible: bool,
    /// Entry to select after the next tree refresh.
    pub(crate) pending_select: Option<PathBuf>,
    pub(crate) stale: bool,
    pub(crate) watch_limited: bool,
    pub(crate) refresh_in_flight: bool,
    pub(crate) refresh_queued: bool,
    pub(crate) refresh_generation: u64,
    pub(crate) show_hidden: bool,
}

#[derive(Default)]
pub(crate) struct GitState {
    /// Git gutter marks keyed by zero-based line.
    pub(crate) marks: HashMap<usize, char>,
    /// Workspace git status keyed by absolute path.
    pub(crate) status: HashMap<PathBuf, char>,
    /// Latest requested tree git-status generation.
    pub(crate) status_generation: u64,
    /// Pending tree git-status generation.
    pub(crate) status_pending: Option<u64>,
    /// Number of roots expected to answer the pending git-status request.
    pub(crate) status_expected: usize,
    /// Number of roots that answered the pending git-status request.
    pub(crate) status_received: usize,
    /// Requested git diff path and revision.
    pub(crate) diff_request: Option<(PathBuf, u64)>,
    /// Last requested git diff path and revision.
    pub(crate) diff_checked: Option<(PathBuf, u64)>,
    /// Debounce deadline for the next git diff request.
    pub(crate) diff_due: Option<Instant>,
    /// Latest requested read-only Git diff generation.
    pub(crate) diff_generation: u64,
    /// Pending Git diff generation, if a request is in flight.
    pub(crate) diff_pending: Option<u64>,
    /// Parsed hunks for the active Git diff view.
    pub(crate) diff_view: Option<git_diff::GitDiffView>,
    /// Latest requested hunk-apply generation.
    pub(crate) apply_generation: u64,
    /// Pending hunk-apply generation, if a request is in flight.
    pub(crate) apply_pending: Option<u64>,
    /// Latest requested Git blame generation.
    pub(crate) blame_generation: u64,
    /// Pending Git blame generation, if a request is in flight.
    pub(crate) blame_pending: Option<u64>,
    /// Latest requested Git conflict-list generation.
    pub(crate) conflicts_generation: u64,
    /// Pending Git conflict-list generation, if a request is in flight.
    pub(crate) conflicts_pending: Option<u64>,
    /// Latest requested conflict-stage generation.
    pub(crate) stage_generation: u64,
    /// Pending conflict-stage generation, if a request is in flight.
    pub(crate) stage_pending: Option<u64>,
    /// Latest requested conflict-resolution generation.
    pub(crate) resolve_generation: u64,
    /// Pending conflict-resolution generation, if a request is in flight.
    pub(crate) resolve_pending: Option<u64>,
    pub(crate) branch: Option<String>,
    pub(crate) branch_checked: bool,
}

#[derive(Default)]
pub(crate) struct LanguageState {
    pub(crate) dap: DapState,
    pub(crate) diagnostics: DiagnosticsState,
    pub(crate) lsp: LspTransportState,
    pub(crate) lsp_requests: LspRequestsState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OverlayKind {
    CommandPalette,
    Settings,
    BufferPicker,
    Fuzzy,
}

impl OverlayKind {
    pub(crate) fn keymap_context(self) -> KeymapContext {
        KeymapContext::Popup
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OverlayEntry {
    pub(crate) kind: OverlayKind,
    pub(crate) focus_before: anvil_session::ui::FocusState,
}

#[derive(Default)]
pub(crate) struct OverlayState {
    pub(crate) stack: Vec<OverlayEntry>,
    pub(crate) command_palette: Option<CommandPaletteState>,
    pub(crate) settings_overlay: Option<SettingsOverlayState>,
    pub(crate) theme_browser: Option<theme_browser::ThemeBrowser>,
    pub(crate) fuzzy: Option<FuzzyState>,
    pub(crate) fuzzy_generation: u64,
    pub(crate) buffer_picker: Option<BufferPickerState>,
    pub(crate) preview: PreviewState,
    pub(crate) preview_generation: u64,
    pub(crate) debug_panel: Option<debug_panel::DebugPanelState>,
    pub(crate) test_explorer: Option<test_explorer::TestExplorerState>,
    pub(crate) command_completion: Option<CommandCompletion>,
    pub(crate) command_palette_recents: Vec<String>,
    pub(crate) location: LocationPanelState,
    pub(crate) lsp_results: LspResultsState,
}

#[derive(Debug, Clone)]
pub(crate) struct CompletionItem {
    pub(crate) label: String,
    pub(crate) filter_text: String,
    pub(crate) sort_text: String,
    pub(crate) preselect: bool,
    pub(crate) insert: String,
    pub(crate) snippet: Option<Snippet>,
    pub(crate) detail: String,
    pub(crate) replace: Option<(usize, usize, usize, usize)>,
    pub(crate) documentation: Option<String>,
    pub(crate) raw: serde_json::Value,
    pub(crate) resolved: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct CompletionState {
    pub(crate) id: u64,
    pub(crate) items: Vec<CompletionItem>,
    pub(crate) filtered: Vec<usize>,
    pub(crate) query: String,
    pub(crate) selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SemanticToken {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) type_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LspRequestKind {
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
pub(crate) struct CodeActionItem {
    pub(crate) title: String,
    pub(crate) edit: Option<WorkspaceEdit>,
    pub(crate) disabled: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CodeActionState {
    pub(crate) items: Vec<CodeActionItem>,
    pub(crate) selected: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct SymbolItem {
    pub(crate) name: String,
    pub(crate) detail: Option<String>,
    pub(crate) line: usize,
    pub(crate) character: usize,
    pub(crate) depth: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct SymbolState {
    pub(crate) items: Vec<SymbolItem>,
    pub(crate) selected: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ReferenceItem {
    pub(crate) path: PathBuf,
    pub(crate) line: usize,
    pub(crate) character: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ReferenceState {
    pub(crate) items: Vec<ReferenceItem>,
    pub(crate) selected: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct DocumentHighlightItem {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) kind: u8,
}

#[derive(Debug, Clone)]
pub(crate) struct DocumentHighlightState {
    pub(crate) revision: u64,
    pub(crate) items: Vec<DocumentHighlightItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallHierarchyDirection {
    Incoming,
    Outgoing,
}

#[derive(Debug, Clone)]
pub(crate) struct CallHierarchyItem {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) line: usize,
    pub(crate) character: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CallHierarchyState {
    pub(crate) direction: CallHierarchyDirection,
    pub(crate) items: Vec<CallHierarchyItem>,
    pub(crate) selected: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct SignatureItem {
    pub(crate) label: String,
    pub(crate) documentation: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SignatureHelpState {
    pub(crate) signatures: Vec<SignatureItem>,
    pub(crate) active_signature: usize,
    pub(crate) active_parameter: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceSymbolItem {
    pub(crate) name: String,
    pub(crate) detail: String,
    pub(crate) path: PathBuf,
    pub(crate) line: usize,
    pub(crate) character: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceSymbolState {
    pub(crate) items: Vec<WorkspaceSymbolItem>,
    pub(crate) filtered: Vec<usize>,
    pub(crate) query: String,
    pub(crate) selected: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct InlayHint {
    pub(crate) line: usize,
    pub(crate) character: usize,
    pub(crate) label: String,
}

#[derive(Debug, Clone)]
pub(crate) struct TreeEntry {
    pub(crate) path: PathBuf,
    pub(crate) depth: usize,
    pub(crate) is_dir: bool,
    pub(crate) ino: u64,
    pub(crate) expanded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModalLayer {
    Surround,
}

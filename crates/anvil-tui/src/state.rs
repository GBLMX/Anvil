//! TUI state groups and view/editor snapshots.

use super::*;
use anvil_lsp_client as lsp;
use anvil_session::plugin_provider::{PluginProviderRegistry, PluginProviderRequests};
use anvil_session::provider::{CancellationToken, PickerEntry, ProviderId};
use anvil_session::recents::{self, Recents};
use std::sync::atomic::{AtomicU64, Ordering};

const GIT_STATUS_DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct BufferId(u64);

impl BufferId {
    pub(crate) fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone)]
pub(crate) struct Buffer {
    pub(crate) id: BufferId,
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
    pub(crate) syntax: anvil_syntax::SyntaxCache,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BufferPickerMode {
    Buffers,
    Plugin {
        provider_id: ProviderId,
    },
    ChangedFiles {
        paths: Vec<PathBuf>,
    },
    /// `(forward, display index)` for each picker row.
    Jumps {
        targets: Vec<(bool, usize)>,
    },
    SearchHistory {
        queries: Vec<String>,
    },
}

pub(crate) struct BufferPickerState {
    pub(crate) query: String,
    pub(crate) entries: Vec<PickerEntry>,
    pub(crate) filtered: Vec<usize>,
    pub(crate) selected: usize,
    pub(crate) cancel: CancellationToken,
    pub(crate) mode: BufferPickerMode,
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
    pub(crate) group: String,
    pub(crate) key: Option<String>,
    pub(crate) recent: bool,
    pub(crate) plugin_id: Option<String>,
    pub(crate) theme: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CommandPaletteMode {
    #[default]
    All,
    History,
    Keybinds,
}

#[derive(Debug, Clone)]
pub(crate) struct CommandPaletteState {
    pub(crate) table: SelectionTable<CommandPaletteItem>,
    pub(crate) theme_picker: theme_picker::ThemePicker,
    pub(crate) mode: CommandPaletteMode,
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

/// Result of one asynchronous adapter attach, tagged with its generation.
pub(crate) type DapAttachMessage = (u64, Result<crate::dap::DapClient, String>);

/// Result of one asynchronous panel refresh, tagged with its generation.
pub(crate) type DapRefreshMessage = (
    u64,
    crate::dap::DapClient,
    Result<debug_panel::DebugSnapshot, String>,
);

#[derive(Default)]
pub(crate) struct DapState {
    pub(crate) breakpoints: HashMap<PathBuf, BTreeSet<u32>>,
    pub(crate) client: Option<crate::dap::DapClient>,
    /// Bumped on every attach and detach. Workers echo it back so a late result
    /// cannot replace a newer client or revive a detached one.
    pub(crate) generation: u64,
    pub(crate) attach: Option<mpsc::Receiver<DapAttachMessage>>,
    pub(crate) attach_started: Option<Instant>,
    pub(crate) attach_worker: Option<crate::dap::DapWorker>,
    pub(crate) request: Option<mpsc::Receiver<DapRefreshMessage>>,
    pub(crate) request_started: Option<Instant>,
    pub(crate) request_worker: Option<crate::dap::DapWorker>,
}

#[derive(Default)]
pub(crate) struct DiagnosticsState {
    pub(crate) by_path: HashMap<PathBuf, Vec<lsp::Diagnostic>>,
    pub(crate) count_shown: usize,
    pub(crate) count_pending: Option<(usize, Instant)>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LspStatus {
    Starting {
        root: PathBuf,
    },
    Running {
        root: PathBuf,
        server_info: Option<String>,
    },
    Restarting {
        root: PathBuf,
        error: String,
        attempt: u8,
    },
    Failed {
        root: PathBuf,
        error: String,
    },
    Stopped {
        root: PathBuf,
    },
}

impl LspStatus {
    pub(crate) fn root(&self) -> &Path {
        match self {
            Self::Starting { root }
            | Self::Running { root, .. }
            | Self::Restarting { root, .. }
            | Self::Failed { root, .. }
            | Self::Stopped { root } => root,
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Starting { .. } => "starting",
            Self::Running { .. } => "running",
            Self::Restarting { .. } => "restarting",
            Self::Failed { .. } => "failed",
            Self::Stopped { .. } => "stopped",
        }
    }
}
pub(crate) type LspKey = (String, String, PathBuf);

#[derive(Default)]
pub(crate) struct LspTransportState {
    pub(crate) workers: HashMap<LspKey, LspWorker>,
    pub(crate) config: Option<LspKey>,
    pub(crate) retries: HashMap<LspKey, u8>,
    pub(crate) retry: Option<(LspKey, Instant)>,
    pub(crate) status: HashMap<LspKey, LspStatus>,
    pub(crate) desired: Option<LspKey>,
    pub(crate) labels: HashMap<LspKey, String>,
    pub(crate) pending_sync: Option<(PathBuf, String)>,
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

/// A sent LSP request plus the edit state its response must still match.
#[derive(Debug, Clone)]
pub(crate) struct PendingRequest {
    pub(crate) kind: LspRequestKind,
    pub(crate) edit_revision: Option<u64>,
    pub(crate) edit_path: Option<PathBuf>,
    pub(crate) edit_snapshot: Option<HashMap<PathBuf, u64>>,
}

impl PendingRequest {
    /// Captures the edit state a response is checked against, when `kind` needs it.
    pub(crate) fn capture(
        kind: LspRequestKind,
        revision: u64,
        path: PathBuf,
        snapshot: impl FnOnce() -> HashMap<PathBuf, u64>,
    ) -> Self {
        if !LspRequestsState::tracks_edit(kind) {
            return Self {
                kind,
                edit_revision: None,
                edit_path: None,
                edit_snapshot: None,
            };
        }
        Self {
            kind,
            edit_revision: Some(revision),
            edit_path: Some(path),
            edit_snapshot: Some(snapshot()),
        }
    }
}

/// Why a response no longer describes the live editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StaleResponse {
    /// The buffer was edited after the request was sent.
    EditRevision,
    /// The requested buffer is no longer the active one.
    InactiveBuffer,
}

impl LspRequestsState {
    /// Requests whose edit metadata is captured when they are sent.
    fn tracks_edit(kind: LspRequestKind) -> bool {
        matches!(
            kind,
            LspRequestKind::Rename
                | LspRequestKind::CodeAction
                | LspRequestKind::Formatting
                | LspRequestKind::SignatureHelp
                | LspRequestKind::DocumentHighlight
                | LspRequestKind::CallHierarchyPrepare(_)
                | LspRequestKind::CallHierarchyIncoming
                | LspRequestKind::CallHierarchyOutgoing
                | LspRequestKind::FoldingRange
                | LspRequestKind::SemanticTokens
                | LspRequestKind::CodeLens
                | LspRequestKind::StickySymbols
        )
    }

    /// Requests rejected when the buffer was edited after sending. CodeLens is
    /// excluded because it carries its revision to the response handler instead.
    fn rejects_stale_edit(kind: LspRequestKind) -> bool {
        Self::tracks_edit(kind) && kind != LspRequestKind::CodeLens
    }

    /// Next request id that will be allocated.
    pub(crate) fn peek_request_id(&self) -> u64 {
        self.next_request
    }

    /// Allocates a request id even when the request then fails to send.
    pub(crate) fn reserve_request_id(&mut self) -> u64 {
        let id = self.next_request;
        self.next_request += 1;
        id
    }

    /// Records the document version announced to the server for `path`.
    pub(crate) fn record_version(&mut self, path: &Path) {
        if let Some(version) = self.versions.get_mut(path) {
            *version += 1;
        } else {
            self.versions.insert(path.to_path_buf(), 1);
        }
    }

    /// Tracks a sent request and the edit metadata captured with it.
    pub(crate) fn track(&mut self, id: u64, request: PendingRequest) {
        self.pending.insert(id, request.kind);
        if let Some(revision) = request.edit_revision {
            self.pending_edit_revision.insert(id, revision);
        }
        if let Some(path) = request.edit_path {
            self.pending_edit_path.insert(id, path);
        }
        if let Some(snapshot) = request.edit_snapshot {
            self.pending_edit_snapshot.insert(id, snapshot);
        }
    }

    /// Removes and returns the request `id` identified, if still pending.
    pub(crate) fn take_response(&mut self, id: u64) -> Option<PendingRequest> {
        let kind = self.pending.remove(&id)?;
        Some(PendingRequest {
            kind,
            edit_revision: self.pending_edit_revision.remove(&id),
            edit_path: self.pending_edit_path.remove(&id),
            edit_snapshot: self.pending_edit_snapshot.remove(&id),
        })
    }

    /// Classifies `request` against the live editor revision and active path.
    pub(crate) fn stale_reason(
        &self,
        request: &PendingRequest,
        revision: u64,
        active_path: Option<&Path>,
    ) -> Option<StaleResponse> {
        if Self::rejects_stale_edit(request.kind)
            && request.edit_revision.is_some_and(|sent| sent != revision)
        {
            return Some(StaleResponse::EditRevision);
        }
        if Self::tracks_edit(request.kind)
            && request
                .edit_path
                .as_deref()
                .is_some_and(|sent| active_path != Some(sent))
        {
            return Some(StaleResponse::InactiveBuffer);
        }
        None
    }

    /// Drops edit metadata for requests that are no longer pending.
    pub(crate) fn prune_orphaned_edit_metadata(&mut self) {
        let pending = &self.pending;
        self.pending_edit_revision
            .retain(|id, _| pending.contains_key(id));
        self.pending_edit_path
            .retain(|id, _| pending.contains_key(id));
        self.pending_edit_snapshot
            .retain(|id, _| pending.contains_key(id));
    }

    /// Cancels matching requests and drops the metadata they carried.
    pub(crate) fn cancel(&mut self, matches_kind: impl Fn(LspRequestKind) -> bool) {
        self.pending.retain(|_, kind| !matches_kind(*kind));
        self.prune_orphaned_edit_metadata();
    }

    /// Forgets every tracked request.
    pub(crate) fn clear(&mut self) {
        self.pending.clear();
        self.pending_edit_revision.clear();
        self.pending_edit_path.clear();
        self.pending_edit_snapshot.clear();
    }

    /// Whether a format-on-save round trip must be skipped right now.
    pub(crate) fn format_on_save_blocked(&self) -> bool {
        self.suppress_format_on_save || self.format_on_save_pending
    }

    pub(crate) fn format_on_save_pending(&self) -> bool {
        self.format_on_save_pending
    }

    pub(crate) fn set_format_on_save_pending(&mut self, pending: bool) {
        self.format_on_save_pending = pending;
    }

    pub(crate) fn set_suppress_format_on_save(&mut self, suppress: bool) {
        self.suppress_format_on_save = suppress;
    }
}

#[derive(Default)]
pub(crate) struct LspResultsState {
    pub(crate) hover: Option<String>,
    pub(crate) code_actions: Option<CodeActionState>,
    pub(crate) document_symbols: Option<SymbolState>,
    pub(crate) references: Option<ReferenceState>,
    pub(crate) document_highlights: Option<DocumentHighlightState>,
    pub(crate) call_hierarchy: Option<CallHierarchyState>,
    pub(crate) signature_help: Option<SignatureHelpState>,
    pub(crate) workspace_symbols: Option<WorkspaceSymbolState>,
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
    /// Recent workspaces, files, and quick-open ranking.
    pub(crate) recent: RecentState,
    /// Workspace tree view and refresh state.
    pub(crate) tree: TreeState,
    /// Git workspace, request, and view state.
    pub(crate) git: GitState,
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
    /// Optional trusted Git service installed by the distribution.
    pub(crate) git_service: Option<std::sync::Arc<dyn anvil_module::git::GitService>>,
    /// Optional trusted workspace-index service installed by the distribution.
    pub(crate) workspace_index_service:
        Option<std::sync::Arc<dyn anvil_module::workspace_index::WorkspaceIndexService>>,
    /// Optional trusted desktop URL service installed by the distribution.
    pub(crate) desktop_service: Option<std::sync::Arc<dyn anvil_module::desktop::DesktopService>>,
    /// Optional trusted language-tooling provider installed by the distribution.
    pub(crate) language_tooling_service:
        Option<std::sync::Arc<dyn anvil_module::language::LanguageToolingService>>,
    /// Cancellation token for the active quick-open enumeration.
    pub(crate) fuzzy_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Dirty swap files that must survive a source conflict.
    pub(crate) conflict_swaps: HashMap<PathBuf, PathBuf>,
    /// Paths changed by a multi-file replace whose buffer reload failed.
    pub(crate) stale_paths: HashSet<PathBuf>,
}
pub(crate) struct WorkspacePoll {
    pub(crate) file_events: Vec<anvil_workspace::FileEvent>,
    pub(crate) root_events: Vec<(PathBuf, anvil_workspace::FileEvent)>,
}

impl WorkspaceState {
    pub(crate) fn root_label(&self, root: &Path) -> String {
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let duplicates = self
            .roots
            .list()
            .iter()
            .filter(|candidate| candidate.path.file_name() == root.file_name())
            .count();
        if duplicates <= 1 {
            return name;
        }
        let Some(parent) = root.parent().and_then(Path::file_name) else {
            return root.display().to_string();
        };
        let short = format!("{}/{}", parent.to_string_lossy(), name);
        let short_duplicates = self
            .roots
            .list()
            .iter()
            .filter(|candidate| {
                candidate.path.file_name() == root.file_name()
                    && candidate.path.parent().and_then(Path::file_name) == Some(parent)
            })
            .count();
        if short_duplicates > 1 {
            root.display().to_string()
        } else {
            short
        }
    }

    pub(crate) fn set_watch_enabled(&mut self, enabled: bool) {
        if let Some(worker) = &self.file_worker {
            let _ = worker.request(anvil_workspace::FileRequest::SetWatchEnabled(enabled));
        }
        for worker in self.root_file_workers.values() {
            let _ = worker.request(anvil_workspace::FileRequest::SetWatchEnabled(enabled));
        }
    }

    pub(crate) fn set_watch_config(&mut self, mode: &str, max_dirs: usize, cross_mounts: bool) {
        let config = || anvil_workspace::FileRequest::SetWatchConfig {
            mode: mode.to_owned(),
            max_dirs,
            cross_mounts,
        };
        if let Some(worker) = &self.file_worker {
            let _ = worker.request(config());
        }
        for worker in self.root_file_workers.values() {
            let _ = worker.request(config());
        }
    }

    pub(crate) fn poll(&mut self) -> WorkspacePoll {
        let file_events = self
            .file_worker
            .as_ref()
            .map(|worker| worker.poll())
            .unwrap_or_default();
        let roots = self.root_file_workers.keys().cloned().collect::<Vec<_>>();
        let mut root_events = Vec::new();
        for root in roots {
            let events = self
                .root_file_workers
                .get(&root)
                .map(|worker| worker.poll())
                .unwrap_or_default();
            root_events.extend(events.into_iter().map(|event| (root.clone(), event)));
        }
        WorkspacePoll {
            file_events,
            root_events,
        }
    }
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
    pub(crate) replace_plan: Option<anvil_workspace::ProjectReplacePlan>,
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

#[cfg(target_os = "linux")]
pub(crate) struct DynamicPluginCommand {
    pub(crate) plugin_id: String,
    pub(crate) title: String,
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
    #[cfg(target_os = "linux")]
    pub(crate) dynamic_commands: BTreeMap<String, DynamicPluginCommand>,
    pub(crate) plugin_providers: PluginProviderRegistry,
    pub(crate) plugin_provider_requests: PluginProviderRequests,
    pub(crate) provider_snapshots: BTreeMap<u64, (u64, usize)>,
    pub(crate) provider_action_generation: u64,
    pub(crate) pkg_install: Option<pkg_install::PkgInstallState>,
}

impl Default for PluginState {
    fn default() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            entries: Vec::new(),
            #[cfg(target_os = "linux")]
            supervisor: PluginSupervisor::new(std::sync::Arc::new(
                crate::plugin_host::SystemHostSpawner,
            )),
            #[cfg(target_os = "linux")]
            manager: None,
            #[cfg(target_os = "linux")]
            store_size: 0,
            #[cfg(target_os = "linux")]
            installer: None,
            #[cfg(target_os = "linux")]
            dynamic_commands: BTreeMap::new(),
            plugin_providers: PluginProviderRegistry::new(),
            plugin_provider_requests: PluginProviderRequests::new(),
            provider_snapshots: BTreeMap::new(),
            provider_action_generation: 0,
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

impl EditSessionState {
    pub(crate) fn capture_insert(&mut self, text: &str) {
        if let Some(session) = self.insert_session.as_mut() {
            session.text.push_str(text);
        }
    }

    pub(crate) fn trim_insert(&mut self, at_end: bool) {
        let Some(session) = self.insert_session.as_mut() else {
            return;
        };
        if !at_end {
            session.valid = false;
            return;
        }
        let _ = session.text.pop();
    }
}

#[derive(Default)]
pub struct StatusState {
    /// Short status line.
    pub(crate) text: String,
    /// Last observed message, used to timestamp direct `text` writes.
    pub(crate) text_seen: String,
    pub(crate) text_at: Option<Instant>,
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
    /// Sparse user overrides loaded from the user settings file.
    pub(crate) user_settings: settings::SettingsPatch,
    /// Effective settings without the user layer, used by reset.
    pub(crate) inherited_settings: Settings,
    /// Effective settings layer for each field.
    pub(crate) sources: settings::SettingsSources,
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
    /// Monotonic external formatter request id.
    pub(crate) format_generation: u64,
    /// In-flight external formatter request.
    pub(crate) format_pending: Option<u64>,
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
    /// Bounded committed search history, newest first.
    pub(crate) history: Vec<String>,
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

pub(crate) struct BufferState {
    /// All open buffers, including the active one after the first switch.
    pub(crate) entries: Vec<Buffer>,
    /// Stable identity of the active editor document.
    pub(crate) active_id: BufferId,
    /// Active buffer index.
    pub(crate) active: usize,
    /// Secondary pane when a vertical split is open.
    pub(crate) split: Option<Pane>,
    /// Internal buffers for unopened files changed by workspace edits.
    pub(crate) background: HashMap<PathBuf, Buffer>,
}

impl Default for BufferState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            active_id: BufferId::next(),
            active: 0,
            split: None,
            background: HashMap::new(),
        }
    }
}

#[derive(Default)]
pub(crate) struct RuntimeState {
    /// Plugin discovery, host, manager, installer, and package install state.
    pub(crate) plugins: PluginState,
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

impl RuntimeState {
    pub(crate) fn poll_task(&mut self) -> Option<task_runner::TaskEvent> {
        let event = self.task_worker.as_mut()?.try_recv()?;
        self.task_worker = None;
        Some(event)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn poll_plugins(
        &mut self,
        gateway: Option<&mut (dyn anvil_ipc::PluginGateway + 'static)>,
    ) -> Vec<SupervisorEvent> {
        self.plugins.supervisor.poll(gateway)
    }
}

#[cfg(unix)]
pub(crate) struct TerminalPoll {
    pub(crate) output: bool,
    pub(crate) error: Option<String>,
    pub(crate) exited: Option<String>,
}

#[cfg(unix)]
impl RuntimeState {
    pub(crate) fn push_terminal_output(&mut self, bytes: &[u8]) {
        self.terminal_output.extend_from_slice(bytes);
        if self.terminal_output.len() > pty_terminal::MAX_TERMINAL_OUTPUT {
            let excess = self.terminal_output.len() - pty_terminal::MAX_TERMINAL_OUTPUT;
            self.terminal_output.drain(..excess);
        }
    }

    pub(crate) fn poll_terminal(&mut self) -> Option<TerminalPoll> {
        let (exited, read) = {
            let terminal = self.terminal.as_mut()?;
            (
                terminal.try_wait().ok().flatten(),
                terminal.read_available(),
            )
        };
        let mut output = false;
        let mut error = None;
        match read {
            Ok(bytes) if !bytes.is_empty() => {
                self.push_terminal_output(&bytes);
                output = true;
            }
            Ok(_) => {}
            Err(read_error) => error = Some(read_error.to_string()),
        }
        Some(TerminalPoll {
            output,
            error,
            exited: exited.map(|status| status.to_string()),
        })
    }
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

impl RecentState {
    pub(crate) fn install(&mut self, recents: Recents) {
        self.workspaces = recents.workspaces;
        self.files = recents.files;
    }

    pub(crate) fn record_workspace(&mut self, path: &Path) {
        recents::push_recent(&mut self.workspaces, path);
    }

    pub(crate) fn record_file(&mut self, path: &Path) {
        recents::push_recent(&mut self.files, path);
    }

    pub(crate) fn save(&self) {
        if !self.persist {
            return;
        }
        let recents = Recents::new(self.workspaces.clone(), self.files.clone());
        let _ = recents::save(&recents);
    }

    pub(crate) fn openable_files(&self, roots: &workspace_roots::WorkspaceRoots) -> Vec<PathBuf> {
        self.files
            .iter()
            .filter(|path| roots.owning_root(path).is_some_and(|root| root.trusted))
            .cloned()
            .collect()
    }
}

#[derive(Clone, Default)]
pub(crate) struct WatchRootStatus {
    pub(crate) coverage: String,
    pub(crate) watched_dirs: usize,
    pub(crate) skipped_dirs: usize,
    pub(crate) overflow_count: u64,
    pub(crate) reason: Option<String>,
}

#[derive(Default)]
pub(crate) struct TreeState {
    /// Flattened workspace tree, when open.
    pub(crate) entries: Vec<TreeEntry>,
    pub(crate) selected: Option<usize>,
    pub(crate) visible: bool,
    /// Entry to select after the next tree refresh.
    pub(crate) pending_select: Option<PathBuf>,
    /// Entry whose ancestors are being expanded by a reveal request.
    pub(crate) pending_reveal: Option<PathBuf>,
    pub(crate) reveal_steps: usize,
    pub(crate) find: Option<TreeFindState>,
    pub(crate) stale: bool,
    pub(crate) watch_limited: bool,

    pub(crate) watch: BTreeMap<String, WatchRootStatus>,
    pub(crate) refresh_in_flight: bool,
    pub(crate) refresh_queued: bool,
    pub(crate) refresh_generation: u64,
    pub(crate) show_hidden: bool,
}

impl TreeState {
    pub(crate) fn entry_visible(&self, name: &str, is_dir: bool) -> bool {
        !is_dir
            || self.show_hidden
            || !name.starts_with('.')
            || matches!(name, ".github" | ".cargo")
    }
}

#[derive(Default)]
pub(crate) struct TreeFindState {
    pub(crate) query: String,
    pub(crate) matches: Vec<usize>,
    pub(crate) current: usize,
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
    /// Deadline for a coalesced external-change Git status refresh.
    pub(crate) status_debounce: Option<Instant>,
    /// Requested git diff path and revision.
    pub(crate) diff_request: Option<(PathBuf, u64)>,
    /// Last requested git diff path and revision.
    pub(crate) diff_checked: Option<(PathBuf, u64)>,
    /// Debounce deadline for the next git diff request.
    pub(crate) diff_due: Option<Instant>,
    /// Latest requested read-only Git diff generation.
    pub(crate) diff_generation: u64,
    /// Path whose diff is being loaded or displayed.
    pub(crate) diff_path: Option<PathBuf>,
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
    /// Number of root workers expected to answer the conflict-list request.
    pub(crate) conflicts_expected: usize,
    /// Number of root workers that answered the conflict-list request.
    pub(crate) conflicts_received: usize,
    /// Root-qualified conflict-list results for the pending generation.
    pub(crate) conflicts_results: Vec<(PathBuf, GitConflictResult)>,
    /// Latest requested conflict-stage generation.
    pub(crate) stage_generation: u64,
    /// Pending conflict-stage generation, if a request is in flight.
    pub(crate) stage_pending: Option<u64>,
    /// Latest requested conflict-resolution generation.
    pub(crate) resolve_generation: u64,
    /// Path whose conflict is being resolved.
    pub(crate) resolve_path: Option<PathBuf>,
    /// Pending conflict-resolution generation, if a request is in flight.
    pub(crate) resolve_pending: Option<u64>,
    /// Lazily detected branch name per workspace root.
    pub(crate) branches: BTreeMap<PathBuf, Option<String>>,
}

impl GitState {
    pub(crate) fn schedule_status_refresh(&mut self, now: Instant) {
        self.status_debounce
            .get_or_insert(now + GIT_STATUS_DEBOUNCE);
    }

    pub(crate) fn take_due_status_refresh(&mut self, now: Instant) -> bool {
        if self.status_debounce.is_none_or(|due| now < due) {
            return false;
        }
        self.status_debounce = None;
        true
    }
}

/// Inlay hints cached for the visible viewport.
#[derive(Default)]
pub(crate) struct InlayHintsState {
    pub(crate) hints: Vec<InlayHint>,
    pub(crate) pending: Option<(u64, PathBuf, u64, usize, usize)>,
    pub(crate) checked: Option<(PathBuf, u64, usize, usize)>,
    pub(crate) support: HashMap<LspKey, bool>,
}

/// Completion popup state, pending resolve request, and active snippet session.
#[derive(Default)]
pub(crate) struct CompletionUiState {
    pub(crate) completion: Option<CompletionState>,
    pub(crate) resolve: Option<(u64, usize)>,
    pub(crate) snippet: Option<SnippetSession>,
}

/// Semantic token legend, cached tokens, and the revision they describe.
#[derive(Default)]
pub(crate) struct SemanticTokensState {
    pub(crate) legend: Vec<String>,
    pub(crate) tokens: Vec<SemanticToken>,
    pub(crate) revision: Option<u64>,
}

#[derive(Default)]
pub(crate) struct LanguageState {
    pub(crate) dap: DapState,
    pub(crate) diagnostics: DiagnosticsState,
    pub(crate) lsp: LspTransportState,
    pub(crate) lsp_requests: LspRequestsState,
    pub(crate) semantic_tokens: SemanticTokensState,
    pub(crate) inlay_hints: InlayHintsState,
    pub(crate) code_lens: Option<code_lens::CodeLensState>,
    pub(crate) completion: CompletionUiState,
}
pub(crate) enum LspPollEffect {
    Event { key: LspKey, event: lsp::LspEvent },
    Retry { key: LspKey },
}

impl LanguageState {
    /// Polls LSP workers and the retry deadline, returning effects for Model.
    pub(crate) fn poll(&mut self, now: Instant) -> Vec<LspPollEffect> {
        let keys: Vec<LspKey> = self.lsp.workers.keys().cloned().collect();
        let mut effects = Vec::with_capacity(keys.len() + 1);
        for key in keys {
            let events = self
                .lsp
                .workers
                .get(&key)
                .map(lsp::LspWorker::poll)
                .unwrap_or_default();
            effects.extend(events.into_iter().map(|event| LspPollEffect::Event {
                key: key.clone(),
                event,
            }));
        }
        if let Some((key, deadline)) = self.lsp.retry.clone()
            && now >= deadline
        {
            self.lsp.retry = None;
            if !self.lsp.workers.contains_key(&key) {
                effects.push(LspPollEffect::Retry { key });
            }
        }
        effects
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OverlayKind {
    CommandPalette,
    Settings,
    BufferPicker,
    Fuzzy,
    Problems,
    WorkspaceSymbols,
    DocumentSymbols,
    References,
    CallHierarchy,
    CodeActions,
    DiagnosticPeek,
    Hover,
    ThemeBrowser,
    DebugPanel,
    TestExplorer,
    Completion,
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
    pub(crate) completion_generation: u64,
    pub(crate) command_palette_recents: Vec<String>,
    pub(crate) location: LocationPanelState,
    pub(crate) lsp_results: LspResultsState,
}

#[derive(Debug, Clone)]
pub(crate) struct CompletionItem {
    pub(crate) source: ProviderId,
    pub(crate) label: String,
    pub(crate) filter_text: String,
    pub(crate) sort_text: String,
    pub(crate) preselect: bool,
    pub(crate) insert: String,
    pub(crate) snippet: Option<Snippet>,
    pub(crate) detail: String,
    pub(crate) replace: Option<(usize, usize, usize, usize)>,
    pub(crate) replace_bytes: Option<(usize, usize)>,
    pub(crate) documentation: Option<String>,
    pub(crate) raw: serde_json::Value,
    pub(crate) resolved: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct CompletionState {
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
    Declaration,
    TypeDefinition,
    Implementation,
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
    CodeActionCommand,
    CodeLens,
}

#[derive(Debug, Clone)]
pub(crate) struct CodeActionCommand {
    pub(crate) command: String,
    pub(crate) arguments: serde_json::Value,
}

#[derive(Debug, Clone)]
pub(crate) struct CodeActionItem {
    pub(crate) title: String,
    pub(crate) edit: Option<WorkspaceEdit>,
    pub(crate) command: Option<CodeActionCommand>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn request(kind: LspRequestKind, revision: u64, path: &str) -> PendingRequest {
        PendingRequest::capture(kind, revision, PathBuf::from(path), || {
            HashMap::from([(PathBuf::from(path), revision)])
        })
    }

    #[test]
    fn peeking_request_ids_does_not_consume_them() {
        let mut state = LspRequestsState::default();
        assert_eq!(state.peek_request_id(), 0);
        assert_eq!(state.reserve_request_id(), 0);
        assert_eq!(state.peek_request_id(), 1);
        assert_eq!(state.reserve_request_id(), 1);
    }

    #[test]
    fn tracks_edit_metadata_only_for_edit_requests() {
        let mut state = LspRequestsState::default();
        let format = state.reserve_request_id();
        state.track(format, request(LspRequestKind::Formatting, 2, "a.rs"));
        assert_eq!(
            state.pending.get(&format),
            Some(&LspRequestKind::Formatting)
        );
        assert_eq!(state.pending_edit_revision.get(&format), Some(&2));
        assert_eq!(
            state.pending_edit_path.get(&format),
            Some(&PathBuf::from("a.rs"))
        );
        assert_eq!(
            state
                .pending_edit_snapshot
                .get(&format)
                .and_then(|snapshot| snapshot.get(&PathBuf::from("a.rs"))),
            Some(&2)
        );

        let hover = state.reserve_request_id();
        state.track(hover, request(LspRequestKind::Hover, 3, "a.rs"));
        assert_eq!(state.pending.get(&hover), Some(&LspRequestKind::Hover));
        assert!(!state.pending_edit_revision.contains_key(&hover));
        assert!(!state.pending_edit_path.contains_key(&hover));
        assert!(!state.pending_edit_snapshot.contains_key(&hover));
    }

    #[test]
    fn classifies_stale_responses_against_live_edit_state() {
        let mut state = LspRequestsState::default();
        let id = state.reserve_request_id();
        state.track(id, request(LspRequestKind::Formatting, 1, "a.rs"));
        let tracked = state.take_response(id).expect("tracked");
        let path = PathBuf::from("a.rs");
        assert_eq!(
            state.stale_reason(&tracked, 2, Some(path.as_path())),
            Some(StaleResponse::EditRevision)
        );
        assert_eq!(
            state.stale_reason(&tracked, 1, Some(Path::new("b.rs"))),
            Some(StaleResponse::InactiveBuffer)
        );
        assert_eq!(state.stale_reason(&tracked, 1, Some(path.as_path())), None);
        assert!(state.pending.is_empty());
        assert!(state.pending_edit_path.is_empty());
    }

    #[test]
    fn code_lens_survives_edits_but_not_a_buffer_switch() {
        let mut state = LspRequestsState::default();
        let id = state.reserve_request_id();
        state.track(id, request(LspRequestKind::CodeLens, 1, "a.rs"));
        let tracked = state.take_response(id).expect("tracked");
        assert_eq!(
            state.stale_reason(&tracked, 2, Some(Path::new("a.rs"))),
            None
        );
        assert_eq!(
            state.stale_reason(&tracked, 1, Some(Path::new("b.rs"))),
            Some(StaleResponse::InactiveBuffer)
        );
    }

    #[test]
    fn cancelling_drops_pending_requests_and_their_edit_metadata() {
        let mut state = LspRequestsState::default();
        let format = state.reserve_request_id();
        state.track(format, request(LspRequestKind::Formatting, 1, "a.rs"));
        let rename = state.reserve_request_id();
        state.track(rename, request(LspRequestKind::Rename, 1, "a.rs"));
        state.cancel(|kind| kind == LspRequestKind::Formatting);
        assert!(!state.pending.contains_key(&format));
        assert!(!state.pending_edit_revision.contains_key(&format));
        assert!(!state.pending_edit_path.contains_key(&format));
        assert!(!state.pending_edit_snapshot.contains_key(&format));
        assert!(state.pending_edit_path.contains_key(&rename));

        state.clear();
        assert!(state.pending.is_empty());
        assert!(state.pending_edit_revision.is_empty());
        assert!(state.pending_edit_path.is_empty());
        assert!(state.pending_edit_snapshot.is_empty());
    }

    #[test]
    fn recording_a_document_version_starts_at_one_and_increments() {
        let mut state = LspRequestsState::default();
        let path = PathBuf::from("a.rs");
        state.record_version(&path);
        assert_eq!(state.versions.get(&path), Some(&1));
        state.record_version(&path);
        assert_eq!(state.versions.get(&path), Some(&2));
    }

    #[test]
    fn coalesces_git_status_refresh_bursts() {
        let mut state = GitState::default();
        let start = Instant::now();
        state.schedule_status_refresh(start);
        let due = state.status_debounce.expect("deadline");
        state.schedule_status_refresh(start + Duration::from_millis(100));
        assert_eq!(state.status_debounce, Some(due));
        assert!(!state.take_due_status_refresh(start + Duration::from_millis(299)));
        assert!(state.take_due_status_refresh(start + Duration::from_millis(300)));
        assert!(state.status_debounce.is_none());
    }
}

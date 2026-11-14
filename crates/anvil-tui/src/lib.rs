//! Ratatui editor shell and shared model.
//!
//! LSP behavior, file-tree state, rendering, and key dispatch live in focused
//! sibling modules; this module keeps buffer/model state and composition.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs, io,
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
use crossterm::event::{self, Event, KeyEvent};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

mod bracket;
mod breadcrumbs;
mod buffer;
mod command_line;
mod command_palette;
mod commands;
pub(crate) use anvil_editor::comment;
mod completion_sources;
mod debug_panel;
mod debugger;
mod editing;
mod file_io;
mod file_tree_ui;
mod folding;
mod format;
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
mod lsp_ui;
mod modal;
mod mouse;
mod overlay_stack;
mod overlays;
mod pipe_locations;
#[cfg(unix)]
pub mod pty_terminal;
mod recents;
mod remote;
mod session;
mod theme_browser;
mod theme_picker;
mod workspace_control;
use anvil_tui_kit::{SelectionItem, SelectionTable};
pub use anvil_workspace::roots as workspace_roots;
mod pkg_install;
mod plugin_activation;
mod plugin_discovery;
mod plugin_document;
#[cfg(target_os = "linux")]
mod plugin_manager;
mod plugins;
mod project_replace;
mod repeat;
pub(crate) use anvil_editor::search;
mod search_ui;
mod splits;
mod status_line;
pub(crate) use anvil_tui_kit::input;
#[cfg(test)]
use lsp_ui::lsp_settings;
use lsp_ui::lsp_settings_with_service;
mod keymap;
mod keys;
mod lsp_runtime;
mod model;
mod readonly;
mod render;
#[cfg(test)]
use readonly::HELP_TEXT;
use readonly::ReadOnlyState;
mod settings;
pub(crate) use anvil_tui_theme::theme;
use settings::Settings;
mod settings_ui;
pub(crate) use anvil_editor::snippet;
use anvil_editor::snippet::{Snippet, SnippetSession};
use settings_ui::SettingsOverlayState;
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
mod task_runner;
mod task_ui;
mod terminal;
mod terminal_background;
#[cfg(unix)]
mod terminal_control;
mod test_explorer;
use anvil_lsp_client::{LspConfig, LspEvent, LspWorker};
#[cfg(target_os = "linux")]
use anvil_session::plugin_session::{DEBUG_PERMISSIONS, DEBUG_PLUGIN_ID};
#[cfg(target_os = "linux")]
use anvil_session::plugin_supervisor::{
    ExecOutcome, HostState, PluginSupervisor, StartMode, StartOutcome, SupervisorEvent,
};
use anvil_syntax::SyntaxState;
use anvil_workspace::FileWorker;
use gateway::GatewaySession;
#[cfg(target_os = "linux")]
use plugin_activation::ActivationEvent;
pub use terminal::{
    DistributionServices, run, run_with_all_services, run_with_input, run_with_input_and_git,
    run_with_services, run_with_workspace, run_with_workspace_status,
};

const MAX_OPEN_BYTES: usize = 256 * 1024 * 1024;
const EFFECT_QUEUE_CAPACITY: usize = 32;

mod state;
use state::*;
pub use state::{EditorSessionState, FileState, StatusState, ViewState, WordMotion};

#[cfg(target_os = "linux")]
fn startup_activation_allowed() -> bool {
    std::env::var_os("ANVIL_PLUGIN_ALLOW_STARTUP")
        .is_some_and(|value| value == "1" || value == "true")
}

#[cfg(target_os = "linux")]
fn workspace_activation_enabled() -> bool {
    std::env::var_os("ANVIL_PLUGIN_DISABLE_WORKSPACE_ACTIVATION").is_none()
}

pub(crate) type GitConflictResult = Result<(String, bool), String>;

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

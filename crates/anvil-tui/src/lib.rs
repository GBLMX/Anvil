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
mod overlay_stack;
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

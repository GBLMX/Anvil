//! Start dashboard focus, actions, and contextual recents.

use super::*;
use crate::state::StartPageFocus;
use anvil_session::command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartAction {
    Insert,
    FindFile,
    FileTree,
    Buffers,
    Palette,
    RecentFiles,
    Workspaces,
    Help,
    Quit,
}

pub(crate) struct StartActionSpec {
    pub(crate) action: StartAction,
    pub(crate) command: Option<command::CommandId>,
    pub(crate) label: &'static str,
    pub(crate) fallback_key: &'static str,
}

pub(crate) const START_ACTIONS: &[StartActionSpec] = &[
    StartActionSpec {
        action: StartAction::Insert,
        command: Some(command::INSERT_HERE),
        label: "insert",
        fallback_key: "i",
    },
    StartActionSpec {
        action: StartAction::FindFile,
        command: Some(command::FUZZY),
        label: "find file",
        fallback_key: "Space f",
    },
    StartActionSpec {
        action: StartAction::FileTree,
        command: Some(command::TREE_OPEN),
        label: "file tree",
        fallback_key: "Space e",
    },
    StartActionSpec {
        action: StartAction::Buffers,
        command: None,
        label: "buffers",
        fallback_key: "Space b",
    },
    StartActionSpec {
        action: StartAction::Palette,
        command: Some(command::COMMAND_PALETTE),
        label: "commands",
        fallback_key: "Space p",
    },
    StartActionSpec {
        action: StartAction::RecentFiles,
        command: None,
        label: "recent files",
        fallback_key: "r",
    },
    StartActionSpec {
        action: StartAction::Workspaces,
        command: None,
        label: "recent workspaces",
        fallback_key: "w",
    },
    StartActionSpec {
        action: StartAction::Help,
        command: None,
        label: "help",
        fallback_key: "?",
    },
    StartActionSpec {
        action: StartAction::Quit,
        command: None,
        label: "quit",
        fallback_key: ":q",
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StartRecentFile {
    pub(crate) path: PathBuf,
    pub(crate) label: String,
    pub(crate) missing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartWorkspaceState {
    Current,
    Available,
    Missing,
    Untrusted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StartRecentWorkspace {
    pub(crate) path: PathBuf,
    pub(crate) label: String,
    pub(crate) detail: String,
    pub(crate) state: StartWorkspaceState,
}

impl Model {
    pub(crate) fn close_start_page(&mut self) {
        self.start_page.pending_space = false;
        if self.start_page.welcome {
            self.start_page.welcome = false;
            self.file.dirty = true;
        }
    }

    pub(crate) fn open_start_page_workspaces(&mut self) {
        self.start_page.welcome = true;
        self.start_page.pending_space = false;
        self.start_page.focus = StartPageFocus::Workspaces;
        self.clamp_start_selection();
        self.file.dirty = true;
    }

    pub(crate) fn start_action_key(&self, action: &StartActionSpec) -> String {
        action
            .command
            .and_then(|command| self.config.keymap.key_for_command(command.as_str()))
            .filter(|key| !key.is_empty())
            .unwrap_or_else(|| action.fallback_key.to_owned())
    }

    pub(crate) fn start_action_disabled_reason(&self, action: StartAction) -> Option<&'static str> {
        match action {
            StartAction::FileTree
                if !self
                    .workspace
                    .roots
                    .primary()
                    .is_some_and(|root| root.trusted) =>
            {
                Some("workspace root is untrusted")
            }
            StartAction::RecentFiles if self.openable_recent_files().is_empty() => {
                Some("no recent files")
            }
            StartAction::Workspaces if self.start_recent_workspaces().is_empty() => {
                Some("no recent workspaces")
            }
            _ => None,
        }
    }

    pub(crate) fn start_focus_actions(&mut self) {
        self.start_page.focus = StartPageFocus::Actions;
        self.file.dirty = true;
    }

    pub(crate) fn start_focus_files(&mut self) {
        self.start_page.focus = StartPageFocus::Files;
        self.file.dirty = true;
    }

    pub(crate) fn start_focus_workspaces(&mut self) {
        self.start_page.focus = StartPageFocus::Workspaces;
        self.file.dirty = true;
    }

    pub(crate) fn start_focus_next(&mut self) {
        self.start_page.focus = match self.start_page.focus {
            StartPageFocus::Actions => StartPageFocus::Files,
            StartPageFocus::Files => StartPageFocus::Workspaces,
            StartPageFocus::Workspaces => StartPageFocus::Actions,
        };
        self.clamp_start_selection();
        self.file.dirty = true;
    }

    pub(crate) fn start_focus_previous(&mut self) {
        self.start_page.focus = match self.start_page.focus {
            StartPageFocus::Actions => StartPageFocus::Workspaces,
            StartPageFocus::Files => StartPageFocus::Actions,
            StartPageFocus::Workspaces => StartPageFocus::Files,
        };
        self.clamp_start_selection();
        self.file.dirty = true;
    }

    pub(crate) fn move_start_selection(&mut self, delta: isize) {
        match self.start_page.focus {
            StartPageFocus::Actions => {
                self.start_page.action_selected = self
                    .start_page
                    .action_selected
                    .saturating_add_signed(delta)
                    .min(START_ACTIONS.len().saturating_sub(1));
            }
            StartPageFocus::Files => {
                self.start_page.file_selected = self
                    .start_page
                    .file_selected
                    .saturating_add_signed(delta)
                    .min(self.start_file_rows().saturating_sub(1));
            }
            StartPageFocus::Workspaces => {
                self.start_page.workspace_selected = self
                    .start_page
                    .workspace_selected
                    .saturating_add_signed(delta)
                    .min(self.start_recent_workspaces().len().saturating_sub(1));
            }
        }
        self.file.dirty = true;
    }

    pub(crate) fn clamp_start_selection(&mut self) {
        self.start_page.action_selected = self
            .start_page
            .action_selected
            .min(START_ACTIONS.len().saturating_sub(1));
        self.start_page.file_selected = self
            .start_page
            .file_selected
            .min(self.start_file_rows().saturating_sub(1));
        self.start_page.workspace_selected = self
            .start_page
            .workspace_selected
            .min(self.start_recent_workspaces().len().saturating_sub(1));
    }

    pub(crate) fn start_file_rows(&self) -> usize {
        usize::from(self.start_page.session_available) + self.start_recent_files().len()
    }

    pub(crate) fn start_recent_files(&self) -> Vec<StartRecentFile> {
        self.openable_recent_files()
            .into_iter()
            .map(|path| {
                let missing = !path.is_file();
                let display = self
                    .workspace
                    .roots
                    .owning_root(&path)
                    .and_then(|root| {
                        let relative = path.strip_prefix(&root.path).ok()?;
                        if root.path == self.workspace.path {
                            Some(relative.display().to_string())
                        } else {
                            let label = root
                                .path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| root.path.display().to_string());
                            Some(format!("{label}/{}", relative.display()))
                        }
                    })
                    .unwrap_or_else(|| path.display().to_string());
                StartRecentFile {
                    path,
                    label: display,
                    missing,
                }
            })
            .collect()
    }

    pub(crate) fn start_recent_workspaces(&self) -> Vec<StartRecentWorkspace> {
        let mut seen = HashSet::new();
        let current = self
            .workspace
            .path
            .canonicalize()
            .unwrap_or_else(|_| self.workspace.path.clone());
        let mut workspaces = Vec::new();
        for path in &self.workspace.recent.workspaces {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !seen.insert(canonical.clone()) {
                continue;
            }
            let missing = !canonical.is_dir();
            let trusted = self
                .workspace
                .roots
                .list()
                .iter()
                .find(|root| root.path == canonical)
                .is_none_or(|root| root.trusted);
            let state = if canonical == current {
                StartWorkspaceState::Current
            } else if missing {
                StartWorkspaceState::Missing
            } else if !trusted {
                StartWorkspaceState::Untrusted
            } else {
                StartWorkspaceState::Available
            };
            let mut components = canonical
                .components()
                .rev()
                .filter_map(|component| component.as_os_str().to_str())
                .filter(|part| !part.is_empty() && *part != "/")
                .take(2)
                .collect::<Vec<_>>();
            components.reverse();
            workspaces.push(StartRecentWorkspace {
                label: components.join("/"),
                detail: canonical.display().to_string(),
                path: canonical,
                state,
            });
        }
        workspaces
    }

    pub(crate) fn activate_start_selection(&mut self) {
        match self.start_page.focus {
            StartPageFocus::Actions => {
                self.activate_start_action(self.start_page.action_selected);
            }
            StartPageFocus::Files => self.activate_start_file(self.start_page.file_selected),
            StartPageFocus::Workspaces => {
                self.activate_start_workspace(self.start_page.workspace_selected);
            }
        }
    }

    pub(crate) fn start_action_index(&self, action: StartAction) -> usize {
        START_ACTIONS
            .iter()
            .position(|spec| spec.action == action)
            .unwrap_or(0)
    }

    pub(crate) fn activate_start_action(&mut self, index: usize) {
        let Some(spec) = START_ACTIONS.get(index) else {
            return;
        };
        if let Some(reason) = self.start_action_disabled_reason(spec.action) {
            self.status.text = format!("E: {reason}");
            self.file.dirty = true;
            return;
        }
        match spec.action {
            StartAction::Insert => {
                self.close_start_page();
                self.insert_here();
            }
            StartAction::FindFile => {
                self.close_start_page();
                self.start_fuzzy();
            }
            StartAction::FileTree => {
                self.close_start_page();
                self.open_tree();
            }
            StartAction::Buffers => {
                self.close_start_page();
                self.open_buffer_picker();
            }
            StartAction::Palette => {
                self.close_start_page();
                self.open_command_palette();
            }
            StartAction::RecentFiles => {
                self.close_start_page();
                self.open_recent_files();
            }
            StartAction::Workspaces => self.start_focus_workspaces(),
            StartAction::Help => self.show_help(),
            StartAction::Quit => {
                self.close_start_page();
                self.execute_command("q");
            }
        }
    }

    pub(crate) fn activate_start_file(&mut self, index: usize) {
        if self.start_page.session_available && index == 0 {
            match crate::session::restore(self, &self.workspace.path.clone()) {
                Ok(true) => self.close_start_page(),
                Ok(false) => {
                    self.start_page.session_available = false;
                    self.status.text = "no restorable session".to_owned();
                    self.file.dirty = true;
                }
                Err(error) => {
                    self.status.text = format!("E: session: {error}");
                    self.file.dirty = true;
                }
            }
            return;
        }
        let index = index.saturating_sub(usize::from(self.start_page.session_available));
        let Some(file) = self.start_recent_files().get(index).cloned() else {
            return;
        };
        if file.missing {
            self.status.text = format!("E: missing file: {}", file.path.display());
            self.file.dirty = true;
            return;
        }
        self.close_start_page();
        if let Err(error) = self.open_path(&file.path) {
            self.status.text = format!("E: {error}");
        }
        self.file.dirty = true;
    }

    pub(crate) fn activate_start_workspace(&mut self, index: usize) {
        let Some(workspace) = self.start_recent_workspaces().get(index).cloned() else {
            return;
        };
        match workspace.state {
            StartWorkspaceState::Current => self.close_start_page(),
            StartWorkspaceState::Missing => {
                self.status.text = format!("E: missing workspace: {}", workspace.path.display());
                self.file.dirty = true;
            }
            StartWorkspaceState::Untrusted => {
                self.status.text = format!("E: untrusted workspace: {}", workspace.path.display());
                self.file.dirty = true;
            }
            StartWorkspaceState::Available => {
                self.request_workspace_switch(&workspace.path);
            }
        }
    }

    pub(crate) fn request_workspace_switch(&mut self, path: &Path) {
        #[cfg(not(unix))]
        {
            let _ = path;
            self.status.text = "E: workspace switching is unsupported on this platform".to_owned();
            self.file.dirty = true;
        }
        #[cfg(unix)]
        {
            if self.file.editor_mode || self.file.stdin_buffer {
                self.status.text =
                    "E: workspace switching is unavailable in editor or stdin mode".to_owned();
                self.file.dirty = true;
                return;
            }
            if self.any_modified() {
                self.status.text =
                    "E: save or discard modified buffers before switching workspace".to_owned();
                self.file.dirty = true;
                return;
            }
            let Ok(path) = path.canonicalize() else {
                self.status.text = format!("E: missing workspace: {}", path.display());
                self.file.dirty = true;
                return;
            };
            if !path.is_dir() {
                self.status.text = format!("E: workspace is not a directory: {}", path.display());
                self.file.dirty = true;
                return;
            }
            self.workspace.pending_switch = Some(path);
            self.close_start_page();
            self.file.dirty = true;
        }
    }
}

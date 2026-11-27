//! Owns the read-only page that temporarily replaces the active buffer.
//!
//! Help, Git diff/blame/conflict views, task output, and terminal output all
//! share this state so the buffer model keeps only real buffers.

use super::*;

pub(crate) const HELP_TEXT: &str = include_str!("help.txt");
pub(crate) const RECOVERY_TEXT: &str = include_str!("../../../docs/src/recovery.md");
pub(crate) const ONBOARDING_TEXT: &str = include_str!("../../../docs/src/onboarding.md");

#[derive(Default)]
pub(crate) struct ReadOnlyState {
    /// Buffer replaced by the read-only page.
    pub(crate) back: Option<View>,
    /// Short title for the current read-only page.
    pub(crate) title: Option<String>,
}

impl Model {
    pub(crate) fn show_help(&mut self) {
        self.show_read_only(HELP_TEXT, "Help: :q returns", "help");
    }

    pub(crate) fn show_recovery_help(&mut self) {
        self.show_read_only(RECOVERY_TEXT, "Recovery: :q returns", "recovery");
    }

    pub(crate) fn show_onboarding_help(&mut self) {
        self.show_read_only(ONBOARDING_TEXT, "Start: :q returns", "start");
    }

    pub(crate) fn show_read_only(&mut self, text: &str, status: &str, title: &str) -> bool {
        if self.pager.back.is_some() {
            self.status.text = "already viewing a read-only view".to_owned();
            self.file.dirty = true;
            return false;
        }
        self.sync_active();
        let view = self.active_view_state();
        self.session.editor = EditorState::from_text(text).into();
        self.session.path = None;
        self.session.cursor = 0;
        self.pager.back = Some(view);
        self.pager.title = Some(title.to_owned());
        self.clear_git_gutter();
        self.syntax_cache
            .highlights
            .borrow_mut()
            .switch_path(None, true);
        self.session.cursor = 0;
        self.view.scroll = 0;
        self.view.hscroll = 0;
        self.file.modified = false;
        self.start_page.welcome_back = self.start_page.welcome;
        self.start_page.welcome = false;
        self.ui.input_mode = InputMode::Normal;
        self.input.pending = None;
        self.status.text = status.to_owned();
        self.file.dirty = true;
        true
    }

    pub(crate) fn replace_read_only(&mut self, text: &str, status: &str, title: &str) {
        if self.pager.back.is_none() {
            self.show_read_only(text, status, title);
            return;
        }
        self.clear_git_gutter();
        self.session.editor = EditorState::from_text(text).into();
        self.syntax_cache
            .highlights
            .borrow_mut()
            .switch_path(None, true);
        self.session.cursor = 0;
        self.view.scroll = 0;
        self.view.hscroll = 0;
        self.file.modified = false;
        self.pager.title = Some(title.to_owned());
        self.status.text = status.to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn close_help(&mut self) -> bool {
        let Some(saved) = self.pager.back.take() else {
            return false;
        };
        let title = self.pager.title.take().unwrap_or_else(|| "help".to_owned());
        self.workspace.git.diff_view = None;
        self.workspace.git.diff_pending = None;
        self.workspace.git.apply_pending = None;
        self.workspace.git.blame_pending = None;
        self.workspace.git.conflicts_pending = None;
        self.workspace.git.stage_pending = None;
        self.workspace.git.resolve_pending = None;
        let Some(document) = self.buffers.document(saved.document).cloned() else {
            return false;
        };
        let syntax = document.syntax.clone();
        self.buffers.active_id = document.id;
        self.session.editor = document.session;
        self.session.path = document.path;
        self.session.cursor = saved.cursor;
        self.view.scroll = saved.scroll;
        self.view.hscroll = saved.hscroll;
        self.file.modified = document.modified;
        self.file.missing = document.missing;
        self.edit.visual_anchor = saved.visual_anchor;
        self.edit.visual_line = saved.visual_line;
        self.search.query = saved.search_query;
        self.syntax_cache.highlights.borrow_mut().switch_path_with(
            self.session.path.as_deref(),
            false,
            Some(syntax),
        );
        self.ui.input_mode = InputMode::Normal;
        self.start_page.welcome = std::mem::take(&mut self.start_page.welcome_back);
        self.input.pending = None;
        self.status.text = format!("Returned from {title}");
        self.file.dirty = true;
        true
    }
}

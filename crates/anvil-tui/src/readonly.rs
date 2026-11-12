//! Owns the read-only page that temporarily replaces the active buffer.
//!
//! Help, Git diff/blame/conflict views, task output, and terminal output all
//! share this state so the buffer model keeps only real buffers.

use super::*;

pub(crate) const HELP_TEXT: &str = include_str!("help.txt");

#[derive(Default)]
pub(crate) struct ReadOnlyState {
    /// Buffer replaced by the read-only page.
    pub(crate) back: Option<Buffer>,
    /// Short title for the current read-only page.
    pub(crate) title: Option<String>,
}

impl Model {
    pub(crate) fn show_help(&mut self) {
        self.show_read_only(HELP_TEXT, "Help: :q returns", "help");
    }

    pub(crate) fn show_read_only(&mut self, text: &str, status: &str, title: &str) -> bool {
        if self.pager.back.is_some() {
            self.status.text = "already viewing a read-only view".to_owned();
            self.file.dirty = true;
            return false;
        }
        let editor = std::mem::replace(
            &mut self.session.editor,
            EditorState::from_text(text).into(),
        );
        let saved = Buffer {
            editor,
            path: self.session.path.take(),
            cursor: self.session.cursor,
            scroll: self.view.scroll,
            hscroll: self.view.hscroll,
            visual_anchor: self.edit.visual_anchor,
            visual_line: self.edit.visual_line,
            modified: self.file.modified,
            missing: self.file.missing,
            search_query: std::mem::take(&mut self.search.query),
            syntax: self.syntax_cache.highlights.borrow().active_cache(),
        };
        self.pager.title = Some(title.to_owned());
        self.pager.back = Some(saved);
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
        self.git.diff_view = None;
        self.git.diff_pending = None;
        self.git.apply_pending = None;
        self.git.blame_pending = None;
        self.git.conflicts_pending = None;
        self.git.stage_pending = None;
        self.git.resolve_pending = None;
        let syntax = saved.syntax.clone();
        self.session.editor = saved.editor;
        self.session.path = saved.path;
        self.session.cursor = saved.cursor;
        self.view.scroll = saved.scroll;
        self.view.hscroll = saved.hscroll;
        self.file.modified = saved.modified;
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

//! Model-side incremental search and replace transitions.

use super::*;
use crate::search::{MAX_SEARCH_MATCHES, find_matches_limited};

impl Model {
    pub(crate) fn begin_search(&mut self) {
        self.search
            .begin(self.cursor, self.editor.selection_bytes());
        self.search_query.clear();
        self.ui.input_mode = InputMode::Search {
            query: String::new(),
        };
        self.dirty = true;
    }

    pub(crate) fn schedule_search(&mut self) {
        self.search.schedule();
        self.dirty = true;
    }

    pub(crate) fn tick_search(&mut self) {
        if !self.search.take_due() {
            return;
        }
        let query = match &self.ui.input_mode {
            InputMode::Search { query } => query.clone(),
            _ => self.search_query.clone(),
        };
        self.apply_search_query(&query, false);
    }

    pub(crate) fn commit_search(&mut self, query: String) {
        if self.apply_search_query(&query, true) {
            self.search_query = query;
            self.ui.input_mode = InputMode::Normal;
        }
        self.dirty = true;
    }

    pub(crate) fn cancel_search(&mut self) {
        if let Some((cursor, selections)) = self.search.restore_origin() {
            self.cursor = cursor;
            self.editor.set_selections_bytes(selections);
        }
        self.search_query.clear();
        self.ui.input_mode = InputMode::Normal;
        self.dirty = true;
    }

    fn apply_search_query(&mut self, query: &str, committed: bool) -> bool {
        self.search.due = None;
        if query.is_empty() {
            self.search.matches.clear();
            self.search.current = 0;
            self.search.error = None;
            self.search.truncated = false;
            return true;
        }
        let text = self.editor.text().slice(0..self.editor.text().len_bytes());
        match find_matches_limited(&text, query, self.search.options, MAX_SEARCH_MATCHES) {
            Ok((matches, truncated)) => {
                let cursor = self.cursor;
                let current = matches
                    .iter()
                    .position(|range| range.start <= cursor && cursor < range.end)
                    .or_else(|| matches.iter().position(|range| range.start >= cursor))
                    .unwrap_or(0);
                if let Some(range) = matches.get(current) {
                    self.cursor = range.start;
                }
                let selections = matches
                    .iter()
                    .map(|range| (range.start, range.end))
                    .collect();
                self.editor.set_selections_bytes(selections);
                self.search.matches = matches;
                self.search.current = current;
                self.search.error = None;
                self.search.truncated = truncated;
                if committed {
                    self.status = self.search_status(query);
                }
                self.dirty = true;
                true
            }
            Err(error) => {
                self.search.error = Some(error.clone());
                self.status = format!("E: {error}");
                self.dirty = true;
                false
            }
        }
    }

    pub(crate) fn search_next(&mut self, forward: bool) {
        if self.search_query.is_empty() {
            self.status = "E: no previous search".to_owned();
            self.dirty = true;
            return;
        }
        if !self.apply_search_query(&self.search_query.clone(), false) {
            return;
        }
        if self.search.matches.is_empty() {
            self.status = "E: pattern not found".to_owned();
            self.dirty = true;
            return;
        }
        let len = self.search.matches.len();
        self.search.current = if forward {
            (self.search.current + 1) % len
        } else {
            (self.search.current + len - 1) % len
        };
        self.cursor = self.search.matches[self.search.current].start;
        self.status = self.search_status(&self.search_query);
        self.dirty = true;
    }

    pub(crate) fn search_status(&self, query: &str) -> String {
        let mut flags = Vec::new();
        if self.search.options.regex {
            flags.push("rx");
        }
        if self.search.options.whole_word {
            flags.push("ww");
        }
        if self.search.options.case_sensitive {
            flags.push("cs");
        }
        let flags = if flags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", flags.join(" "))
        };
        if let Some(error) = &self.search.error {
            return format!("/{query}{flags} error: {error}");
        }
        let suffix = if self.search.truncated { "+" } else { "" };
        if self.search.matches.is_empty() {
            format!("/{query}{flags} [0/0]")
        } else {
            format!(
                "/{query}{flags} [{}/{}{suffix}]",
                self.search.current + 1,
                self.search.matches.len()
            )
        }
    }

    pub(crate) fn replace_current(&mut self, replacement: &str) -> bool {
        if self.reject_read_only() {
            return false;
        }
        let query = self.replace_query();
        if query.is_empty() || !self.apply_search_query(&query, false) {
            self.status = "E: no active search".to_owned();
            self.dirty = true;
            return false;
        }
        let Some(range) = self.search.selected().cloned() else {
            self.status = "E: pattern not found".to_owned();
            self.dirty = true;
            return false;
        };
        if !self
            .editor
            .apply_transaction(vec![(range, replacement.to_owned())])
        {
            self.status = "E: replacement failed".to_owned();
            self.dirty = true;
            return false;
        }
        self.modified = true;
        self.apply_search_query(&query, false);
        self.status = "replacement applied".to_owned();
        self.dirty = true;
        true
    }

    pub(crate) fn replace_all(&mut self, replacement: &str) -> bool {
        if self.reject_read_only() {
            return false;
        }
        let query = self.replace_query();
        if query.is_empty() || !self.apply_search_query(&query, false) {
            self.status = "E: no active search".to_owned();
            self.dirty = true;
            return false;
        }
        if self.search.matches.is_empty() {
            self.status = "E: pattern not found".to_owned();
            self.dirty = true;
            return false;
        }
        let count = self.search.matches.len();
        let edits = self
            .search
            .matches
            .iter()
            .cloned()
            .map(|range| (range, replacement.to_owned()))
            .collect();
        if !self.editor.apply_transaction(edits) {
            self.status = "E: replacement failed".to_owned();
            self.dirty = true;
            return false;
        }
        self.modified = true;
        self.apply_search_query(&query, false);
        self.status = format!("{count} replacements applied");
        self.dirty = true;
        true
    }

    fn replace_query(&self) -> String {
        match &self.ui.input_mode {
            InputMode::Search { query } => query.clone(),
            _ => self.search_query.clone(),
        }
    }

    pub(crate) fn toggle_search_regex(&mut self) {
        self.search.options.regex = !self.search.options.regex;
        self.schedule_search();
    }

    pub(crate) fn toggle_search_whole_word(&mut self) {
        self.search.options.whole_word = !self.search.options.whole_word;
        self.schedule_search();
    }

    pub(crate) fn toggle_search_case(&mut self) {
        self.search.options.case_sensitive = !self.search.options.case_sensitive;
        self.schedule_search();
    }
}

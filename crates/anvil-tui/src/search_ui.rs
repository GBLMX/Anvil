//! Model-side incremental search and replace transitions.

use super::*;
use crate::file_worker::{FileRequest, MAX_PROJECT_RESULTS};
use crate::search::{MAX_SEARCH_MATCHES, find_matches_limited};

impl Model {
    pub(crate) fn begin_search(&mut self) {
        self.search
            .state
            .begin(self.session.cursor, self.session.editor.selection_bytes());
        self.search.query.clear();
        self.ui.input_mode = InputMode::Search {
            query: InputBuffer::default(),
        };
        self.file.dirty = true;
    }

    pub(crate) fn schedule_search(&mut self) {
        self.search.state.schedule();
        self.file.dirty = true;
    }

    pub(crate) fn tick_search(&mut self) {
        if !self.search.state.take_due() {
            return;
        }
        let query = match &self.ui.input_mode {
            InputMode::Search { query } => query.text().to_owned(),
            _ => self.search.query.clone(),
        };
        self.apply_search_query(&query, false);
    }

    pub(crate) fn commit_search(&mut self, query: String) {
        if self.apply_search_query(&query, true) {
            self.search.query = query;
            self.ui.input_mode = InputMode::Normal;
        }
        self.file.dirty = true;
    }

    pub(crate) fn cancel_search(&mut self) {
        if let Some((cursor, selections)) = self.search.state.restore_origin() {
            self.session.cursor = cursor;
            self.session.editor.set_selections_bytes(selections);
        }
        self.search.query.clear();
        self.ui.input_mode = InputMode::Normal;
        self.file.dirty = true;
    }

    fn apply_search_query(&mut self, query: &str, committed: bool) -> bool {
        self.search.state.due = None;
        if query.is_empty() {
            self.search.state.matches.clear();
            self.search.state.current = 0;
            self.search.state.error = None;
            self.search.state.truncated = false;
            return true;
        }
        let text = self
            .session
            .editor
            .text()
            .slice(0..self.session.editor.text().len_bytes());
        match find_matches_limited(&text, query, self.search.state.options, MAX_SEARCH_MATCHES) {
            Ok((matches, truncated)) => {
                let cursor = self.session.cursor;
                let current = matches
                    .iter()
                    .position(|range| range.start <= cursor && cursor < range.end)
                    .or_else(|| matches.iter().position(|range| range.start >= cursor))
                    .unwrap_or(0);
                if let Some(range) = matches.get(current) {
                    self.session.cursor = range.start;
                }
                let selections = matches
                    .iter()
                    .map(|range| (range.start, range.end))
                    .collect();
                self.session.editor.set_selections_bytes(selections);
                self.search.state.matches = matches;
                self.search.state.current = current;
                self.search.state.error = None;
                self.search.state.truncated = truncated;
                if committed {
                    self.status.text = self.search_status(query);
                }
                self.file.dirty = true;
                true
            }
            Err(error) => {
                self.search.state.error = Some(error.clone());
                self.status.text = format!("E: {error}");
                self.file.dirty = true;
                false
            }
        }
    }

    pub(crate) fn search_next(&mut self, forward: bool) {
        if self.search.query.is_empty() {
            self.status.text = "E: no previous search".to_owned();
            self.file.dirty = true;
            return;
        }
        if !self.apply_search_query(&self.search.query.clone(), false) {
            return;
        }
        if self.search.state.matches.is_empty() {
            self.status.text = "E: pattern not found".to_owned();
            self.file.dirty = true;
            return;
        }
        self.record_jump();
        let len = self.search.state.matches.len();
        self.search.state.current = if forward {
            (self.search.state.current + 1) % len
        } else {
            (self.search.state.current + len - 1) % len
        };
        self.session.cursor = self.search.state.matches[self.search.state.current].start;
        self.status.text = self.search_status(&self.search.query);
        self.file.dirty = true;
    }

    pub(crate) fn search_status(&self, query: &str) -> String {
        let mut flags = Vec::new();
        if self.search.state.options.regex {
            flags.push("rx");
        }
        if self.search.state.options.whole_word {
            flags.push("ww");
        }
        if self.search.state.options.case_sensitive {
            flags.push("cs");
        }
        let flags = if flags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", flags.join(" "))
        };
        if let Some(error) = &self.search.state.error {
            return format!("/{query}{flags} error: {error}");
        }
        let suffix = if self.search.state.truncated { "+" } else { "" };
        if self.search.state.matches.is_empty() {
            format!("/{query}{flags} [0/0]")
        } else {
            format!(
                "/{query}{flags} [{}/{}{suffix}]",
                self.search.state.current + 1,
                self.search.state.matches.len()
            )
        }
    }

    pub(crate) fn replace_current(&mut self, replacement: &str) -> bool {
        if self.reject_read_only() {
            return false;
        }
        let query = self.replace_query();
        if query.is_empty() || !self.apply_search_query(&query, false) {
            self.status.text = "E: no active search".to_owned();
            self.file.dirty = true;
            return false;
        }
        let Some(range) = self.search.state.selected().cloned() else {
            self.status.text = "E: pattern not found".to_owned();
            self.file.dirty = true;
            return false;
        };
        if !self
            .session
            .editor
            .apply_transaction(vec![(range, replacement.to_owned())])
        {
            self.status.text = "E: replacement failed".to_owned();
            self.file.dirty = true;
            return false;
        }
        self.file.modified = true;
        self.apply_search_query(&query, false);
        self.status.text = "replacement applied".to_owned();
        self.file.dirty = true;
        true
    }

    pub(crate) fn replace_all(&mut self, replacement: &str) -> bool {
        if self.reject_read_only() {
            return false;
        }
        let query = self.replace_query();
        if query.is_empty() || !self.apply_search_query(&query, false) {
            self.status.text = "E: no active search".to_owned();
            self.file.dirty = true;
            return false;
        }
        if self.search.state.matches.is_empty() {
            self.status.text = "E: pattern not found".to_owned();
            self.file.dirty = true;
            return false;
        }
        let count = self.search.state.matches.len();
        let edits = self
            .search
            .state
            .matches
            .iter()
            .cloned()
            .map(|range| (range, replacement.to_owned()))
            .collect();
        if !self.session.editor.apply_transaction(edits) {
            self.status.text = "E: replacement failed".to_owned();
            self.file.dirty = true;
            return false;
        }
        self.file.modified = true;
        self.apply_search_query(&query, false);
        self.status.text = format!("{count} replacements applied");
        self.file.dirty = true;
        true
    }

    fn replace_query(&self) -> String {
        match &self.ui.input_mode {
            InputMode::Search { query } => query.text().to_owned(),
            _ => self.search.query.clone(),
        }
    }

    pub(crate) fn toggle_search_regex(&mut self) {
        self.search.state.options.regex = !self.search.state.options.regex;
        self.schedule_search();
    }

    pub(crate) fn toggle_search_whole_word(&mut self) {
        self.search.state.options.whole_word = !self.search.state.options.whole_word;
        self.schedule_search();
    }

    pub(crate) fn toggle_search_case(&mut self) {
        self.search.state.options.case_sensitive = !self.search.state.options.case_sensitive;
        self.schedule_search();
    }

    pub(crate) fn request_project_search(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            self.status.text = "E: project search requires a query".to_owned();
            self.file.dirty = true;
            return;
        }
        if !self.ensure_file_worker() {
            self.status.text = "E: gateway unavailable".to_owned();
            self.file.dirty = true;
            return;
        }
        if self.workspace.file_worker.is_none() {
            self.status.text = "E: file worker unavailable".to_owned();
            self.file.dirty = true;
            return;
        }
        self.project.search_generation = self.project.search_generation.wrapping_add(1);
        let generation = self.project.search_generation;
        let mut expected = 0usize;
        let mut error = None;
        if let Some(worker) = self.workspace.file_worker.as_ref() {
            if let Err(request_error) = worker.request(FileRequest::ProjectSearch {
                query: query.to_owned(),
                generation,
            }) {
                error = Some(request_error);
            } else {
                expected += 1;
            }
        }
        for worker in self.workspace.root_file_workers.values() {
            match worker.request(FileRequest::ProjectSearch {
                query: query.to_owned(),
                generation,
            }) {
                Ok(()) => expected += 1,
                Err(request_error) => error = error.or(Some(request_error)),
            }
        }
        if expected == 0 {
            self.status.text = format!(
                "E: project search: {}",
                error.unwrap_or_else(|| "no workers".to_owned())
            );
            self.file.dirty = true;
            return;
        }
        self.project.search_pending = Some(generation);
        self.project.search_expected = expected;
        self.project.search_received = 0;
        self.project.search_results.clear();
        self.project.search_truncated = false;
        self.project.search_error = error;
        self.status.text = format!("searching project: {query} ({expected} roots)");
        self.file.dirty = true;
    }

    pub(crate) fn handle_project_search(
        &mut self,
        root: Option<PathBuf>,
        query: String,
        generation: u64,
        result: Result<(Vec<crate::pipe_locations::PipeLocation>, bool), String>,
    ) {
        if self.project.search_pending != Some(generation) {
            return;
        }
        self.project.search_received = self.project.search_received.saturating_add(1);
        match result {
            Ok((mut locations, truncated)) => {
                self.project.search_truncated |= truncated;
                if let Some(root) = root {
                    for location in &mut locations {
                        if !Path::new(&location.path).is_absolute() {
                            location.path =
                                root.join(&location.path).to_string_lossy().into_owned();
                        }
                    }
                }
                self.project.search_results.extend(locations);
            }
            Err(error) => {
                self.project.search_error.get_or_insert(error);
            }
        }
        if self.project.search_received < self.project.search_expected {
            self.status.text = format!(
                "searching project: {query} ({}/{})",
                self.project.search_received, self.project.search_expected
            );
            self.file.dirty = true;
            return;
        }
        self.project.search_pending = None;
        let mut seen = std::collections::HashSet::new();
        self.project.search_results.retain(|location| {
            seen.insert((
                location.path.clone(),
                location.line,
                location.column,
                location.message.clone(),
            ))
        });
        self.project.search_results.sort_by(|left, right| {
            (&left.path, left.line, left.column).cmp(&(&right.path, right.line, right.column))
        });
        if self.project.search_results.len() > MAX_PROJECT_RESULTS {
            self.project.search_results.truncate(MAX_PROJECT_RESULTS);
            self.project.search_truncated = true;
        }
        let count = self.project.search_results.len();
        let mut locations = std::mem::take(&mut self.project.search_results);
        self.set_project_search_results(&query, std::mem::take(&mut locations));
        let suffix = if self.project.search_truncated {
            "+"
        } else {
            ""
        };
        self.status.text = match self.project.search_error.take() {
            Some(error) => {
                if count == 0 {
                    format!("E: project search: {error}")
                } else {
                    format!("search: {count}{suffix} matches; error: {error}")
                }
            }
            None if count == 0 => format!("search: no matches for {query}"),
            None => format!("search: {count}{suffix} matches for {query}"),
        };
        self.file.dirty = true;
    }

    pub(crate) fn cancel_project_search(&mut self) {
        self.project.search_pending = None;
        self.project.search_expected = 0;
        self.project.search_received = 0;
        self.project.search_results.clear();
        self.project.search_truncated = false;
        self.project.search_error = None;
        self.overlays.location.stdin_locations.clear();
        self.overlays.location.stdin_location_selected = None;
        self.overlays.location.stdin_location_scroll = 0;
        self.overlays.location.location_title = "Locations".to_owned();
        self.status.text = "search cancelled".to_owned();
        self.file.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipe_locations::PipeLocation;
    use anvil_editor::EditorState;

    #[test]
    fn project_search_requires_a_query() {
        let mut model = Model::new(EditorState::from_text("x"), None);
        model.execute_command("search-project");
        assert_eq!(model.status.text, "E: project search requires a query");
    }

    #[test]
    fn project_search_results_drive_the_location_panel() {
        let mut model = Model::new(EditorState::from_text("x"), None);
        model.set_project_search_results(
            "needle",
            vec![PipeLocation {
                path: "src/main.rs".to_owned(),
                line: 4,
                column: 2,
                message: "needle".to_owned(),
            }],
        );
        assert_eq!(model.overlays.location.location_title, "Search: needle");
        assert_eq!(model.overlays.location.stdin_locations.len(), 1);
        model.cancel_project_search();
        assert!(model.overlays.location.stdin_locations.is_empty());
        assert_eq!(model.overlays.location.location_title, "Locations");
    }

    fn location(path: &str) -> PipeLocation {
        PipeLocation {
            path: path.to_owned(),
            line: 1,
            column: 1,
            message: "needle".to_owned(),
        }
    }

    #[test]
    fn project_search_waits_for_all_roots_and_merges_absolute_paths() {
        let mut model = Model::new(EditorState::from_text("x"), None);
        let root = PathBuf::from("/tmp/second-root");
        model.project.search_generation = 7;
        model.project.search_pending = Some(7);
        model.project.search_expected = 2;
        model.project.search_received = 0;
        model.project.search_results.clear();
        model.project.search_truncated = false;
        model.project.search_error = None;

        model.handle_project_search(
            Some(root.clone()),
            "needle".to_owned(),
            7,
            Ok((vec![location("src/lib.rs")], false)),
        );

        assert_eq!(model.project.search_pending, Some(7));
        assert!(model.overlays.location.stdin_locations.is_empty());

        model.handle_project_search(
            None,
            "needle".to_owned(),
            7,
            Ok((vec![location("src/main.rs")], false)),
        );

        assert_eq!(model.project.search_pending, None);
        assert_eq!(model.overlays.location.stdin_locations.len(), 2);
        assert!(
            model
                .overlays
                .location
                .stdin_locations
                .iter()
                .any(|entry| entry.path == "src/main.rs")
        );
        assert!(
            model
                .overlays
                .location
                .stdin_locations
                .iter()
                .any(|entry| { entry.path == root.join("src/lib.rs").to_string_lossy() })
        );
        assert!(model.status.text.contains("2 matches"));
    }

    #[test]
    fn stale_project_search_results_are_ignored() {
        let mut model = Model::new(EditorState::from_text("x"), None);
        model.project.search_pending = Some(7);
        model.project.search_expected = 2;
        model.project.search_received = 0;

        model.handle_project_search(
            None,
            "needle".to_owned(),
            6,
            Ok((vec![location("src/main.rs")], false)),
        );

        assert_eq!(model.project.search_received, 0);
        assert!(model.project.search_results.is_empty());
        assert_eq!(model.project.search_pending, Some(7));
    }

    #[test]
    fn project_search_caps_merged_results_globally() {
        let mut model = Model::new(EditorState::from_text("x"), None);
        model.project.search_pending = Some(1);
        model.project.search_expected = 1;
        model.project.search_received = 0;
        let locations = (0..MAX_PROJECT_RESULTS + 1)
            .map(|index| PipeLocation {
                path: format!("src/{index}.rs"),
                line: 1,
                column: 1,
                message: "needle".to_owned(),
            })
            .collect();

        model.handle_project_search(None, "needle".to_owned(), 1, Ok((locations, false)));

        assert_eq!(
            model.overlays.location.stdin_locations.len(),
            MAX_PROJECT_RESULTS
        );
        assert!(model.status.text.contains('+'));
    }

    #[test]
    fn project_search_cancel_clears_pending_root_state() {
        let mut model = Model::new(EditorState::from_text("x"), None);
        model.project.search_pending = Some(7);
        model.project.search_expected = 2;
        model.project.search_received = 1;
        model.project.search_results.push(location("src/main.rs"));

        model.cancel_project_search();

        assert_eq!(model.project.search_pending, None);
        assert_eq!(model.project.search_expected, 0);
        assert_eq!(model.project.search_received, 0);
        assert!(model.project.search_results.is_empty());
    }
}

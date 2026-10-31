//! Search, substitution, and Ex command dispatch.

use super::*;

impl Model {
    pub(crate) fn execute_search(&mut self, query: String) {
        if query.is_empty() {
            if self.search_query.is_empty() {
                self.status = "E: no previous search".to_owned();
                self.dirty = true;
            } else {
                self.search_next(true);
            }
            return;
        }
        self.search_query = query;
        let matches = Self::find_matches(self.editor.text(), &self.search_query);
        if matches.is_empty() {
            self.status = format!("E: pattern not found: {}", self.search_query);
            self.dirty = true;
            return;
        }
        let target = matches
            .iter()
            .copied()
            .find(|&offset| offset >= self.cursor)
            .unwrap_or(matches[0]);
        self.jump_to(target);
        self.status = format!("/{}", self.search_query);
    }

    pub(crate) fn search_next(&mut self, forward: bool) {
        if self.search_query.is_empty() {
            self.status = "E: no previous search".to_owned();
            self.dirty = true;
            return;
        }
        let matches = Self::find_matches(self.editor.text(), &self.search_query);
        if matches.is_empty() {
            self.status = format!("E: pattern not found: {}", self.search_query);
            self.dirty = true;
            return;
        }
        let target = if forward {
            matches
                .iter()
                .copied()
                .find(|&offset| offset > self.cursor)
                .unwrap_or(matches[0])
        } else {
            matches
                .iter()
                .rev()
                .copied()
                .find(|&offset| offset < self.cursor)
                .unwrap_or(matches[matches.len() - 1])
        };
        self.jump_to(target);
        self.status = format!("/{}", self.search_query);
    }

    fn substitute(&mut self, whole: bool, spec: &str) {
        if self.reject_read_only() {
            return;
        }
        let mut parts = spec.splitn(3, '/');
        let (Some(old), Some(new)) = (parts.next(), parts.next()) else {
            self.status = "E: usage: :s/old/new/[g]".to_owned();
            self.dirty = true;
            return;
        };
        let flags = parts.next().unwrap_or_default();
        if old.is_empty() {
            self.status = "E: empty search pattern".to_owned();
            self.dirty = true;
            return;
        }
        if !flags.is_empty() && flags != "g" {
            self.status = format!("E: unknown substitute flags: {flags}");
            self.dirty = true;
            return;
        }
        let range = if whole {
            0..self.editor.text().len_bytes()
        } else {
            let Some(line) = self.editor.index().offset_to_line(self.cursor) else {
                self.status = "E: no line".to_owned();
                self.dirty = true;
                return;
            };
            line.start..line.end
        };
        let matches = Self::find_matches_in(self.editor.text(), old, range);
        let matches = if flags.contains('g') {
            let mut non_overlapping = Vec::new();
            let mut end = 0;
            for start in matches {
                if start >= end {
                    non_overlapping.push(start);
                    end = start + old.len();
                }
            }
            non_overlapping
        } else {
            matches.into_iter().take(1).collect()
        };
        if matches.is_empty() {
            self.status = "E: pattern not found".to_owned();
            self.dirty = true;
            return;
        }
        let first = matches[0];
        for start in matches.into_iter().rev() {
            let end = start + old.len();
            self.editor.set_selection_bytes(start, end);
            self.editor.delete(start..end);
            if !new.is_empty() {
                self.editor.insert(start, new);
            }
        }
        self.cursor = first.min(self.editor.text().len_bytes());
        self.editor.set_selection_bytes(self.cursor, self.cursor);
        self.modified = true;
        self.dirty = true;
        self.status = "substitution applied".to_owned();
    }

    pub(crate) fn execute_command(&mut self, command: &str) {
        if let Some(rest) = command.strip_prefix("%s/") {
            self.substitute(true, rest);
            return;
        }
        if let Some(rest) = command.strip_prefix("s/") {
            self.substitute(false, rest);
            return;
        }
        let mut parts = command.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or_default();
        let argument = parts.next().unwrap_or_default().trim();
        match (name, argument) {
            ("w", "") => {
                self.save();
            }
            ("w", path) => {
                self.save_as(path);
            }
            ("wa", "") => {
                self.save_all();
            }
            ("e", path) => self.open_file(path),
            ("bn", "") => self.switch_buffer(1),
            ("bp", "") => self.switch_buffer(-1),
            ("q", "") => self.request_quit(),
            ("q!", "") => self.force_quit(),
            ("help", "") => self.show_help(),
            ("reconnect", "") => self.reconnect_gateway(),
            ("lsp-start", "") => self.start_lsp(),
            ("tree", "") => self.open_tree(),
            ("lsp-stop", "") => self.stop_lsp(),
            ("format", "") => self.request_formatting(),
            ("symbols", "") => self.request_document_symbols(),
            ("plugin-start", "") => self.start_host(),
            ("plugin-oom", "") => self.start_oom_host(),
            ("plugin-exec", "") => self.exec_host(),
            ("plugin-stop", "") => self.stop_host(),
            ("wq", "") => {
                if self.save() {
                    if self.any_modified() {
                        self.status = "E: other buffers have unsaved changes".to_owned();
                        self.dirty = true;
                    } else {
                        self.quit = true;
                    }
                }
            }
            ("wq", path) => {
                if self.save_as(path) {
                    if self.any_modified() {
                        self.status = "E: other buffers have unsaved changes".to_owned();
                        self.dirty = true;
                    } else {
                        self.quit = true;
                    }
                }
            }
            _ => {
                self.status = format!("E: unknown command: {command}");
                self.dirty = true;
            }
        }
    }

    fn find_matches(text: &TextDocument, needle: &str) -> Vec<usize> {
        Self::find_matches_in(text, needle, 0..text.len_bytes())
    }

    fn find_matches_in(
        text: &TextDocument,
        needle: &str,
        range: std::ops::Range<usize>,
    ) -> Vec<usize> {
        let needle = needle.as_bytes();
        if needle.is_empty() || needle.len() > range.end.saturating_sub(range.start) {
            return Vec::new();
        }
        let mut matches = Vec::new();
        let mut carry = Vec::new();
        let mut processed = range.start;
        for chunk in text.chunks(range) {
            if chunk.is_empty() {
                continue;
            }
            let mut window = Vec::with_capacity(carry.len() + chunk.len());
            window.extend_from_slice(&carry);
            window.extend_from_slice(chunk.as_bytes());
            let window_start = processed.saturating_sub(carry.len());
            let mut from = 0;
            while from + needle.len() <= window.len() {
                let Some(local) = window[from..]
                    .windows(needle.len())
                    .position(|candidate| candidate == needle)
                else {
                    break;
                };
                let at = from + local;
                matches.push(window_start + at);
                from = at + 1;
            }
            let keep = needle.len().saturating_sub(1).min(window.len());
            carry.clear();
            carry.extend_from_slice(&window[window.len() - keep..]);
            processed += chunk.len();
        }
        matches
    }
}

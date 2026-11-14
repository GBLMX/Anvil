use super::*;

impl Model {
    pub(crate) fn cancel_completion(&mut self) {
        self.language.lsp_requests.pending.retain(|_, kind| {
            !matches!(
                kind,
                LspRequestKind::Completion | LspRequestKind::CompletionResolve
            )
        });
        self.overlays.lsp_results.completion = None;
        self.overlays.lsp_results.completion_resolve = None;
        self.close_overlay(OverlayKind::Completion);
    }
    pub(crate) fn auto_completion(&mut self) {
        if self.active_lsp_worker().is_some() {
            self.request_completion();
        }
    }
    pub(crate) fn request_completion(&mut self) {
        self.cancel_completion();
        self.close_hover();
        self.close_diagnostic_peek();
        self.cancel_code_actions();
        let local = self.local_completion_items();
        let had_local = !local.is_empty();
        if had_local {
            self.install_completion(local);
            self.status.text = "completion".to_owned();
        }
        let Some(uri) = self
            .session
            .path
            .as_ref()
            .map(|path| anvil_lsp_client::file_uri(path))
        else {
            self.file.dirty = true;
            return;
        };
        let Some((line, character)) = self.lsp_position() else {
            return;
        };
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character},
            "context": {"triggerKind": 1}
        });
        if self.send_lsp_request(
            LspRequestKind::Completion,
            "textDocument/completion",
            params,
        ) {
            self.status.text = "completion".to_owned();
        }
        self.file.dirty = true;
    }
    pub(crate) fn request_completion_resolve(&mut self) {
        let Some(completion) = self.overlays.lsp_results.completion.as_ref() else {
            return;
        };
        let Some(index) = completion.filtered.get(completion.selected).copied() else {
            return;
        };
        let Some(item) = completion.items.get(index) else {
            return;
        };
        if item.source.as_str() != "builtin.lsp" {
            return;
        }
        if item.resolved
            || self
                .overlays
                .lsp_results
                .completion_resolve
                .is_some_and(|(_, pending)| pending == index)
        {
            return;
        }
        let raw = item.raw.clone();
        let id = self.language.lsp_requests.next_request;
        if self.send_lsp_request(
            LspRequestKind::CompletionResolve,
            "completionItem/resolve",
            raw,
        ) {
            self.overlays.lsp_results.completion_resolve = Some((id, index));
        }
    }
    pub(crate) fn apply_completion(&mut self) {
        let Some(completion) = self.overlays.lsp_results.completion.take() else {
            return;
        };
        self.close_overlay(OverlayKind::Completion);
        let Some(item) = completion
            .filtered
            .get(completion.selected)
            .and_then(|index| completion.items.get(*index))
        else {
            return;
        };
        let label = item.label.clone();
        let additional = crate::completion_sources::completion_additional_edits(&item.raw);
        let insert = item.insert.clone();
        let replace = item.replace;
        let replace_bytes = item.replace_bytes;
        let mut snippet = item.snippet.clone();
        self.overlays.lsp_results.snippet = None;
        if let Some((start, end)) = replace_bytes {
            let end = end.max(start);
            if !self.apply_completion_transaction(start, end, &insert) {
                return;
            }
            self.session.cursor = start + insert.len();
            self.file.modified = true;
            if let Some(snippet) = snippet.take() {
                self.start_snippet(&snippet, start);
            }
        } else if let Some((start_line, start_char, end_line, end_char)) = replace {
            let (Some(start), Some(end)) = (
                self.try_offset_at_utf16(start_line, start_char),
                self.try_offset_at_utf16(end_line, end_char),
            ) else {
                self.status.text = "E: invalid completion edit range".to_owned();
                return;
            };
            if start > end {
                self.status.text = "E: invalid completion edit range".to_owned();
                return;
            }
            if !self.apply_completion_transaction(start, end, &insert) {
                return;
            }
            self.session.cursor = start + insert.len();
            self.file.modified = true;
            if let Some(snippet) = snippet.take() {
                self.start_snippet(&snippet, start);
            }
        } else {
            let selections = self.session.editor.selection_bytes();
            if selections.len() != 1 {
                snippet = None;
            }
            let start = selections
                .first()
                .map(|(anchor, head)| (*anchor).min(*head))
                .unwrap_or(self.session.cursor);
            self.insert_text(&insert);
            if let Some(snippet) = snippet {
                self.start_snippet(&snippet, start);
            }
        }
        if !self.apply_additional_completion_edits(&additional) {
            return;
        }
        self.status.text = format!("completed {label}");
        self.file.dirty = true;
    }

    fn apply_completion_transaction(&mut self, start: usize, end: usize, text: &str) -> bool {
        let revision = self.session.editor.revision();
        match self
            .session
            .editor
            .apply_transaction_at(revision, vec![(start..end, text.to_owned())])
        {
            Ok(_) => true,
            Err(error) => {
                self.status.text = format!("E: completion edit: {error}");
                false
            }
        }
    }

    fn apply_additional_completion_edits(
        &mut self,
        edits: &[crate::completion_sources::CompletionAdditionalEdit],
    ) -> bool {
        let mut ranges = Vec::with_capacity(edits.len());
        for edit in edits {
            let (Some(start), Some(end)) = (
                self.try_offset_at_utf16(edit.start_line, edit.start_character),
                self.try_offset_at_utf16(edit.end_line, edit.end_character),
            ) else {
                self.status.text = "E: invalid completion edit range".to_owned();
                return false;
            };
            if start > end {
                self.status.text = "E: invalid completion edit range".to_owned();
                return false;
            }
            ranges.push((start, end, edit.new_text.clone()));
        }
        ranges.sort_by_key(|(start, end, _)| (*start, *end));
        for pair in ranges.windows(2) {
            if pair[0].1 > pair[1].0 {
                self.status.text = "E: overlapping completion edits".to_owned();
                return false;
            }
        }
        for (start, end, text) in ranges.into_iter().rev() {
            let deleted = end.saturating_sub(start);
            let delta = text.len() as isize - deleted as isize;
            let cursor = self.session.cursor;
            if !self.apply_completion_transaction(start, end, &text) {
                return false;
            }
            if let Some(snippet) = self.overlays.lsp_results.snippet.as_mut() {
                snippet.adjust_delete(start, end);
                snippet.adjust_insert(start, text.len());
            }
            if start <= cursor {
                self.session.cursor = if end <= cursor {
                    (cursor as isize + delta).max(0) as usize
                } else {
                    start + text.len()
                };
            }
        }
        true
    }
    fn start_snippet(&mut self, snippet: &Snippet, base: usize) {
        self.overlays.lsp_results.snippet = SnippetSession::new(snippet.offset_stops(base));
        self.goto_snippet_stop();
    }
    fn goto_snippet_stop(&mut self) {
        let ranges = self
            .overlays
            .lsp_results
            .snippet
            .as_ref()
            .map(SnippetSession::mirror_ranges)
            .unwrap_or_default();
        let Some((start, _)) = ranges.first().copied() else {
            self.overlays.lsp_results.snippet = None;
            return;
        };
        let mirrors = ranges.len();
        self.session.editor.set_selections_bytes(ranges);
        self.session.cursor = start;
        if mirrors > 1 {
            self.status.text = format!("snippet stop ({mirrors} mirrors)");
        }
        self.file.dirty = true;
    }
    fn select_snippet_groups(&mut self, ranges: Vec<(usize, usize)>) {
        if let Some((start, _)) = ranges.first().copied() {
            self.session.editor.set_selections_bytes(ranges);
            self.session.cursor = start;
            self.file.dirty = true;
        }
    }
    pub(crate) fn next_snippet_stop(&mut self) -> bool {
        let Some(mut session) = self.overlays.lsp_results.snippet.take() else {
            return false;
        };
        if session.next_stop().is_some() {
            let ranges = session.mirror_ranges();
            self.overlays.lsp_results.snippet = Some(session);
            self.select_snippet_groups(ranges);
            return true;
        }
        let end = session
            .current()
            .map(|(_, end)| end)
            .unwrap_or(self.session.cursor);
        self.session.cursor = end;
        self.session.editor.set_selection_bytes(end, end);
        self.file.dirty = true;
        true
    }
    pub(crate) fn previous_snippet_stop(&mut self) -> bool {
        let Some(mut session) = self.overlays.lsp_results.snippet.take() else {
            return false;
        };
        let moved = session.previous();
        if moved.is_some() {
            let ranges = session.mirror_ranges();
            self.overlays.lsp_results.snippet = Some(session);
            self.select_snippet_groups(ranges);
        } else {
            self.overlays.lsp_results.snippet = Some(session);
        }
        true
    }
    pub(crate) fn cancel_snippet(&mut self) {
        self.overlays.lsp_results.snippet = None;
        self.file.dirty = true;
    }
    pub(crate) fn move_completion(&mut self, delta: isize) {
        if let Some(completion) = self.overlays.lsp_results.completion.as_mut() {
            completion.selected = completion
                .selected
                .saturating_add_signed(delta)
                .min(completion.filtered.len().saturating_sub(1));
            self.file.dirty = true;
        }
        self.request_completion_resolve();
    }
    pub(crate) fn filter_completion(&mut self) {
        if let Some(completion) = self.overlays.lsp_results.completion.as_mut() {
            let query = completion.query.to_lowercase();
            let items = &completion.items;
            let mut filtered = completion
                .filtered
                .iter()
                .copied()
                .filter(|index| items[*index].filter_text.to_lowercase().contains(&query))
                .collect::<Vec<_>>();
            filtered.sort_by(|left, right| {
                let left_item = &items[*left];
                let right_item = &items[*right];
                let left_prefix =
                    !query.is_empty() && left_item.filter_text.to_lowercase().starts_with(&query);
                let right_prefix =
                    !query.is_empty() && right_item.filter_text.to_lowercase().starts_with(&query);
                right_prefix
                    .cmp(&left_prefix)
                    .then_with(|| {
                        if left_prefix {
                            left_item
                                .label
                                .chars()
                                .count()
                                .cmp(&right_item.label.chars().count())
                        } else {
                            std::cmp::Ordering::Equal
                        }
                    })
                    .then_with(|| left_item.sort_text.cmp(&right_item.sort_text))
                    .then_with(|| left_item.label.cmp(&right_item.label))
            });
            completion.filtered = filtered;
            let preferred = if query.is_empty() {
                completion
                    .filtered
                    .iter()
                    .position(|index| items[*index].preselect)
                    .unwrap_or(0)
            } else {
                completion
                    .filtered
                    .iter()
                    .position(|index| items[*index].preselect)
                    .or_else(|| {
                        completion.filtered.iter().position(|index| {
                            items[*index].filter_text.to_lowercase().starts_with(&query)
                        })
                    })
                    .unwrap_or(0)
            };
            completion.selected = preferred.min(completion.filtered.len().saturating_sub(1));
            self.file.dirty = true;
        }
        self.request_completion_resolve();
    }
}

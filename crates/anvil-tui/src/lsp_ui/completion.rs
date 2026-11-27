use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CompletionMatch {
    rank: u8,
    gaps: usize,
    start: usize,
    length: usize,
}

fn completion_match(text: &str, query: &str) -> Option<CompletionMatch> {
    let text_chars = text.chars().collect::<Vec<_>>();
    let lower = text.to_lowercase().chars().collect::<Vec<_>>();
    let query = query.to_lowercase().chars().collect::<Vec<_>>();
    let length = text_chars.len();
    if query.is_empty() {
        return Some(CompletionMatch {
            rank: 0,
            gaps: 0,
            start: 0,
            length: 0,
        });
    }
    if query.len() > lower.len() {
        return None;
    }
    if lower.starts_with(&query) {
        return Some(CompletionMatch {
            rank: 0,
            gaps: 0,
            start: 0,
            length,
        });
    }
    if let Some(start) = lower
        .windows(query.len())
        .position(|window| window == query.as_slice())
    {
        return Some(CompletionMatch {
            rank: 2,
            gaps: 0,
            start,
            length,
        });
    }

    let mut positions = Vec::with_capacity(query.len());
    let mut cursor = 0;
    for character in &query {
        let offset = lower[cursor..]
            .iter()
            .position(|candidate| candidate == character)?;
        let position = cursor + offset;
        positions.push(position);
        cursor = position + 1;
    }
    let boundary = positions.iter().all(|&position| {
        position == 0
            || !lower[position - 1].is_alphanumeric()
            || (position < text_chars.len()
                && text_chars[position].is_uppercase()
                && text_chars[position - 1].is_lowercase())
    });
    Some(CompletionMatch {
        rank: if boundary { 1 } else { 3 },
        gaps: positions
            .windows(2)
            .map(|pair| pair[1].saturating_sub(pair[0] + 1))
            .sum(),
        start: positions.first().copied().unwrap_or(0),
        length,
    })
}

impl Model {
    pub(crate) fn cancel_completion(&mut self) {
        self.language.lsp_requests.cancel(|kind| {
            matches!(
                kind,
                LspRequestKind::Completion | LspRequestKind::CompletionResolve
            )
        });
        self.language.completion.completion = None;
        self.language.completion.resolve = None;
        self.language.completion.incomplete = false;
        self.language.completion.inline = None;
        self.close_overlay(OverlayKind::Completion);
    }
    pub(crate) fn auto_completion(&mut self) {
        let text = self.session.editor.text();
        let text = text.slice(0..text.len_bytes());
        let cursor = self.session.cursor.min(text.len());
        let prefix = &text[..cursor];
        let trigger = self
            .language
            .lsp
            .config
            .as_ref()
            .and_then(|key| self.language.completion.support.get(key))
            .and_then(|characters| {
                characters
                    .iter()
                    .find(|character| prefix.ends_with(character.as_str()))
                    .cloned()
            });
        if let Some(trigger) = trigger {
            self.start_completion(Some(&trigger), true);
            return;
        }
        if !prefix.ends_with('/') {
            return;
        }
        let local = self.local_completion_items();
        if local.is_empty() {
            return;
        }
        self.cancel_completion();
        self.install_completion(local);
        self.status.text = "completion".to_owned();
        self.file.dirty = true;
    }
    pub(crate) fn request_completion(&mut self) {
        self.start_completion(None, false);
    }

    fn start_completion(&mut self, trigger: Option<&str>, quiet: bool) {
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
        let Some(path) = self.session.path.clone() else {
            self.file.dirty = true;
            return;
        };
        if self.active_lsp_worker().is_none() {
            if !quiet && trigger.is_none() && !had_local {
                self.status.text = "E: lsp is not running".to_owned();
            }
            self.file.dirty = true;
            return;
        }
        let Some((line, character)) = self.lsp_position() else {
            return;
        };
        let context = match trigger {
            Some(trigger) => serde_json::json!({
                "triggerKind": 2,
                "triggerCharacter": trigger
            }),
            None => serde_json::json!({"triggerKind": 1}),
        };
        let params = serde_json::json!({
            "textDocument": {"uri": anvil_lsp_client::file_uri(&path)},
            "position": {"line": line, "character": character},
            "context": context
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

    pub(crate) fn schedule_keyword_completion(&mut self) {
        let settings = self.config.settings.completion;
        if (!settings.auto_popup && !settings.inline_completion)
            || !matches!(self.ui.input_mode, InputMode::Insert)
            || self.language.completion.completion.is_some()
            || self.language.completion.snippet.is_some()
        {
            self.language.completion.keyword_due = None;
            self.language.completion.inline = None;
            self.language.completion.inline_requests.clear();
            return;
        }
        self.language.completion.inline = None;
        self.language.completion.inline_requests.clear();
        let text = self.session.editor.text();
        let text = text.slice(0..text.len_bytes());
        let cursor = self.session.cursor.min(text.len());
        let prefix = crate::completion_sources::completion_prefix(&text, cursor);
        if prefix.chars().count() < settings.popup_trigger_len {
            self.language.completion.keyword_due = None;
            return;
        }
        let class = {
            let cache = self.syntax_cache.highlights.borrow();
            cache
                .highlights()
                .iter()
                .find(|highlight| highlight.start <= cursor && cursor < highlight.end)
                .map(|highlight| highlight.class)
        };
        let allowed = match class {
            Some(anvil_syntax::SyntaxClass::Comment) => settings.popup_in_comments,
            Some(anvil_syntax::SyntaxClass::String) => settings.popup_in_strings,
            _ => true,
        };
        if !allowed {
            self.language.completion.keyword_due = None;
            return;
        }
        self.language.completion.keyword_signature = Some((self.session.editor.revision(), cursor));
        self.language.completion.keyword_due = Some(
            std::time::Instant::now()
                + std::time::Duration::from_millis(u64::from(settings.popup_delay_ms)),
        );
    }

    pub(crate) fn tick_keyword_completion(&mut self) {
        if !matches!(self.ui.input_mode, InputMode::Insert)
            || self.language.completion.completion.is_some()
            || self.language.completion.snippet.is_some()
        {
            self.language.completion.keyword_due = None;
            return;
        }
        let Some(due) = self.language.completion.keyword_due else {
            return;
        };
        if std::time::Instant::now() < due {
            return;
        }
        let current = (self.session.editor.revision(), self.session.cursor);
        if self.language.completion.keyword_signature != Some(current) {
            self.language.completion.keyword_due = None;
            return;
        }
        self.language.completion.keyword_due = None;
        if self.config.settings.completion.inline_completion {
            self.request_inline_completion();
        } else {
            self.start_completion(None, true);
        }
    }

    pub(crate) fn request_inline_completion(&mut self) {
        if !self.config.settings.completion.inline_completion
            || !matches!(self.ui.input_mode, InputMode::Insert)
            || self.language.completion.completion.is_some()
            || self.language.completion.snippet.is_some()
            || self.active_lsp_worker().is_none()
        {
            self.language.completion.inline = None;
            self.language.completion.inline_requests.clear();
            return;
        }
        self.language.completion.inline = None;
        let Some(path) = self.session.path.clone() else {
            return;
        };
        let Some((line, character)) = self.lsp_position() else {
            return;
        };
        let request_revision = self.session.editor.revision();
        let request_cursor = self.session.cursor;
        let params = serde_json::json!({
            "textDocument": {"uri": anvil_lsp_client::file_uri(&path)},
            "position": {"line": line, "character": character},
            "context": {"triggerKind": 1}
        });
        if let Some(id) = self.send_lsp_request_id(
            LspRequestKind::InlineCompletion,
            "textDocument/inlineCompletion",
            params,
        ) {
            self.language
                .completion
                .inline_requests
                .insert(id, (request_revision, request_cursor, Some(path)));
        }
    }

    pub(crate) fn apply_inline_completion(&mut self) -> bool {
        let Some(inline) = self.language.completion.inline.take() else {
            return false;
        };
        if inline.revision != self.session.editor.revision()
            || inline.cursor != self.session.cursor
            || inline.path != self.session.path
        {
            return false;
        }
        if let Some((start_line, start_char, end_line, end_char)) = inline.replace
            && let (Some(start), Some(end)) = (
                self.try_offset_at_utf16(start_line, start_char),
                self.try_offset_at_utf16(end_line, end_char),
            )
            && start <= end
            && start <= self.session.cursor
        {
            let end = end.max(self.session.cursor);
            if self.apply_completion_transaction(start, end, &inline.text) {
                self.session.cursor = start + inline.text.len();
                self.file.modified = true;
                self.file.dirty = true;
                return true;
            }
        }
        self.insert_text(&inline.text);
        self.file.dirty = true;
        true
    }

    pub(crate) fn request_completion_more(&mut self) {
        if !self.language.completion.incomplete
            || self.language.completion.completion.is_none()
            || self.active_lsp_worker().is_none()
        {
            return;
        }
        let Some(path) = self.session.path.clone() else {
            return;
        };
        let Some((line, character)) = self.lsp_position() else {
            return;
        };
        self.language
            .lsp_requests
            .cancel(|kind| kind == LspRequestKind::Completion);
        let params = serde_json::json!({
            "textDocument": {"uri": anvil_lsp_client::file_uri(&path)},
            "position": {"line": line, "character": character},
            "context": {"triggerKind": 3}
        });
        if self.send_lsp_request(
            LspRequestKind::Completion,
            "textDocument/completion",
            params,
        ) {
            self.language.completion.incomplete = false;
        }
    }
    pub(crate) fn request_completion_resolve(&mut self) {
        let Some(completion) = self.language.completion.completion.as_ref() else {
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
                .language
                .completion
                .resolve
                .is_some_and(|(_, pending)| pending == index)
        {
            return;
        }
        let raw = item.raw.clone();
        let id = self.language.lsp_requests.peek_request_id();
        if self.send_lsp_request(
            LspRequestKind::CompletionResolve,
            "completionItem/resolve",
            raw,
        ) {
            self.language.completion.resolve = Some((id, index));
        }
    }
    pub(crate) fn apply_completion(&mut self) {
        let Some(completion) = self.language.completion.completion.take() else {
            return;
        };
        self.language
            .lsp_requests
            .cancel(|kind| kind == LspRequestKind::Completion);
        self.language.completion.incomplete = false;
        self.close_overlay(OverlayKind::Completion);
        let query_len = completion.query.len();
        let Some(item) = completion
            .filtered
            .get(completion.selected)
            .and_then(|index| completion.items.get(*index))
        else {
            return;
        };
        let label = item.label.clone();
        self.language.completion.recent = Some(label.clone());
        let additional = crate::completion_sources::completion_additional_edits(&item.raw);
        let mut snippet = item.snippet.clone();
        let mut insert = item.insert.clone();
        if item
            .raw
            .get("insertTextMode")
            .and_then(serde_json::Value::as_u64)
            == Some(2)
        {
            insert = self.adjust_completion_indentation(&insert, &mut snippet);
        }
        let auto_brackets = self.should_auto_bracket(item, &insert);
        if auto_brackets {
            insert.push_str("()");
        }
        let replace = item.replace;
        let replace_bytes = item.replace_bytes;
        self.language.completion.snippet = None;
        if let Some((start, end)) = replace_bytes {
            let end = end.max(start).max(self.session.cursor);
            if !self.apply_completion_transaction(start, end, &insert) {
                return;
            }
            self.session.cursor = start + insert.len().saturating_sub(usize::from(auto_brackets));
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
            let end = end.max(start).max(self.session.cursor);
            if !self.apply_completion_transaction(start, end, &insert) {
                return;
            }
            self.session.cursor = start + insert.len().saturating_sub(usize::from(auto_brackets));
            self.file.modified = true;
            if let Some(snippet) = snippet.take() {
                self.start_snippet(&snippet, start);
            }
        } else {
            let selections = self.session.editor.selection_bytes();
            if selections.len() != 1 {
                self.insert_text(&insert);
            } else {
                let (anchor, head) = selections[0];
                let (start, end) = if anchor == head {
                    (
                        self.session.cursor.saturating_sub(query_len),
                        self.session.cursor,
                    )
                } else {
                    (anchor.min(head), anchor.max(head))
                };
                if !self.apply_completion_transaction(start, end, &insert) {
                    return;
                }
                self.session.cursor =
                    start + insert.len().saturating_sub(usize::from(auto_brackets));
                self.file.modified = true;
                if let Some(snippet) = snippet.take() {
                    self.start_snippet(&snippet, start);
                }
            }
        }
        if !self.apply_additional_completion_edits(&additional) {
            return;
        }
        let snippet_active = self.language.completion.snippet.is_some();
        let callable = insert.contains('(');
        let command_requested = self.execute_completion_command(&item.raw);
        self.status.text = format!("completed {label}");
        self.file.dirty = true;
        if !snippet_active {
            self.auto_completion();
        }
        if !command_requested
            && callable
            && self.language.completion.completion.is_none()
            && self.active_lsp_worker().is_some()
        {
            self.request_signature_help(None);
        }
    }

    fn should_auto_bracket(&self, item: &crate::state::CompletionItem, insert: &str) -> bool {
        if !self.config.settings.completion.auto_brackets
            || item.snippet.is_some()
            || insert.is_empty()
            || insert.contains('(')
            || self.auto_brackets_blocked()
        {
            return false;
        }
        let kind = item.raw.get("kind").and_then(serde_json::Value::as_u64);
        matches!(kind, Some(2..=4)) && self.session.editor.selection_bytes().len() == 1
    }

    fn auto_brackets_blocked(&self) -> bool {
        if let Some(extension) = self
            .session
            .path
            .as_deref()
            .and_then(|path| path.extension())
            .and_then(|extension| extension.to_str())
            && matches!(
                extension.to_ascii_lowercase().as_str(),
                "tsx" | "jsx" | "vue"
            )
        {
            return true;
        }
        matches!(
            self.language.lsp.config.as_ref().map(|key| key.1.as_str()),
            Some("typescriptreact" | "javascriptreact" | "vue")
        )
    }

    fn adjust_completion_indentation(
        &self,
        text: &str,
        snippet: &mut Option<crate::snippet::Snippet>,
    ) -> String {
        if !text.contains('\n') {
            return text.to_owned();
        }
        let buffer = self.session.editor.text();
        let buffer = buffer.slice(0..buffer.len_bytes());
        let cursor = self.session.cursor.min(buffer.len());
        let line_start = buffer[..cursor].rfind('\n').map_or(0, |index| index + 1);
        let indent = buffer[line_start..cursor]
            .chars()
            .take_while(|character| matches!(character, ' ' | '\t'))
            .collect::<String>();
        if indent.is_empty() {
            return text.to_owned();
        }
        let characters = text.chars().collect::<Vec<_>>();
        let mut output = String::with_capacity(text.len());
        for (index, character) in characters.iter().enumerate() {
            output.push(*character);
            if *character == '\n' && characters.get(index + 1).is_some_and(|next| *next != '\n') {
                output.push_str(&indent);
            }
        }
        if let Some(snippet) = snippet {
            for stop in &mut snippet.stops {
                let added = characters
                    .iter()
                    .enumerate()
                    .filter(|(index, character)| {
                        **character == '\n'
                            && *index < stop.start
                            && characters.get(index + 1).is_some_and(|next| *next != '\n')
                    })
                    .count()
                    * indent.len();
                stop.start += added;
                stop.end += added;
            }
        }
        output
    }

    fn execute_completion_command(&mut self, raw: &serde_json::Value) -> bool {
        let Some(command) = raw
            .pointer("/command/command")
            .and_then(serde_json::Value::as_str)
        else {
            return false;
        };
        if command == "editor.action.triggerParameterHints" {
            if self.active_lsp_worker().is_some() {
                self.request_signature_help(None);
            }
            return true;
        }
        if self.active_lsp_worker().is_none() {
            return false;
        }
        let arguments = raw
            .pointer("/command/arguments")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
        let params = serde_json::json!({
            "command": command,
            "arguments": arguments
        });
        self.send_lsp_request(
            LspRequestKind::CompletionCommand,
            "workspace/executeCommand",
            params,
        )
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
            if let Some(snippet) = self.language.completion.snippet.as_mut() {
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
        self.language.completion.snippet = SnippetSession::new(snippet.offset_stops(base));
        self.goto_snippet_stop();
    }
    fn goto_snippet_stop(&mut self) {
        let ranges = self
            .language
            .completion
            .snippet
            .as_ref()
            .map(SnippetSession::mirror_ranges)
            .unwrap_or_default();
        let Some((start, _)) = ranges.first().copied() else {
            self.language.completion.snippet = None;
            return;
        };
        let mirrors = ranges.len();
        let _ = self.session.editor.try_set_selections_bytes(ranges);
        self.session.cursor = start;
        if mirrors > 1 {
            self.status.text = format!("snippet stop ({mirrors} mirrors)");
        }
        self.file.dirty = true;
    }
    fn select_snippet_groups(&mut self, ranges: Vec<(usize, usize)>) {
        if let Some((start, _)) = ranges.first().copied() {
            let _ = self.session.editor.try_set_selections_bytes(ranges);
            self.session.cursor = start;
            self.file.dirty = true;
        }
    }
    pub(crate) fn next_snippet_stop(&mut self) -> bool {
        let Some(mut session) = self.language.completion.snippet.take() else {
            return false;
        };
        if session.next_stop().is_some() {
            let ranges = session.mirror_ranges();
            self.language.completion.snippet = Some(session);
            self.select_snippet_groups(ranges);
            return true;
        }
        let end = session
            .current()
            .map(|(_, end)| end)
            .unwrap_or(self.session.cursor);
        self.session.cursor = end;
        let _ = self.session.editor.try_set_selection_bytes(end, end);
        self.file.dirty = true;
        true
    }
    pub(crate) fn previous_snippet_stop(&mut self) -> bool {
        let Some(mut session) = self.language.completion.snippet.take() else {
            return false;
        };
        let moved = session.previous();
        if moved.is_some() {
            let ranges = session.mirror_ranges();
            self.language.completion.snippet = Some(session);
            self.select_snippet_groups(ranges);
        } else {
            self.language.completion.snippet = Some(session);
        }
        true
    }
    pub(crate) fn cancel_snippet(&mut self) {
        self.language.completion.snippet = None;
        self.file.dirty = true;
    }
    pub(crate) fn move_completion(&mut self, delta: isize) {
        if let Some(completion) = self.language.completion.completion.as_mut() {
            completion.selected = completion
                .selected
                .saturating_add_signed(delta)
                .min(completion.filtered.len().saturating_sub(1));
            self.file.dirty = true;
        }
        self.language.completion.documentation_scroll = 0;
        self.request_completion_resolve();
    }
    pub(crate) fn filter_completion(&mut self) {
        if let Some(completion) = self.language.completion.completion.as_mut() {
            let query = completion.query.to_lowercase();
            let previous = completion
                .filtered
                .get(completion.selected)
                .and_then(|index| completion.items.get(*index))
                .map(|item| item.label.clone());
            let empty_query = query.is_empty();
            let items = &completion.items;
            let mut filtered = (0..items.len())
                .filter_map(|index| {
                    completion_match(&items[index].filter_text, &query).map(|score| (index, score))
                })
                .collect::<Vec<_>>();
            filtered.sort_by(|(left, left_score), (right, right_score)| {
                if empty_query {
                    return items[*left]
                        .sort_text
                        .cmp(&items[*right].sort_text)
                        .then_with(|| items[*left].label.cmp(&items[*right].label));
                }
                left_score
                    .rank
                    .cmp(&right_score.rank)
                    .then_with(|| left_score.gaps.cmp(&right_score.gaps))
                    .then_with(|| left_score.start.cmp(&right_score.start))
                    .then_with(|| left_score.length.cmp(&right_score.length))
                    .then_with(|| items[*left].sort_text.cmp(&items[*right].sort_text))
                    .then_with(|| items[*left].label.cmp(&items[*right].label))
            });
            completion.filtered = filtered
                .into_iter()
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let preferred = previous
                .and_then(|label| {
                    completion
                        .filtered
                        .iter()
                        .position(|index| items[*index].label == label)
                })
                .or_else(|| {
                    if query.is_empty() {
                        completion
                            .filtered
                            .iter()
                            .position(|index| items[*index].preselect)
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
                    }
                })
                .unwrap_or(0);
            completion.selected = preferred.min(completion.filtered.len().saturating_sub(1));
            self.file.dirty = true;
        }
        self.request_completion_resolve();
    }
}

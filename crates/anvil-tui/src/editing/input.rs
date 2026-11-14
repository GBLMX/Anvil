use super::*;
use anvil_platform::clipboard::{Clipboard as _, SystemClipboard};

impl Model {
    pub(crate) fn insert_auto_pair(&mut self, character: char) {
        if self.reject_read_only() {
            return;
        }
        let selections = self.session.editor.selection_bytes();
        if selections.len() != 1 {
            self.insert_text(&character.to_string());
            return;
        }
        let (anchor, head) = selections[0];
        if anchor != head {
            if let Some(close) = matching_close(character) {
                self.surround_selection(character, close);
            } else {
                self.insert_text(&character.to_string());
            }
            return;
        }
        let text = self.session.editor.text();
        let next = self
            .session
            .editor
            .next_grapheme_boundary(self.session.cursor)
            .and_then(|end| text.slice(self.session.cursor..end).chars().next());
        if let Some(close) = closing_pair(character)
            && let Some(next) = next
            && next == close
        {
            self.move_char(1);
            return;
        }
        if let Some(close) = matching_close(character) {
            self.insert_text(&format!("{character}{close}"));
            self.move_char(-1);
        } else {
            self.insert_text(&character.to_string());
        }
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.reject_read_only() {
            return;
        }
        let selections = self.session.editor.selection_bytes();
        let mirror_edit = self
            .overlays
            .lsp_results
            .snippet
            .as_ref()
            .is_some_and(|snippet| snippet.matches_ranges(&selections));
        if self.overlays.lsp_results.snippet.is_some() {
            if selections.len() > 1 && !mirror_edit {
                self.overlays.lsp_results.snippet = None;
            } else if selections.len() == 1
                && let Some((anchor, head)) = selections.first().copied()
            {
                let start = anchor.min(head);
                let end = anchor.max(head);
                if let Some(snippet) = self.overlays.lsp_results.snippet.as_mut() {
                    snippet.adjust_delete(start, end);
                    snippet.adjust_insert(start, text.len());
                }
            }
        }
        let old_selections = selections.clone();
        if selections.len() > 1 || selections.iter().any(|(anchor, head)| anchor != head) {
            if self.session.editor.insert_at_selections(text) {
                if mirror_edit {
                    let carets = self
                        .session
                        .editor
                        .selection_bytes()
                        .iter()
                        .map(|(start, _)| start.saturating_add(text.len()))
                        .collect::<Vec<_>>();
                    if let Some(snippet) = self.overlays.lsp_results.snippet.as_mut() {
                        snippet.replace_current_with_carets(&old_selections, &carets);
                    }
                    self.session.editor.set_selections_bytes(
                        carets.iter().map(|caret| (*caret, *caret)).collect(),
                    );
                    self.session.cursor = carets.first().copied().unwrap_or(self.session.cursor);
                } else {
                    self.sync_cursor_from_selection();
                }
                self.file.modified = true;
                self.edit.capture_insert(text);
            }
            self.maybe_request_signature_help(text);
            self.file.dirty = true;
            return;
        }
        self.session
            .editor
            .set_selection_bytes(self.session.cursor, self.session.cursor);
        self.session.editor.insert(self.session.cursor, text);
        self.session.cursor += text.len();
        self.file.modified = true;
        self.edit.capture_insert(text);
        self.maybe_request_signature_help(text);
        self.file.dirty = true;
    }

    fn maybe_request_signature_help(&mut self, text: &str) {
        if let Some(trigger) = text
            .chars()
            .last()
            .filter(|character| matches!(character, '(' | ','))
        {
            self.request_signature_help(trigger);
        }
    }

    pub(crate) fn sync_cursor_from_selection(&mut self) {
        self.session.cursor = self
            .session
            .editor
            .selection_bytes()
            .first()
            .map(|(_, head)| *head)
            .unwrap_or(0)
            .min(self.session.editor.text().len_bytes());
    }

    pub(crate) fn undo(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if self.session.editor.undo() {
            self.sync_cursor_from_selection();
            self.file.modified = true;
        } else {
            self.status.text = "Already at oldest change".to_owned();
        }
        self.file.dirty = true;
    }

    pub(crate) fn redo(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if self.session.editor.redo() {
            self.sync_cursor_from_selection();
            self.file.modified = true;
        } else {
            self.status.text = "Already at newest change".to_owned();
        }
        self.file.dirty = true;
    }

    pub(crate) fn delete_lines(&mut self, count: usize) {
        if self.reject_read_only() {
            return;
        }
        let Some(position) = self
            .session
            .editor
            .index()
            .position(self.session.editor.text(), self.session.cursor)
        else {
            return;
        };
        let Some(start) = self.session.editor.index().line_to_offset(position.line) else {
            return;
        };
        let Some(last_start) = self
            .session
            .editor
            .index()
            .line_to_offset(position.line.saturating_add(count.saturating_sub(1)))
        else {
            return;
        };
        let Some(last) = self.session.editor.line_range_at(last_start) else {
            return;
        };
        let deleted = self.session.editor.text().slice(start..last.end);
        self.session.editor.set_selection_bytes(start, last.end);
        self.session.editor.delete(start..last.end);
        self.session.cursor = start;
        self.edit.last_change = Some(LastChange::DeleteLines { count });
        if let Some(register) = self.input.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.file.modified = true;
        self.file.dirty = true;
    }

    pub(crate) fn change_lines(&mut self, count: usize) {
        if self.reject_read_only() {
            return;
        }
        let Some(position) = self
            .session
            .editor
            .index()
            .position(self.session.editor.text(), self.session.cursor)
        else {
            return;
        };
        let Some(start) = self.session.editor.index().line_to_offset(position.line) else {
            return;
        };
        let Some(last_start) = self
            .session
            .editor
            .index()
            .line_to_offset(position.line.saturating_add(count.saturating_sub(1)))
        else {
            return;
        };
        let Some(last) = self.session.editor.line_range_at(last_start) else {
            return;
        };
        let mut end = last.end;
        if end > start && self.session.editor.text().slice(end - 1..end) == "\n" {
            end -= 1;
        }
        if end > start && self.session.editor.text().slice(end - 1..end) == "\r" {
            end -= 1;
        }
        let deleted = self.session.editor.text().slice(start..end);
        self.session.editor.set_selection_bytes(start, end);
        self.session.editor.delete(start..end);
        self.session.cursor = start;
        if let Some(register) = self.input.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.file.modified = true;
        self.begin_insert(InsertOrigin::InsertHere);
        self.file.dirty = true;
    }

    pub(crate) fn yank_lines(&mut self, count: usize) {
        let Some(position) = self
            .session
            .editor
            .index()
            .position(self.session.editor.text(), self.session.cursor)
        else {
            return;
        };
        let Some(start) = self.session.editor.index().line_to_offset(position.line) else {
            return;
        };
        let Some(last_start) = self
            .session
            .editor
            .index()
            .line_to_offset(position.line.saturating_add(count.saturating_sub(1)))
        else {
            return;
        };
        let Some(last) = self.session.editor.line_range_at(last_start) else {
            return;
        };
        if self.session.editor.yank(start..last.end.max(start)) {
            self.record_register();
            self.status.text = format!("{count} lines yanked");
        }
        self.file.dirty = true;
    }

    pub(crate) fn record_register(&mut self) {
        let Some(register) = self.input.register.take() else {
            return;
        };
        let text = self.session.editor.yank_text().to_owned();
        self.record_register_text(register, &text);
    }

    pub(crate) fn record_register_text(&mut self, register: char, text: &str) {
        match register {
            '_' => {}
            '+' => {
                if let Err(error) = SystemClipboard.set_text(text) {
                    self.status.text = format!("E: {error}");
                    self.file.dirty = true;
                }
            }
            _ => {
                self.session.editor.set_register(register, text);
            }
        }
    }

    pub(crate) fn put_register(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if let Some(register) = self.input.register.take() {
            if register == '_' {
                return;
            }
            let text = if register == '+' {
                match SystemClipboard.get_text() {
                    Ok(Some(text)) => text,
                    Ok(None) => {
                        self.status.text = "E: clipboard is empty".to_owned();
                        self.file.dirty = true;
                        return;
                    }
                    Err(error) => {
                        self.status.text = format!("E: {error}");
                        self.file.dirty = true;
                        return;
                    }
                }
            } else {
                let Some(text) = self
                    .session
                    .editor
                    .register_text(register)
                    .map(str::to_owned)
                else {
                    self.status.text = "E: register empty".to_owned();
                    self.file.dirty = true;
                    return;
                };
                text
            };
            self.session
                .editor
                .set_selection_bytes(self.session.cursor, self.session.cursor);
            self.session.editor.insert(self.session.cursor, &text);
            self.session.cursor += text.len();
            self.file.modified = true;
            self.file.dirty = true;
            return;
        }
        if self.session.editor.put(self.session.cursor).is_some() {
            self.sync_cursor_from_selection();
            self.file.modified = true;
        } else {
            self.status.text = "E: register empty".to_owned();
        }
        self.file.dirty = true;
    }

    pub(crate) fn append(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if let Some(line) = self
            .session
            .editor
            .index()
            .offset_to_line(self.session.cursor)
            && self.session.cursor < line.end
        {
            self.move_char(1);
        }
        self.begin_insert(InsertOrigin::Append);
        self.file.dirty = true;
    }

    pub(crate) fn append_line_end(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if let Some(line) = self
            .session
            .editor
            .index()
            .offset_to_line(self.session.cursor)
        {
            self.session.cursor = line.end;
        }
        self.begin_insert(InsertOrigin::AppendLineEnd);
        self.file.dirty = true;
    }

    pub(crate) fn open_below(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if let Some(line) = self
            .session
            .editor
            .index()
            .offset_to_line(self.session.cursor)
        {
            self.session.cursor = line.end;
        }
        self.insert_text("\n");
        self.begin_insert(InsertOrigin::OpenBelow);
    }

    pub(crate) fn open_above(&mut self) {
        if self.reject_read_only() {
            return;
        }
        let start = self
            .session
            .editor
            .index()
            .offset_to_line(self.session.cursor)
            .map_or(0, |line| line.start);
        self.session.cursor = start;
        self.insert_text("\n");
        self.session.cursor = start;
        self.begin_insert(InsertOrigin::OpenAbove);
    }

    pub(crate) fn backspace(&mut self) {
        if self.reject_read_only() {
            return;
        }
        let selections = self.session.editor.selection_bytes();
        if selections.len() == 1 {
            let (anchor, head) = selections[0];
            if anchor == head && self.session.cursor > 0 {
                let text = self.session.editor.text();
                if let Some(previous) = self
                    .session
                    .editor
                    .previous_grapheme_boundary(self.session.cursor)
                {
                    let opening = text
                        .slice(previous..self.session.cursor)
                        .chars()
                        .next()
                        .and_then(matching_close);
                    let next = self
                        .session
                        .editor
                        .next_grapheme_boundary(self.session.cursor)
                        .and_then(|end| text.slice(self.session.cursor..end).chars().next());
                    if let (Some(close), Some(next)) = (opening, next)
                        && next == close
                    {
                        let end = self
                            .session
                            .editor
                            .next_grapheme_boundary(self.session.cursor)
                            .unwrap_or(self.session.cursor);
                        self.session.editor.set_selection_bytes(previous, end);
                        self.session.editor.delete(previous..end);
                        self.session.cursor = previous;
                        self.file.modified = true;
                        self.file.dirty = true;
                        return;
                    }
                }
            }
        }
        self.cancel_signature_help();
        let at_insert_end = self.edit.insert_session.as_ref().is_some_and(|session| {
            session.valid && self.session.cursor == session.start + session.text.len()
        });
        if self.delete_selections(false) {
            self.edit.trim_insert(false);
            return;
        }
        let Some(start) = self
            .session
            .editor
            .previous_grapheme_boundary(self.session.cursor)
        else {
            return;
        };
        if start == self.session.cursor {
            return;
        }
        if let Some(snippet) = self.overlays.lsp_results.snippet.as_mut() {
            snippet.adjust_delete(start, self.session.cursor);
        }
        self.session
            .editor
            .set_selection_bytes(start, self.session.cursor);
        self.session.editor.delete(start..self.session.cursor);
        self.session.cursor = start;
        self.file.modified = true;
        self.edit.trim_insert(at_insert_end);
    }

    pub(crate) fn delete_forward(&mut self) {
        if self.reject_read_only() {
            return;
        }
        self.cancel_signature_help();
        if self.delete_selections(true) {
            return;
        }
        let Some(end) = self
            .session
            .editor
            .next_grapheme_boundary(self.session.cursor)
        else {
            return;
        };
        if end == self.session.cursor {
            return;
        }
        if let Some(snippet) = self.overlays.lsp_results.snippet.as_mut() {
            snippet.adjust_delete(self.session.cursor, end);
        }
        self.session
            .editor
            .set_selection_bytes(self.session.cursor, end);
        self.session.editor.delete(self.session.cursor..end);
        self.file.modified = true;
        self.file.dirty = true;
    }

    pub(super) fn delete_selections(&mut self, forward: bool) -> bool {
        let selections = self.session.editor.selection_bytes();
        if selections.len() == 1 && selections[0].0 == selections[0].1 {
            return false;
        }
        let mirror_delete = self
            .overlays
            .lsp_results
            .snippet
            .as_ref()
            .is_some_and(|snippet| snippet.matches_ranges(&selections));
        if self.overlays.lsp_results.snippet.is_some() {
            if selections.len() > 1 && !mirror_delete {
                self.overlays.lsp_results.snippet = None;
            } else if selections.len() == 1
                && let Some((anchor, head)) = selections.first().copied()
            {
                let start = anchor.min(head);
                let end = anchor.max(head);
                if let Some(snippet) = self.overlays.lsp_results.snippet.as_mut() {
                    snippet.adjust_delete(start, end);
                }
            }
        }
        let old_selections = selections.clone();
        let mut edits = Vec::new();
        for (anchor, head) in selections {
            let start = anchor.min(head);
            let end = anchor.max(head);
            if start == end {
                let target = if forward {
                    self.session.editor.next_grapheme_boundary(start)
                } else {
                    self.session.editor.previous_grapheme_boundary(start)
                };
                let Some(target) = target else {
                    continue;
                };
                if target == start {
                    continue;
                }
                edits.push((start.min(target)..start.max(target), String::new()));
            } else {
                edits.push((start..end, String::new()));
            }
        }
        if edits.is_empty() || !self.session.editor.apply_transaction(edits) {
            return false;
        }
        if mirror_delete {
            let carets = self
                .session
                .editor
                .selection_bytes()
                .iter()
                .map(|(start, _)| *start)
                .collect::<Vec<_>>();
            if let Some(snippet) = self.overlays.lsp_results.snippet.as_mut() {
                snippet.replace_current_with_carets(&old_selections, &carets);
            }
            self.session
                .editor
                .set_selections_bytes(carets.iter().map(|caret| (*caret, *caret)).collect());
            self.session.cursor = carets.first().copied().unwrap_or(self.session.cursor);
        } else {
            self.sync_cursor_from_selection();
        }
        self.file.modified = true;
        self.file.dirty = true;
        true
    }
}

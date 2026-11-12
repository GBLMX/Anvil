use super::*;

impl Model {
    pub(crate) fn insert_here(&mut self) {
        if self.reject_read_only() {
            return;
        }
        self.begin_insert(InsertOrigin::InsertHere);
        self.file.dirty = true;
    }

    pub(crate) fn select_word_occurrences(&mut self) {
        let text = self
            .session
            .editor
            .text()
            .slice(0..self.session.editor.text().len_bytes());
        let cursor = self.session.cursor.min(text.len());
        let Some((start, end)) = word_bounds(&text, cursor) else {
            self.status.text = "E: no word under cursor".to_owned();
            self.file.dirty = true;
            return;
        };
        let word = &text[start..end];
        let mut selections = Vec::new();
        for (offset, _) in text.match_indices(word) {
            let match_end = offset + word.len();
            let left_ok = offset == 0
                || !text[..offset]
                    .chars()
                    .next_back()
                    .is_some_and(is_word_character);
            let right_ok = match_end == text.len()
                || !text[match_end..]
                    .chars()
                    .next()
                    .is_some_and(is_word_character);
            if left_ok && right_ok {
                selections.push((offset, match_end));
            }
        }
        if selections.is_empty() || !self.session.editor.set_selections_bytes(selections) {
            return;
        }
        self.session.cursor = self
            .session
            .editor
            .selection_bytes()
            .first()
            .map(|(start, _)| *start)
            .unwrap_or(cursor);
        self.begin_insert(InsertOrigin::InsertHere);
        self.status.text = format!("{} selections", self.session.editor.selection_bytes().len());
        self.file.dirty = true;
    }

    pub(crate) fn split_selection_lines(&mut self) {
        let selections = self.session.editor.selection_bytes();
        let mut carets = Vec::new();
        for (anchor, head) in selections {
            let start = anchor.min(head);
            let end = anchor.max(head);
            let Some(first_line) = self.session.editor.index().offset_to_line(start) else {
                continue;
            };
            let Some(last_line) = self.session.editor.index().offset_to_line(end) else {
                continue;
            };
            for line in first_line.line..=last_line.line {
                if let Some(offset) = self.session.editor.index().line_to_offset(line) {
                    carets.push((offset, offset));
                }
            }
        }
        carets.sort_unstable();
        carets.dedup();
        if carets.len() < 2 || !self.session.editor.set_selections_bytes(carets) {
            self.status.text = "E: no lines to split".to_owned();
            self.file.dirty = true;
            return;
        }
        self.session.cursor = self
            .session
            .editor
            .selection_bytes()
            .first()
            .map(|(start, _)| *start)
            .unwrap_or(self.session.cursor);
        self.status.text = format!("{} cursors", self.session.editor.selection_bytes().len());
        self.file.dirty = true;
    }

    pub(crate) fn extend_line_cursor(&mut self, up: bool) {
        let Some(line) = self
            .session
            .editor
            .index()
            .offset_to_line(self.session.cursor)
        else {
            return;
        };
        let target_line = if up {
            line.line.checked_sub(1)
        } else {
            Some(line.line + 1)
        };
        let Some(target_line) =
            target_line.and_then(|line| self.session.editor.index().line_to_offset(line))
        else {
            return;
        };
        let Some(target) = self.session.editor.index().offset_to_line(target_line) else {
            return;
        };
        let column = self.session.cursor.saturating_sub(line.start);
        let target_offset = target.start + column.min(target.end.saturating_sub(target.start));
        let mut selections = self.session.editor.selection_bytes();
        if selections.len() == 1
            && selections[0].0 == selections[0].1
            && selections[0].0 != self.session.cursor
        {
            selections[0] = (self.session.cursor, self.session.cursor);
        }
        selections.push((target_offset, target_offset));
        selections.sort_unstable();
        selections.dedup();
        if !self.session.editor.set_selections_bytes(selections) {
            return;
        }
        self.session.cursor = target_offset;
        self.status.text = format!("{} cursors", self.session.editor.selection_bytes().len());
        self.file.dirty = true;
    }

    pub(crate) fn collapse_selections(&mut self) {
        let selections = self.session.editor.selection_bytes();
        if selections.len() <= 1 && selections.iter().all(|(anchor, head)| anchor == head) {
            return;
        }
        self.sync_cursor_from_selection();
        self.session
            .editor
            .set_selection_bytes(self.session.cursor, self.session.cursor);
        self.status.text = "cursors merged".to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn begin_visual(&mut self, line: bool) {
        self.edit.visual_anchor = Some(self.session.cursor);
        self.edit.visual_line = line;
        self.sync_visual_selection();
        self.file.dirty = true;
    }

    pub(crate) fn exit_visual(&mut self) {
        self.edit.visual_anchor = None;
        self.session
            .editor
            .set_selection_bytes(self.session.cursor, self.session.cursor);
        self.file.dirty = true;
    }

    pub(crate) fn sync_visual_selection(&mut self) {
        let Some(anchor) = self.edit.visual_anchor else {
            return;
        };
        let (start, end) = if self.edit.visual_line {
            let Some(anchor_line) = self.session.editor.index().offset_to_line(anchor) else {
                return;
            };
            let Some(cursor_line) = self
                .session
                .editor
                .index()
                .offset_to_line(self.session.cursor)
            else {
                return;
            };
            let start_line = anchor_line.line.min(cursor_line.line);
            let end_line = anchor_line.line.max(cursor_line.line);
            let Some(start) = self.session.editor.index().line_to_offset(start_line) else {
                return;
            };
            let Some(last) = self.session.editor.index().line_to_offset(end_line) else {
                return;
            };
            let Some(last) = self.session.editor.line_range_at(last) else {
                return;
            };
            (start, last.end)
        } else {
            let start = anchor.min(self.session.cursor);
            let last = anchor.max(self.session.cursor);
            let end = self
                .session
                .editor
                .next_grapheme_boundary(last)
                .unwrap_or(last);
            (start, end)
        };
        if start < end {
            self.session.editor.set_selection_bytes(start, end);
        }
    }

    pub(crate) fn yank_visual(&mut self) {
        let Some((anchor, head)) = self.session.editor.selection_bytes().first().copied() else {
            return;
        };
        let range = anchor.min(head)..anchor.max(head);
        if range.start < range.end && self.session.editor.yank(range) {
            self.record_register();
        }
        self.exit_visual();
    }

    pub(crate) fn delete_visual(&mut self) {
        if self.reject_read_only() {
            self.exit_visual();
            return;
        }
        let selections = self.session.editor.selection_bytes();
        let deleted = selections
            .first()
            .copied()
            .map(|(anchor, head)| {
                self.session
                    .editor
                    .text()
                    .slice(anchor.min(head)..anchor.max(head))
            })
            .unwrap_or_default();
        if let Some((anchor, head)) = selections.first().copied() {
            let range = anchor.min(head)..anchor.max(head);
            if range.start < range.end {
                let _ = self.session.editor.yank(range);
            }
        }
        if let Some(register) = self.input.register.take() {
            self.record_register_text(register, &deleted);
        }
        if !self.delete_selections(false) {
            self.exit_visual();
            return;
        }
        self.exit_visual();
    }

    pub(crate) fn toggle_comment(&mut self) {
        if self.reject_read_only() {
            return;
        }
        let Some(path) = self.session.path.as_deref() else {
            self.status.text = "E: comment: unnamed buffer".to_owned();
            self.file.dirty = true;
            return;
        };
        let Some(style) = comment::comment_style(path) else {
            self.status.text = "E: comment: unsupported file type".to_owned();
            self.file.dirty = true;
            return;
        };
        let lines = self.selected_line_ranges();
        if lines.is_empty() {
            self.status.text = "E: comment: no lines".to_owned();
            self.file.dirty = true;
            return;
        }
        if lines.len() > comment::MAX_COMMENT_LINES {
            self.status.text = "E: comment: too many lines".to_owned();
            self.file.dirty = true;
            return;
        }
        let text = self
            .session
            .editor
            .text()
            .slice(0..self.session.editor.text().len_bytes());
        let Some(edits) = comment::toggle_edits(&text, &lines, style.line) else {
            self.status.text = "E: comment: no changes".to_owned();
            self.file.dirty = true;
            return;
        };
        let count = lines.len();
        if !self.session.editor.apply_transaction(edits) {
            self.status.text = "E: comment: edit failed".to_owned();
            self.file.dirty = true;
            return;
        }
        self.sync_cursor_from_selection();
        self.file.modified = true;
        self.file.dirty = true;
        self.status.text = format!("toggled comment on {count} line(s)");
    }

    fn selected_line_ranges(&self) -> Vec<std::ops::Range<usize>> {
        let mut ranges = Vec::new();
        if self.session.editor.text().len_bytes() == 0 {
            ranges.push(0..0);
            return ranges;
        }
        for (anchor, head) in self.session.editor.selection_bytes() {
            let start = anchor.min(head);
            let end = anchor.max(head);
            let Some(first) = self.session.editor.line_range_at(start) else {
                continue;
            };
            let last_offset = if end > start {
                self.session
                    .editor
                    .previous_grapheme_boundary(end)
                    .unwrap_or(end)
            } else {
                end
            };
            let Some(last) = self.session.editor.line_range_at(last_offset) else {
                continue;
            };
            let mut at = first.start;
            loop {
                let Some(range) = self.session.editor.line_range_at(at) else {
                    break;
                };
                let done = range.end >= last.end || range.end == at;
                ranges.push(range.clone());
                if done || ranges.len() > comment::MAX_COMMENT_LINES {
                    break;
                }
                at = range.end;
            }
        }
        ranges.sort_by_key(|range| range.start);
        ranges.dedup_by_key(|range| range.start);
        ranges
    }
}

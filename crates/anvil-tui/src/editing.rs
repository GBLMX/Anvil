//! Buffer editing, motions, registers, and undo operations.

use super::*;

impl Model {
    pub(crate) fn move_words(&mut self, motion: WordMotion) {
        let count = self.take_count();
        for _ in 0..count {
            let target = match motion {
                WordMotion::NextStart => next_word_start(self.editor.text(), self.cursor),
                WordMotion::PreviousStart => previous_word_start(self.editor.text(), self.cursor),
                WordMotion::End => end_word(self.editor.text(), self.cursor),
            };
            if target == self.cursor {
                break;
            }
            self.cursor = target;
            self.dirty = true;
        }
    }

    pub fn move_word(&mut self, motion: WordMotion) {
        self.move_words(motion);
    }

    pub(crate) fn change_word(&mut self, motion: WordMotion) {
        if self.reject_read_only() {
            return;
        }
        let Some(range) = self.word_range(motion) else {
            return;
        };
        let deleted = self.editor.text().slice(range.clone());
        self.editor.set_selection_bytes(range.start, range.end);
        self.editor.delete(range.clone());
        if let Some(register) = self.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.cursor = range.start;
        self.modified = true;
        self.begin_insert(InsertOrigin::ChangeWord(motion));
        self.dirty = true;
    }

    pub(crate) fn change_inner_word(&mut self) {
        if self.reject_read_only() {
            return;
        }
        let Some(range) = self.inner_word_range() else {
            self.status = "E: no word".to_owned();
            self.dirty = true;
            return;
        };
        let deleted = self.editor.text().slice(range.clone());
        self.editor.set_selection_bytes(range.start, range.end);
        self.editor.delete(range.clone());
        if let Some(register) = self.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.cursor = range.start;
        self.modified = true;
        self.begin_insert(InsertOrigin::ChangeInnerWord);
        self.dirty = true;
    }

    pub(crate) fn change_inner_quote(&mut self, quote: char) {
        if self.reject_read_only() {
            return;
        }
        let Some(range) = self.inner_quote_range(quote) else {
            self.status = "E: no quote pair".to_owned();
            self.dirty = true;
            return;
        };
        let deleted = self.editor.text().slice(range.clone());
        self.editor.set_selection_bytes(range.start, range.end);
        self.editor.delete(range.clone());
        if let Some(register) = self.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.cursor = range.start;
        self.modified = true;
        self.begin_insert(InsertOrigin::ChangeInnerQuote(quote));
        self.dirty = true;
    }

    pub(crate) fn inner_word_range(&self) -> Option<std::ops::Range<usize>> {
        let line = self.editor.line_range_at(self.cursor)?;
        let text = self.editor.text().slice(line.start..line.end);
        let local = self.cursor.saturating_sub(line.start).min(text.len());
        let class_of = |character: char| {
            if character.is_whitespace() {
                0
            } else if character.is_alphanumeric() || character == '_' {
                1
            } else {
                2
            }
        };
        let current = text[local..]
            .chars()
            .next()
            .map(class_of)
            .filter(|class| *class != 0)
            .or_else(|| text[..local].chars().next_back().map(class_of))?;
        let mut start = local;
        for (index, character) in text[..local].char_indices().rev() {
            if class_of(character) != current {
                break;
            }
            start = index;
        }
        let mut end = local;
        for (index, character) in text[local..].char_indices() {
            if class_of(character) != current {
                break;
            }
            end = local + index + character.len_utf8();
        }
        (start < end).then_some(line.start + start..line.start + end)
    }

    pub(crate) fn inner_quote_range(&self, quote: char) -> Option<std::ops::Range<usize>> {
        let line = self.editor.line_range_at(self.cursor)?;
        let text = self.editor.text().slice(line.start..line.end);
        let local = self.cursor.saturating_sub(line.start).min(text.len());
        let left = text[..local].rfind(quote)?;
        let right = text[local..].find(quote)? + local;
        (left + quote.len_utf8() <= right)
            .then_some(line.start + left + quote.len_utf8()..line.start + right)
    }

    pub(crate) fn around_quote_range(&self, quote: char) -> Option<std::ops::Range<usize>> {
        let inner = self.inner_quote_range(quote)?;
        let text = self.editor.text();
        let start = inner.start.checked_sub(quote.len_utf8())?;
        let end = inner.end.checked_add(quote.len_utf8())?;
        (end <= text.len_bytes()
            && text.slice(start..inner.start) == quote.to_string()
            && text.slice(inner.end..end) == quote.to_string())
        .then_some(start..end)
    }

    pub(crate) fn bracket_range(&self, open: char, around: bool) -> Option<std::ops::Range<usize>> {
        let close = match open {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            '<' => '>',
            _ => return None,
        };
        let line = self.editor.line_range_at(self.cursor)?;
        let text = self.editor.text().slice(line.start..line.end);
        let local = self.cursor.saturating_sub(line.start).min(text.len());
        let mut stack = Vec::new();
        for (index, character) in text.char_indices() {
            if character == open {
                stack.push(index);
            } else if character == close
                && let Some(start) = stack.pop()
                && start <= local
                && local <= index
            {
                let (start, end) = if around {
                    (start, index + close.len_utf8())
                } else {
                    (start + open.len_utf8(), index)
                };
                return (start < end).then_some(line.start + start..line.start + end);
            }
        }
        None
    }

    pub(crate) fn around_word_range(&self) -> Option<std::ops::Range<usize>> {
        let inner = self.inner_word_range()?;
        let line = self.editor.line_range_at(inner.start)?;
        let text = self.editor.text().slice(line.start..line.end);
        let mut start = inner.start - line.start;
        let mut end = inner.end - line.start;
        while let Some(character) = text[end..].chars().next() {
            if !character.is_whitespace() {
                break;
            }
            end += character.len_utf8();
        }
        if end == inner.end - line.start {
            while start > 0 {
                let character = text[..start].chars().next_back()?;
                if !character.is_whitespace() {
                    break;
                }
                start -= character.len_utf8();
            }
        }
        Some(line.start + start..line.start + end)
    }

    fn word_object_range(&mut self, around: bool, count: usize) -> Option<std::ops::Range<usize>> {
        let saved = self.cursor;
        let mut cursor = saved;
        let mut selected: Option<std::ops::Range<usize>> = None;
        for iteration in 0..count.max(1) {
            if iteration > 0 {
                let text = self.editor.text();
                while let Some(character) = text.slice(cursor..text.len_bytes()).chars().next() {
                    if !character.is_whitespace() {
                        break;
                    }
                    cursor += character.len_utf8();
                }
            }
            self.cursor = cursor;
            let Some(next) = (if around {
                self.around_word_range()
            } else {
                self.inner_word_range()
            }) else {
                break;
            };
            cursor = next.end;
            if let Some(range) = selected.as_mut() {
                range.end = next.end;
            } else {
                selected = Some(next);
            }
        }
        self.cursor = saved;
        selected
    }

    pub(crate) fn apply_word_operator(&mut self, operator: char, around: bool, count: usize) {
        let range = self.word_object_range(around, count);
        let Some(range) = range else {
            self.status = "E: no word object".to_owned();
            self.dirty = true;
            return;
        };
        match operator {
            'y' => {
                let cursor = self.cursor;
                self.editor.set_selection_bytes(range.start, range.end);
                if self.editor.yank(range) {
                    self.record_register();
                }
                let _ = self.editor.set_selection_bytes(cursor, cursor);
            }
            'c' => {
                let deleted = self.editor.text().slice(range.clone());
                self.editor.set_selection_bytes(range.start, range.end);
                self.editor.delete(range.clone());
                self.cursor = range.start;
                if let Some(register) = self.register.take() {
                    self.record_register_text(register, &deleted);
                }
                self.modified = true;
                self.begin_insert(if around {
                    InsertOrigin::ChangeWord(WordMotion::NextStart)
                } else {
                    InsertOrigin::ChangeInnerWord
                });
            }
            'd' => {
                let deleted = self.editor.text().slice(range.clone());
                self.editor.set_selection_bytes(range.start, range.end);
                self.editor.delete(range.clone());
                self.cursor = range.start;
                if let Some(register) = self.register.take() {
                    self.record_register_text(register, &deleted);
                }
                self.modified = true;
            }
            _ => return,
        }
        self.dirty = true;
    }

    pub(crate) fn apply_quote_operator(&mut self, operator: char, quote: char, around: bool) {
        let range = if around {
            self.around_quote_range(quote)
        } else {
            self.inner_quote_range(quote)
        };
        let Some(range) = range else {
            self.status = "E: no quote object".to_owned();
            self.dirty = true;
            return;
        };
        match operator {
            'y' => {
                let cursor = self.cursor;
                self.editor.set_selection_bytes(range.start, range.end);
                if self.editor.yank(range) {
                    self.record_register();
                }
                let _ = self.editor.set_selection_bytes(cursor, cursor);
            }
            'c' => {
                let deleted = self.editor.text().slice(range.clone());
                self.editor.set_selection_bytes(range.start, range.end);
                self.editor.delete(range.clone());
                self.cursor = range.start;
                if let Some(register) = self.register.take() {
                    self.record_register_text(register, &deleted);
                }
                self.modified = true;
                self.begin_insert(if around {
                    InsertOrigin::ChangeAroundQuote(quote)
                } else {
                    InsertOrigin::ChangeInnerQuote(quote)
                });
            }
            'd' => {
                let deleted = self.editor.text().slice(range.clone());
                self.editor.set_selection_bytes(range.start, range.end);
                self.editor.delete(range.clone());
                self.cursor = range.start;
                if let Some(register) = self.register.take() {
                    self.record_register_text(register, &deleted);
                }
                self.modified = true;
            }
            _ => return,
        }
        self.dirty = true;
    }

    pub(crate) fn apply_bracket_operator(&mut self, operator: char, open: char, around: bool) {
        let Some(range) = self.bracket_range(open, around) else {
            self.status = "E: no bracket object".to_owned();
            self.dirty = true;
            return;
        };
        match operator {
            'y' => {
                let cursor = self.cursor;
                self.editor.set_selection_bytes(range.start, range.end);
                if self.editor.yank(range) {
                    self.record_register();
                }
                let _ = self.editor.set_selection_bytes(cursor, cursor);
            }
            'c' => {
                let deleted = self.editor.text().slice(range.clone());
                self.editor.set_selection_bytes(range.start, range.end);
                self.editor.delete(range.clone());
                self.cursor = range.start;
                if let Some(register) = self.register.take() {
                    self.record_register_text(register, &deleted);
                }
                self.modified = true;
                self.begin_insert(InsertOrigin::ChangeBracket { open, around });
            }
            'd' => {
                let deleted = self.editor.text().slice(range.clone());
                self.editor.set_selection_bytes(range.start, range.end);
                self.editor.delete(range.clone());
                self.cursor = range.start;
                if let Some(register) = self.register.take() {
                    self.record_register_text(register, &deleted);
                }
                self.modified = true;
            }
            _ => return,
        }
        self.dirty = true;
    }

    fn line_is_blank(&self, line: usize) -> bool {
        let Some(start) = self.editor.index().line_to_offset(line) else {
            return true;
        };
        let Some(range) = self.editor.line_range_at(start) else {
            return true;
        };
        self.editor
            .text()
            .slice(range.start..range.end)
            .trim()
            .is_empty()
    }

    pub(crate) fn paragraph_range(&self, around: bool) -> Option<std::ops::Range<usize>> {
        let position = self
            .editor
            .index()
            .position(self.editor.text(), self.cursor)?;
        let mut start = position.line;
        let mut end = position.line;
        if self.line_is_blank(position.line) {
            while start > 0 && self.line_is_blank(start - 1) {
                start -= 1;
            }
            while end + 1 < self.editor.index().len_lines() && self.line_is_blank(end + 1) {
                end += 1;
            }
        } else {
            while start > 0 && !self.line_is_blank(start - 1) {
                start -= 1;
            }
            while end + 1 < self.editor.index().len_lines() && !self.line_is_blank(end + 1) {
                end += 1;
            }
            if around {
                while end + 1 < self.editor.index().len_lines() && self.line_is_blank(end + 1) {
                    end += 1;
                }
            }
        }
        let start = self.editor.index().line_to_offset(start)?;
        let last = self.editor.index().line_to_offset(end)?;
        let end = self.editor.line_range_at(last)?.end;
        (start < end).then_some(start..end)
    }

    fn paragraph_object_range(
        &mut self,
        around: bool,
        count: usize,
    ) -> Option<std::ops::Range<usize>> {
        let saved = self.cursor;
        let mut cursor = saved;
        let mut selected: Option<std::ops::Range<usize>> = None;
        for iteration in 0..count.max(1) {
            if iteration > 0 {
                let text = self.editor.text();
                while let Some(character) = text.slice(cursor..text.len_bytes()).chars().next() {
                    if !character.is_whitespace() {
                        break;
                    }
                    cursor += character.len_utf8();
                }
            }
            self.cursor = cursor;
            let Some(next) = self.paragraph_range(around) else {
                break;
            };
            cursor = next.end;
            if let Some(range) = selected.as_mut() {
                range.end = next.end;
            } else {
                selected = Some(next);
            }
        }
        self.cursor = saved;
        selected
    }

    pub(crate) fn apply_paragraph_operator(&mut self, operator: char, around: bool, count: usize) {
        let Some(range) = self.paragraph_object_range(around, count) else {
            self.status = "E: no paragraph object".to_owned();
            self.dirty = true;
            return;
        };
        match operator {
            'y' => {
                let cursor = self.cursor;
                self.editor.set_selection_bytes(range.start, range.end);
                if self.editor.yank(range) {
                    self.record_register();
                }
                let _ = self.editor.set_selection_bytes(cursor, cursor);
            }
            'c' => {
                let deleted = self.editor.text().slice(range.clone());
                self.editor.set_selection_bytes(range.start, range.end);
                self.editor.delete(range.clone());
                self.cursor = range.start;
                if let Some(register) = self.register.take() {
                    self.record_register_text(register, &deleted);
                }
                self.modified = true;
                self.begin_insert(InsertOrigin::ChangeParagraph { around });
            }
            'd' => {
                let deleted = self.editor.text().slice(range.clone());
                self.editor.set_selection_bytes(range.start, range.end);
                self.editor.delete(range.clone());
                self.cursor = range.start;
                if let Some(register) = self.register.take() {
                    self.record_register_text(register, &deleted);
                }
                self.modified = true;
            }
            _ => return,
        }
        self.dirty = true;
    }

    pub(crate) fn word_range(&self, motion: WordMotion) -> Option<std::ops::Range<usize>> {
        let text = self.editor.text();
        let (start, end) = match motion {
            WordMotion::NextStart => (self.cursor, next_word_start(text, self.cursor)),
            WordMotion::PreviousStart => (previous_word_start(text, self.cursor), self.cursor),
            WordMotion::End => (self.cursor, self.word_end_exclusive()),
        };
        (start < end).then_some(start..end)
    }

    pub(crate) fn tag_range(&self, around: bool) -> Option<std::ops::Range<usize>> {
        let line = self.editor.line_range_at(self.cursor)?;
        let text = self.editor.text().slice(line.start..line.end);
        let local = self.cursor.saturating_sub(line.start).min(text.len());
        let open_start = text[..local].rfind('<')?;
        if text[open_start..].starts_with("</") {
            return None;
        }
        let open_end = text[open_start..].find('>')? + open_start;
        let name = text[open_start + 1..open_end]
            .split(|character: char| character.is_whitespace() || character == '/')
            .next()?;
        if name.is_empty() {
            return None;
        }
        let close = format!("</{name}>");
        let close_start = text[open_end + 1..].find(&close)? + open_end + 1;
        let close_end = close_start + close.len();
        let (start, end) = if around {
            (open_start, close_end)
        } else {
            (open_end + 1, close_start)
        };
        (start < end).then_some(line.start + start..line.start + end)
    }

    pub(crate) fn apply_tag_operator(&mut self, operator: char, around: bool) {
        let Some(range) = self.tag_range(around) else {
            self.status = "E: no tag object".to_owned();
            self.dirty = true;
            return;
        };
        match operator {
            'y' => {
                let cursor = self.cursor;
                self.editor.set_selection_bytes(range.start, range.end);
                if self.editor.yank(range) {
                    self.record_register();
                }
                let _ = self.editor.set_selection_bytes(cursor, cursor);
            }
            'c' => {
                let deleted = self.editor.text().slice(range.clone());
                self.editor.set_selection_bytes(range.start, range.end);
                self.editor.delete(range.clone());
                self.cursor = range.start;
                if let Some(register) = self.register.take() {
                    self.record_register_text(register, &deleted);
                }
                self.modified = true;
                self.begin_insert(InsertOrigin::ChangeTag { around });
            }
            'd' => {
                let deleted = self.editor.text().slice(range.clone());
                self.editor.set_selection_bytes(range.start, range.end);
                self.editor.delete(range.clone());
                self.cursor = range.start;
                if let Some(register) = self.register.take() {
                    self.record_register_text(register, &deleted);
                }
                self.modified = true;
            }
            _ => return,
        }
        self.dirty = true;
    }

    pub(crate) fn word_end_exclusive(&self) -> usize {
        let text = self.editor.text();
        let end = end_word(text, self.cursor);
        self.editor
            .next_grapheme_boundary(end)
            .filter(|next| *next > end)
            .unwrap_or(end)
            .min(text.len_bytes())
    }

    pub(crate) fn delete_words(&mut self, motion: WordMotion, count: usize) {
        let mut deleted = String::new();
        for _ in 0..count {
            let Some(range) = self.word_range(motion) else {
                break;
            };
            deleted.push_str(&self.editor.text().slice(range.clone()));
            self.editor.set_selection_bytes(range.start, range.end);
            self.editor.delete(range.clone());
            self.cursor = range.start;
            self.modified = true;
        }
        if !deleted.is_empty() {
            self.last_change = Some(LastChange::DeleteWords { motion, count });
        }
        if let Some(register) = self.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.dirty = true;
    }

    pub(crate) fn yank_words(&mut self, motion: WordMotion, count: usize) {
        for _ in 0..count {
            self.yank_word(motion);
        }
    }

    pub(crate) fn yank_word(&mut self, motion: WordMotion) {
        let Some(range) = self.word_range(motion) else {
            return;
        };
        let cursor = self.cursor;
        self.editor.set_selection_bytes(range.start, range.end);
        if self.editor.yank(range) {
            self.record_register();
        }
        let _ = self.editor.set_selection_bytes(cursor, cursor);
        self.dirty = true;
    }

    pub(crate) fn move_lines(&mut self, delta: isize) {
        let count = self.take_count();
        for _ in 0..count {
            self.move_line(delta);
        }
    }

    pub(crate) fn move_line(&mut self, delta: isize) {
        let Some(position) = self
            .editor
            .index()
            .position(self.editor.text(), self.cursor)
        else {
            return;
        };
        let target = position.line as isize + delta;
        if target < 0 {
            return;
        }
        let Some(offset) = self.editor.index().offset_at_display_column(
            self.editor.text(),
            target as usize,
            position.display_column,
        ) else {
            return;
        };
        self.set_caret(offset);
        self.dirty = true;
    }

    pub(crate) fn move_chars(&mut self, delta: isize) {
        let count = self.take_count();
        for _ in 0..count {
            self.move_char(delta);
        }
    }

    pub(crate) fn move_char(&mut self, delta: isize) {
        let next = if delta < 0 {
            self.editor.previous_grapheme_boundary(self.cursor)
        } else {
            self.editor.next_grapheme_boundary(self.cursor)
        };
        if let Some(offset) = next {
            self.set_caret(offset);
            self.dirty = true;
        }
    }

    pub(crate) fn move_home(&mut self) {
        if let Some(line) = self.editor.index().offset_to_line(self.cursor) {
            self.set_caret(line.start);
            self.dirty = true;
        }
    }

    pub(crate) fn move_end(&mut self) {
        if let Some(line) = self.editor.index().offset_to_line(self.cursor) {
            self.set_caret(line.end);
            self.dirty = true;
        }
    }

    fn set_caret(&mut self, offset: usize) {
        if let Some(session) = self.insert_session.as_mut() {
            session.valid = false;
        }
        self.cursor = offset;
        if self.editor.selection_bytes().len() <= 1 {
            self.editor.set_selection_bytes(offset, offset);
        }
    }

    pub(crate) fn ensure_cursor_visible(&mut self, height: u16, width: u16) {
        let Some(position) = self
            .editor
            .index()
            .position(self.editor.text(), self.cursor)
        else {
            return;
        };
        let line = position.line;
        let height = usize::from(height.max(1));
        let margin = 3.min(height.saturating_sub(1) / 2);
        if line < self.scroll.saturating_add(margin) {
            self.scroll = line.saturating_sub(margin);
        } else if line + margin >= self.scroll.saturating_add(height) {
            self.scroll = line + margin + 1 - height;
        }
        let column = position.display_column;
        let width = usize::from(width.max(1));
        if column < self.hscroll {
            self.hscroll = column;
        } else if column >= self.hscroll.saturating_add(width) {
            self.hscroll = column + 1 - width;
        }
    }

    #[cfg(feature = "e2e-bench")]
    #[doc(hidden)]
    pub fn ensure_cursor_visible_for_bench(&mut self, height: u16, width: u16) {
        self.ensure_cursor_visible(height, width);
    }

    pub(crate) fn insert_here(&mut self) {
        if self.reject_read_only() {
            return;
        }
        self.begin_insert(InsertOrigin::InsertHere);
        self.dirty = true;
    }

    pub(crate) fn select_word_occurrences(&mut self) {
        let text = self.editor.text().slice(0..self.editor.text().len_bytes());
        let cursor = self.cursor.min(text.len());
        let Some((start, end)) = word_bounds(&text, cursor) else {
            self.status = "E: no word under cursor".to_owned();
            self.dirty = true;
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
        if selections.is_empty() || !self.editor.set_selections_bytes(selections) {
            return;
        }
        self.cursor = self
            .editor
            .selection_bytes()
            .first()
            .map(|(start, _)| *start)
            .unwrap_or(cursor);
        self.begin_insert(InsertOrigin::InsertHere);
        self.status = format!("{} selections", self.editor.selection_bytes().len());
        self.dirty = true;
    }

    pub(crate) fn split_selection_lines(&mut self) {
        let selections = self.editor.selection_bytes();
        let mut carets = Vec::new();
        for (anchor, head) in selections {
            let start = anchor.min(head);
            let end = anchor.max(head);
            let Some(first_line) = self.editor.index().offset_to_line(start) else {
                continue;
            };
            let Some(last_line) = self.editor.index().offset_to_line(end) else {
                continue;
            };
            for line in first_line.line..=last_line.line {
                if let Some(offset) = self.editor.index().line_to_offset(line) {
                    carets.push((offset, offset));
                }
            }
        }
        carets.sort_unstable();
        carets.dedup();
        if carets.len() < 2 || !self.editor.set_selections_bytes(carets) {
            self.status = "E: no lines to split".to_owned();
            self.dirty = true;
            return;
        }
        self.cursor = self
            .editor
            .selection_bytes()
            .first()
            .map(|(start, _)| *start)
            .unwrap_or(self.cursor);
        self.status = format!("{} cursors", self.editor.selection_bytes().len());
        self.dirty = true;
    }

    pub(crate) fn extend_line_cursor(&mut self, up: bool) {
        let Some(line) = self.editor.index().offset_to_line(self.cursor) else {
            return;
        };
        let target_line = if up {
            line.line.checked_sub(1)
        } else {
            Some(line.line + 1)
        };
        let Some(target_line) =
            target_line.and_then(|line| self.editor.index().line_to_offset(line))
        else {
            return;
        };
        let Some(target) = self.editor.index().offset_to_line(target_line) else {
            return;
        };
        let column = self.cursor.saturating_sub(line.start);
        let target_offset = target.start + column.min(target.end.saturating_sub(target.start));
        let mut selections = self.editor.selection_bytes();
        if selections.len() == 1
            && selections[0].0 == selections[0].1
            && selections[0].0 != self.cursor
        {
            selections[0] = (self.cursor, self.cursor);
        }
        selections.push((target_offset, target_offset));
        selections.sort_unstable();
        selections.dedup();
        if !self.editor.set_selections_bytes(selections) {
            return;
        }
        self.cursor = target_offset;
        self.status = format!("{} cursors", self.editor.selection_bytes().len());
        self.dirty = true;
    }

    pub(crate) fn collapse_selections(&mut self) {
        let selections = self.editor.selection_bytes();
        if selections.len() <= 1 && selections.iter().all(|(anchor, head)| anchor == head) {
            return;
        }
        self.sync_cursor_from_selection();
        self.editor.set_selection_bytes(self.cursor, self.cursor);
        self.status = "cursors merged".to_owned();
        self.dirty = true;
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.reject_read_only() {
            return;
        }
        let selections = self.editor.selection_bytes();
        if selections.len() > 1 || selections.iter().any(|(anchor, head)| anchor != head) {
            if self.editor.insert_at_selections(text) {
                self.sync_cursor_from_selection();
                self.modified = true;
                self.capture_insert(text);
            }
            self.maybe_request_signature_help(text);
            self.dirty = true;
            return;
        }
        self.editor.set_selection_bytes(self.cursor, self.cursor);
        self.editor.insert(self.cursor, text);
        self.cursor += text.len();
        self.modified = true;
        self.capture_insert(text);
        self.maybe_request_signature_help(text);
        self.dirty = true;
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
        self.cursor = self
            .editor
            .selection_bytes()
            .first()
            .map(|(_, head)| *head)
            .unwrap_or(0)
            .min(self.editor.text().len_bytes());
    }

    pub(crate) fn undo(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if self.editor.undo() {
            self.sync_cursor_from_selection();
            self.modified = true;
        } else {
            self.status = "Already at oldest change".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn redo(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if self.editor.redo() {
            self.sync_cursor_from_selection();
            self.modified = true;
        } else {
            self.status = "Already at newest change".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn delete_lines(&mut self, count: usize) {
        if self.reject_read_only() {
            return;
        }
        let Some(position) = self
            .editor
            .index()
            .position(self.editor.text(), self.cursor)
        else {
            return;
        };
        let Some(start) = self.editor.index().line_to_offset(position.line) else {
            return;
        };
        let Some(last_start) = self
            .editor
            .index()
            .line_to_offset(position.line.saturating_add(count.saturating_sub(1)))
        else {
            return;
        };
        let Some(last) = self.editor.line_range_at(last_start) else {
            return;
        };
        let deleted = self.editor.text().slice(start..last.end);
        self.editor.set_selection_bytes(start, last.end);
        self.editor.delete(start..last.end);
        self.cursor = start;
        self.last_change = Some(LastChange::DeleteLines { count });
        if let Some(register) = self.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.modified = true;
        self.dirty = true;
    }

    pub(crate) fn yank_lines(&mut self, count: usize) {
        let Some(position) = self
            .editor
            .index()
            .position(self.editor.text(), self.cursor)
        else {
            return;
        };
        let Some(start) = self.editor.index().line_to_offset(position.line) else {
            return;
        };
        let Some(last_start) = self
            .editor
            .index()
            .line_to_offset(position.line.saturating_add(count.saturating_sub(1)))
        else {
            return;
        };
        let Some(last) = self.editor.line_range_at(last_start) else {
            return;
        };
        if self.editor.yank(start..last.end.max(start)) {
            self.record_register();
            self.status = format!("{count} lines yanked");
        }
        self.dirty = true;
    }

    pub(crate) fn record_register(&mut self) {
        let Some(register) = self.register.take() else {
            return;
        };
        let text = self.editor.yank_text().to_owned();
        self.record_register_text(register, &text);
    }

    pub(crate) fn record_register_text(&mut self, register: char, text: &str) {
        match register {
            '_' => {}
            '+' => {
                if let Err(error) = clipboard::copy(text) {
                    self.status = format!("E: {error}");
                    self.dirty = true;
                }
            }
            _ => {
                self.registers.insert(register, text.to_owned());
            }
        }
    }

    pub(crate) fn put_register(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if let Some(register) = self.register.take() {
            if register == '_' {
                return;
            }
            let text = if register == '+' {
                match clipboard::paste() {
                    Ok(text) => text,
                    Err(error) => {
                        self.status = format!("E: {error}");
                        self.dirty = true;
                        return;
                    }
                }
            } else {
                let Some(text) = self.registers.get(&register).cloned() else {
                    self.status = "E: register empty".to_owned();
                    self.dirty = true;
                    return;
                };
                text
            };
            self.editor.set_selection_bytes(self.cursor, self.cursor);
            self.editor.insert(self.cursor, &text);
            self.cursor += text.len();
            self.modified = true;
            self.dirty = true;
            return;
        }
        if self.editor.put(self.cursor).is_some() {
            self.sync_cursor_from_selection();
            self.modified = true;
        } else {
            self.status = "E: register empty".to_owned();
        }
        self.dirty = true;
    }

    pub(crate) fn append(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if let Some(line) = self.editor.index().offset_to_line(self.cursor)
            && self.cursor < line.end
        {
            self.move_char(1);
        }
        self.begin_insert(InsertOrigin::Append);
        self.dirty = true;
    }

    pub(crate) fn append_line_end(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if let Some(line) = self.editor.index().offset_to_line(self.cursor) {
            self.cursor = line.end;
        }
        self.begin_insert(InsertOrigin::AppendLineEnd);
        self.dirty = true;
    }

    pub(crate) fn open_below(&mut self) {
        if self.reject_read_only() {
            return;
        }
        if let Some(line) = self.editor.index().offset_to_line(self.cursor) {
            self.cursor = line.end;
        }
        self.insert_text("\n");
        self.begin_insert(InsertOrigin::OpenBelow);
    }

    pub(crate) fn open_above(&mut self) {
        if self.reject_read_only() {
            return;
        }
        let start = self
            .editor
            .index()
            .offset_to_line(self.cursor)
            .map_or(0, |line| line.start);
        self.cursor = start;
        self.insert_text("\n");
        self.cursor = start;
        self.begin_insert(InsertOrigin::OpenAbove);
    }

    pub(crate) fn backspace(&mut self) {
        if self.reject_read_only() {
            return;
        }
        self.cancel_signature_help();
        let at_insert_end = self.insert_session.as_ref().is_some_and(|session| {
            session.valid && self.cursor == session.start + session.text.len()
        });
        if self.delete_selections(false) {
            self.trim_insert_session(false);
            return;
        }
        let Some(start) = self.editor.previous_grapheme_boundary(self.cursor) else {
            return;
        };
        if start == self.cursor {
            return;
        }
        self.editor.set_selection_bytes(start, self.cursor);
        self.editor.delete(start..self.cursor);
        self.cursor = start;
        self.modified = true;
        self.trim_insert_session(at_insert_end);
    }

    pub(crate) fn delete_forward(&mut self) {
        if self.reject_read_only() {
            return;
        }
        self.cancel_signature_help();
        if self.delete_selections(true) {
            return;
        }
        let Some(end) = self.editor.next_grapheme_boundary(self.cursor) else {
            return;
        };
        if end == self.cursor {
            return;
        }
        self.editor.set_selection_bytes(self.cursor, end);
        self.editor.delete(self.cursor..end);
        self.modified = true;
        self.dirty = true;
    }

    fn delete_selections(&mut self, forward: bool) -> bool {
        let selections = self.editor.selection_bytes();
        if selections.len() == 1 && selections[0].0 == selections[0].1 {
            return false;
        }
        let mut edits = Vec::new();
        for (anchor, head) in selections {
            let start = anchor.min(head);
            let end = anchor.max(head);
            if start == end {
                let target = if forward {
                    self.editor.next_grapheme_boundary(start)
                } else {
                    self.editor.previous_grapheme_boundary(start)
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
        if edits.is_empty() || !self.editor.apply_transaction(edits) {
            return false;
        }
        self.sync_cursor_from_selection();
        self.modified = true;
        self.dirty = true;
        true
    }
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn word_bounds(text: &str, cursor: usize) -> Option<(usize, usize)> {
    let mut start = cursor.min(text.len());
    let mut end = start;
    if (start == text.len() || !text[start..].chars().next().is_some_and(is_word_character))
        && let Some(previous) = text[..start].chars().next_back()
        && is_word_character(previous)
    {
        start -= previous.len_utf8();
        end = start + previous.len_utf8();
    }
    while start > 0 {
        let Some(previous) = text[..start].chars().next_back() else {
            break;
        };
        if !is_word_character(previous) {
            break;
        }
        start -= previous.len_utf8();
    }
    while end < text.len() {
        let Some(next) = text[end..].chars().next() else {
            break;
        };
        if !is_word_character(next) {
            break;
        }
        end += next.len_utf8();
    }
    (start < end).then_some((start, end))
}

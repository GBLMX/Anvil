use super::*;

impl Model {
    pub(crate) fn move_words(&mut self, motion: WordMotion) {
        let count = self.take_count();
        for _ in 0..count {
            let target = match motion {
                WordMotion::NextStart => {
                    next_word_start(self.session.editor.text(), self.session.cursor)
                }
                WordMotion::PreviousStart => {
                    previous_word_start(self.session.editor.text(), self.session.cursor)
                }
                WordMotion::End => end_word(self.session.editor.text(), self.session.cursor),
            };
            if target == self.session.cursor {
                break;
            }
            let forward = !matches!(motion, WordMotion::PreviousStart);
            let Some(target) = self.visible_motion_offset(target, forward, false) else {
                break;
            };
            if target == self.session.cursor {
                break;
            }
            self.session.cursor = target;
            self.file.dirty = true;
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
        let deleted = self.session.editor.text().slice(range.clone());
        self.session
            .editor
            .set_selection_bytes(range.start, range.end);
        self.session.editor.delete(range.clone());
        if let Some(register) = self.input.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.session.cursor = range.start;
        self.file.modified = true;
        self.begin_insert(InsertOrigin::ChangeWord(motion));
        self.file.dirty = true;
    }

    pub(crate) fn change_inner_word(&mut self) {
        if self.reject_read_only() {
            return;
        }
        let Some(range) = self.inner_word_range() else {
            self.status.text = "E: no word".to_owned();
            self.file.dirty = true;
            return;
        };
        let deleted = self.session.editor.text().slice(range.clone());
        self.session
            .editor
            .set_selection_bytes(range.start, range.end);
        self.session.editor.delete(range.clone());
        if let Some(register) = self.input.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.session.cursor = range.start;
        self.file.modified = true;
        self.begin_insert(InsertOrigin::ChangeInnerWord);
        self.file.dirty = true;
    }

    pub(crate) fn change_inner_quote(&mut self, quote: char) {
        if self.reject_read_only() {
            return;
        }
        let Some(range) = self.inner_quote_range(quote) else {
            self.status.text = "E: no quote pair".to_owned();
            self.file.dirty = true;
            return;
        };
        let deleted = self.session.editor.text().slice(range.clone());
        self.session
            .editor
            .set_selection_bytes(range.start, range.end);
        self.session.editor.delete(range.clone());
        if let Some(register) = self.input.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.session.cursor = range.start;
        self.file.modified = true;
        self.begin_insert(InsertOrigin::ChangeInnerQuote(quote));
        self.file.dirty = true;
    }

    pub(crate) fn inner_word_range(&self) -> Option<std::ops::Range<usize>> {
        self.session.editor.inner_word_object(self.session.cursor)
    }

    pub(crate) fn inner_quote_range(&self, quote: char) -> Option<std::ops::Range<usize>> {
        self.session
            .editor
            .inner_quote_object(self.session.cursor, quote)
    }

    pub(crate) fn around_quote_range(&self, quote: char) -> Option<std::ops::Range<usize>> {
        self.session
            .editor
            .around_quote_object(self.session.cursor, quote)
    }

    pub(crate) fn bracket_range(&self, open: char, around: bool) -> Option<std::ops::Range<usize>> {
        self.session
            .editor
            .bracket_object(self.session.cursor, open, around)
    }

    pub(crate) fn around_word_range(&self) -> Option<std::ops::Range<usize>> {
        let inner = self.inner_word_range()?;
        let line = self.session.editor.line_range_at(inner.start)?;
        let text = self.session.editor.text().slice(line.start..line.end);
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
        let saved = self.session.cursor;
        let mut cursor = saved;
        let mut selected: Option<std::ops::Range<usize>> = None;
        for iteration in 0..count.max(1) {
            if iteration > 0 {
                let text = self.session.editor.text();
                while let Some(character) = text.slice(cursor..text.len_bytes()).chars().next() {
                    if !character.is_whitespace() {
                        break;
                    }
                    cursor += character.len_utf8();
                }
            }
            self.session.cursor = cursor;
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
        self.session.cursor = saved;
        selected
    }

    fn apply_operator_range(
        &mut self,
        operator: char,
        range: std::ops::Range<usize>,
        change_origin: InsertOrigin,
    ) {
        match operator {
            'y' => {
                let cursor = self.session.cursor;
                self.session
                    .editor
                    .set_selection_bytes(range.start, range.end);
                if self.session.editor.yank(range) {
                    self.record_register();
                }
                let _ = self.session.editor.set_selection_bytes(cursor, cursor);
            }
            'c' => {
                let deleted = self.session.editor.text().slice(range.clone());
                self.session
                    .editor
                    .set_selection_bytes(range.start, range.end);
                self.session.editor.delete(range.clone());
                self.session.cursor = range.start;
                if let Some(register) = self.input.register.take() {
                    self.record_register_text(register, &deleted);
                }
                self.file.modified = true;
                self.begin_insert(change_origin);
            }
            'd' => {
                let deleted = self.session.editor.text().slice(range.clone());
                self.session
                    .editor
                    .set_selection_bytes(range.start, range.end);
                self.session.editor.delete(range.clone());
                self.session.cursor = range.start;
                if let Some(register) = self.input.register.take() {
                    self.record_register_text(register, &deleted);
                }
                self.file.modified = true;
            }
            _ => return,
        }
        self.file.dirty = true;
    }

    pub(crate) fn apply_word_operator(&mut self, operator: char, around: bool, count: usize) {
        let range = self.word_object_range(around, count);
        let Some(range) = range else {
            self.status.text = "E: no word object".to_owned();
            self.file.dirty = true;
            return;
        };
        let origin = if around {
            InsertOrigin::ChangeWord(WordMotion::NextStart)
        } else {
            InsertOrigin::ChangeInnerWord
        };
        self.apply_operator_range(operator, range, origin);
    }

    pub(crate) fn apply_quote_operator(&mut self, operator: char, quote: char, around: bool) {
        let range = if around {
            self.around_quote_range(quote)
        } else {
            self.inner_quote_range(quote)
        };
        let Some(range) = range else {
            self.status.text = "E: no quote object".to_owned();
            self.file.dirty = true;
            return;
        };
        let origin = if around {
            InsertOrigin::ChangeAroundQuote(quote)
        } else {
            InsertOrigin::ChangeInnerQuote(quote)
        };
        self.apply_operator_range(operator, range, origin);
    }

    pub(crate) fn apply_bracket_operator(&mut self, operator: char, open: char, around: bool) {
        let Some(range) = self.bracket_range(open, around) else {
            self.status.text = "E: no bracket object".to_owned();
            self.file.dirty = true;
            return;
        };
        self.apply_operator_range(
            operator,
            range,
            InsertOrigin::ChangeBracket { open, around },
        );
    }

    pub(crate) fn paragraph_range(&self, around: bool) -> Option<std::ops::Range<usize>> {
        self.session
            .editor
            .paragraph_object(self.session.cursor, around)
    }

    fn paragraph_object_range(
        &mut self,
        around: bool,
        count: usize,
    ) -> Option<std::ops::Range<usize>> {
        let saved = self.session.cursor;
        let mut cursor = saved;
        let mut selected: Option<std::ops::Range<usize>> = None;
        for iteration in 0..count.max(1) {
            if iteration > 0 {
                let text = self.session.editor.text();
                while let Some(character) = text.slice(cursor..text.len_bytes()).chars().next() {
                    if !character.is_whitespace() {
                        break;
                    }
                    cursor += character.len_utf8();
                }
            }
            self.session.cursor = cursor;
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
        self.session.cursor = saved;
        selected
    }

    pub(crate) fn apply_paragraph_operator(&mut self, operator: char, around: bool, count: usize) {
        let Some(range) = self.paragraph_object_range(around, count) else {
            self.status.text = "E: no paragraph object".to_owned();
            self.file.dirty = true;
            return;
        };
        self.apply_operator_range(operator, range, InsertOrigin::ChangeParagraph { around });
    }

    pub(crate) fn word_range(&self, motion: WordMotion) -> Option<std::ops::Range<usize>> {
        let text = self.session.editor.text();
        let (start, end) = match motion {
            WordMotion::NextStart => (
                self.session.cursor,
                next_word_start(text, self.session.cursor),
            ),
            WordMotion::PreviousStart => (
                previous_word_start(text, self.session.cursor),
                self.session.cursor,
            ),
            WordMotion::End => (self.session.cursor, self.word_end_exclusive()),
        };
        (start < end).then_some(start..end)
    }

    pub(crate) fn tag_range(&self, around: bool) -> Option<std::ops::Range<usize>> {
        self.session.editor.tag_object(self.session.cursor, around)
    }

    pub(crate) fn apply_tag_operator(&mut self, operator: char, around: bool) {
        let Some(range) = self.tag_range(around) else {
            self.status.text = "E: no tag object".to_owned();
            self.file.dirty = true;
            return;
        };
        self.apply_operator_range(operator, range, InsertOrigin::ChangeTag { around });
    }

    pub(crate) fn word_end_exclusive(&self) -> usize {
        let text = self.session.editor.text();
        let end = end_word(text, self.session.cursor);
        self.session
            .editor
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
            deleted.push_str(&self.session.editor.text().slice(range.clone()));
            self.session
                .editor
                .set_selection_bytes(range.start, range.end);
            self.session.editor.delete(range.clone());
            self.session.cursor = range.start;
            self.file.modified = true;
        }
        if !deleted.is_empty() {
            self.edit.last_change = Some(LastChange::DeleteWords { motion, count });
        }
        if let Some(register) = self.input.register.take() {
            self.record_register_text(register, &deleted);
        }
        self.file.dirty = true;
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
        let cursor = self.session.cursor;
        self.session
            .editor
            .set_selection_bytes(range.start, range.end);
        if self.session.editor.yank(range) {
            self.record_register();
        }
        let _ = self.session.editor.set_selection_bytes(cursor, cursor);
        self.file.dirty = true;
    }
}

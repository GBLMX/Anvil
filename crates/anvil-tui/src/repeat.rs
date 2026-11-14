//! Semantic last-change recording and repeat.

use super::*;

impl Model {
    pub(crate) fn begin_insert(&mut self, origin: InsertOrigin) {
        self.edit.insert_session = Some(InsertSession {
            origin,
            text: String::new(),
            start: self.session.cursor,
            valid: true,
        });
        self.ui.input_mode = InputMode::Insert;
        self.file.dirty = true;
    }

    pub(crate) fn finish_insert_session(&mut self) {
        self.overlays.lsp_results.snippet = None;
        let Some(session) = self.edit.insert_session.take() else {
            return;
        };
        if !session.valid {
            return;
        }
        if matches!(
            session.origin,
            InsertOrigin::InsertHere | InsertOrigin::Append | InsertOrigin::AppendLineEnd
        ) && session.text.is_empty()
        {
            return;
        }
        let last = match session.origin {
            InsertOrigin::InsertHere => LastChange::InsertAtCursor { text: session.text },
            InsertOrigin::Append => LastChange::Append { text: session.text },
            InsertOrigin::AppendLineEnd => LastChange::AppendLineEnd { text: session.text },
            InsertOrigin::OpenBelow => LastChange::OpenBelow { text: session.text },
            InsertOrigin::OpenAbove => LastChange::OpenAbove { text: session.text },
            InsertOrigin::ChangeWord(motion) => LastChange::ChangeWord {
                motion,
                inserted: session.text,
            },
            InsertOrigin::ChangeInnerWord => LastChange::ChangeInnerWord {
                inserted: session.text,
            },
            InsertOrigin::ChangeInnerQuote(quote) => LastChange::ChangeInnerQuote {
                quote,
                inserted: session.text,
            },
            InsertOrigin::ChangeAroundQuote(quote) => LastChange::ChangeAroundQuote {
                quote,
                inserted: session.text,
            },
            InsertOrigin::ChangeBracket { open, around } => LastChange::ChangeBracket {
                open,
                around,
                inserted: session.text,
            },
            InsertOrigin::ChangeParagraph { around } => LastChange::ChangeParagraph {
                around,
                inserted: session.text,
            },
            InsertOrigin::ChangeTag { around } => LastChange::ChangeTag {
                around,
                inserted: session.text,
            },
        };
        self.edit.last_change = Some(last);
    }

    pub(crate) fn repeat_last_change(&mut self, count: usize) {
        let Some(last) = self.edit.last_change.clone() else {
            self.status.text = "E: no previous change".to_owned();
            self.file.dirty = true;
            return;
        };
        match last {
            LastChange::DeleteWords {
                motion,
                count: original,
            } => {
                self.delete_words(motion, if count == 0 { original } else { count });
            }
            LastChange::DeleteLines { count: original } => {
                self.delete_lines(if count == 0 { original } else { count });
            }
            LastChange::ChangeWord { motion, inserted } => {
                if let Some(range) = self.word_range(motion) {
                    self.replace_now(range, &inserted);
                }
            }
            LastChange::ChangeInnerWord { inserted } => {
                if let Some(range) = self.inner_word_range() {
                    self.replace_now(range, &inserted);
                }
            }
            LastChange::ChangeInnerQuote { quote, inserted } => {
                if let Some(range) = self.inner_quote_range(quote) {
                    self.replace_now(range, &inserted);
                }
            }
            LastChange::ChangeAroundQuote { quote, inserted } => {
                if let Some(range) = self.around_quote_range(quote) {
                    self.replace_now(range, &inserted);
                }
            }
            LastChange::ChangeBracket {
                open,
                around,
                inserted,
            } => {
                if let Some(range) = self.bracket_range(open, around) {
                    self.replace_now(range, &inserted);
                }
            }
            LastChange::ChangeParagraph { around, inserted } => {
                if let Some(range) = self.paragraph_range(around) {
                    self.replace_now(range, &inserted);
                }
            }
            LastChange::ChangeTag { around, inserted } => {
                if let Some(range) = self.tag_range(around) {
                    self.replace_now(range, &inserted);
                }
            }
            LastChange::InsertAtCursor { text } => {
                for _ in 0..count.max(1) {
                    self.insert_now(&text);
                }
            }
            LastChange::Append { text } => {
                for _ in 0..count.max(1) {
                    if let Some(line) = self
                        .session
                        .editor
                        .index()
                        .offset_to_line(self.session.cursor)
                        && self.session.cursor < line.end
                    {
                        self.move_char(1);
                    }
                    self.insert_now(&text);
                }
            }
            LastChange::AppendLineEnd { text } => {
                for _ in 0..count.max(1) {
                    if let Some(line) = self
                        .session
                        .editor
                        .index()
                        .offset_to_line(self.session.cursor)
                    {
                        self.session.cursor = line.end;
                    }
                    self.insert_now(&text);
                }
            }
            LastChange::OpenBelow { text } => {
                if let Some(line) = self
                    .session
                    .editor
                    .index()
                    .offset_to_line(self.session.cursor)
                {
                    self.session.cursor = line.end;
                }
                self.insert_now(&format!("\n{text}"));
            }
            LastChange::OpenAbove { text } => {
                let start = self
                    .session
                    .editor
                    .index()
                    .offset_to_line(self.session.cursor)
                    .map_or(0, |line| line.start);
                self.session.cursor = start;
                self.insert_now(&format!("{text}\n"));
            }
        }
    }

    fn replace_now(&mut self, range: std::ops::Range<usize>, text: &str) {
        self.session
            .editor
            .set_selection_bytes(range.start, range.end);
        self.session.editor.delete(range.clone());
        if !text.is_empty() {
            self.session.editor.insert(range.start, text);
        }
        self.session.cursor = range.start + text.len();
        self.session
            .editor
            .set_selection_bytes(self.session.cursor, self.session.cursor);
        self.file.modified = true;
        self.file.dirty = true;
    }

    fn insert_now(&mut self, text: &str) {
        self.session
            .editor
            .set_selection_bytes(self.session.cursor, self.session.cursor);
        self.session.editor.insert(self.session.cursor, text);
        self.session.cursor += text.len();
        self.file.modified = true;
        self.file.dirty = true;
    }
}

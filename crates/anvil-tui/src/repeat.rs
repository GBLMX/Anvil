//! Semantic last-change recording and repeat.

use super::*;

impl Model {
    pub(crate) fn begin_insert(&mut self, origin: InsertOrigin) {
        self.insert_session = Some(InsertSession {
            origin,
            text: String::new(),
            start: self.cursor,
            valid: true,
        });
        self.ui.input_mode = InputMode::Insert;
        self.dirty = true;
    }

    pub(crate) fn capture_insert(&mut self, text: &str) {
        if let Some(session) = self.insert_session.as_mut() {
            session.text.push_str(text);
        }
    }

    pub(crate) fn finish_insert_session(&mut self) {
        let Some(session) = self.insert_session.take() else {
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
        };
        self.last_change = Some(last);
    }

    pub(crate) fn trim_insert_session(&mut self, at_end: bool) {
        let Some(session) = self.insert_session.as_mut() else {
            return;
        };
        if !at_end {
            session.valid = false;
            return;
        }
        let _ = session.text.pop();
    }

    pub(crate) fn repeat_last_change(&mut self, count: usize) {
        let Some(last) = self.last_change.clone() else {
            self.status = "E: no previous change".to_owned();
            self.dirty = true;
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
            LastChange::InsertAtCursor { text } => {
                for _ in 0..count.max(1) {
                    self.insert_now(&text);
                }
            }
            LastChange::Append { text } => {
                for _ in 0..count.max(1) {
                    if let Some(line) = self.editor.index().offset_to_line(self.cursor)
                        && self.cursor < line.end
                    {
                        self.move_char(1);
                    }
                    self.insert_now(&text);
                }
            }
            LastChange::AppendLineEnd { text } => {
                for _ in 0..count.max(1) {
                    if let Some(line) = self.editor.index().offset_to_line(self.cursor) {
                        self.cursor = line.end;
                    }
                    self.insert_now(&text);
                }
            }
            LastChange::OpenBelow { text } => {
                if let Some(line) = self.editor.index().offset_to_line(self.cursor) {
                    self.cursor = line.end;
                }
                self.insert_now(&format!("\n{text}"));
            }
            LastChange::OpenAbove { text } => {
                let start = self
                    .editor
                    .index()
                    .offset_to_line(self.cursor)
                    .map_or(0, |line| line.start);
                self.cursor = start;
                self.insert_now(&format!("{text}\n"));
            }
        }
    }

    fn replace_now(&mut self, range: std::ops::Range<usize>, text: &str) {
        self.editor.set_selection_bytes(range.start, range.end);
        self.editor.delete(range.clone());
        if !text.is_empty() {
            self.editor.insert(range.start, text);
        }
        self.cursor = range.start + text.len();
        self.editor.set_selection_bytes(self.cursor, self.cursor);
        self.modified = true;
        self.dirty = true;
    }

    fn insert_now(&mut self, text: &str) {
        self.editor.set_selection_bytes(self.cursor, self.cursor);
        self.editor.insert(self.cursor, text);
        self.cursor += text.len();
        self.modified = true;
        self.dirty = true;
    }
}

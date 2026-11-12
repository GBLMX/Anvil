//! Bounded matching-bracket search.

use super::*;
use std::path::{Path, PathBuf};

pub(crate) const MAX_BRACKET_SCAN_BYTES: usize = 256 * 1024;

#[derive(Default)]
pub(crate) struct BracketCache {
    key: Option<(u64, usize, Option<PathBuf>)>,
    pair: Option<(usize, usize)>,
}

impl BracketCache {
    pub(crate) fn key_matches(&self, revision: u64, path: Option<&Path>, cursor: usize) -> bool {
        self.key
            .as_ref()
            .is_some_and(|key| key.0 == revision && key.1 == cursor && key.2.as_deref() == path)
    }

    pub(crate) fn cached(&self) -> Option<(usize, usize)> {
        self.pair
    }

    pub(crate) fn set_none(&mut self, revision: u64, path: Option<&Path>, cursor: usize) {
        self.key = Some((revision, cursor, path.map(Path::to_path_buf)));
        self.pair = None;
    }

    pub(crate) fn pair(
        &mut self,
        text: &str,
        revision: u64,
        path: Option<&Path>,
        cursor: usize,
    ) -> Option<(usize, usize)> {
        let key = (revision, cursor, path.map(Path::to_path_buf));
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.pair = matching_bracket(text, cursor, path);
        }
        self.pair
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Forward,
    Backward,
}

struct Lexer<'a> {
    text: &'a str,
    line_comment: Option<&'static str>,
    block_comment: Option<(&'static str, &'static str)>,
    single_quote: bool,
}

impl<'a> Lexer<'a> {
    fn new(text: &'a str, path: Option<&Path>) -> Self {
        let line_comment = path
            .and_then(crate::comment::comment_style)
            .map(|style| style.line);
        let extension = path
            .and_then(Path::extension)
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let block_comment =
            matches!(extension.as_str(), "rs" | "go" | "jsonc").then_some(("/*", "*/"));
        let single_quote = !matches!(extension.as_str(), "rs" | "go");
        Self {
            text,
            line_comment,
            block_comment,
            single_quote,
        }
    }

    /// Returns the next character offset and whether it is bracket-significant.
    fn next(&self, at: usize) -> Option<(usize, char, bool)> {
        let character = self.text.get(at..)?.chars().next()?;
        let next = at + character.len_utf8();
        if let Some(marker) = self.line_comment
            && self.text[at..].starts_with(marker)
        {
            let end = self.text[at..]
                .find('\n')
                .map_or(self.text.len(), |offset| at + offset);
            return Some((end, '\0', false));
        }
        if let Some((open, close)) = self.block_comment
            && self.text[at..].starts_with(open)
        {
            let end = self.text[at + open.len()..]
                .find(close)
                .map_or(self.text.len(), |offset| {
                    at + open.len() + offset + close.len()
                });
            return Some((end, '\0', false));
        }
        if matches!(character, '"' | '`') || (self.single_quote && character == '\'') {
            let mut index = next;
            let mut escaped = false;
            while let Some(next_character) = self.text.get(index..)?.chars().next() {
                index += next_character.len_utf8();
                if next_character == '\n' && !escaped {
                    break;
                }
                if next_character == character && !escaped {
                    break;
                }
                escaped = !escaped && next_character == '\\';
            }
            return Some((index, '\0', false));
        }
        Some((next, character, is_bracket(character)))
    }
}

pub(crate) fn is_bracket(character: char) -> bool {
    matches!(character, '(' | ')' | '[' | ']' | '{' | '}')
}

fn bracket_pair(character: char) -> Option<(char, char)> {
    Some(match character {
        '(' => ('(', ')'),
        ')' => ('(', ')'),
        '[' => ('[', ']'),
        ']' => ('[', ']'),
        '{' => ('{', '}'),
        '}' => ('{', '}'),
        _ => return None,
    })
}

fn bracket_at(text: &str, cursor: usize) -> Option<(usize, char)> {
    if let Some(character) = text.get(cursor..)?.chars().next()
        && is_bracket(character)
    {
        return Some((cursor, character));
    }
    let (offset, character) = text.get(..cursor)?.char_indices().next_back()?;
    is_bracket(character).then_some((offset, character))
}

/// Finds the matching bracket for the bracket at or immediately before `cursor`.
pub(crate) fn matching_bracket(
    text: &str,
    cursor: usize,
    path: Option<&Path>,
) -> Option<(usize, usize)> {
    let (target, character) = bracket_at(text, cursor)?;
    let (open, close) = bracket_pair(character)?;
    let line_start = text[..target].rfind('\n').map_or(0, |offset| offset + 1);
    let context_start = if target <= MAX_BRACKET_SCAN_BYTES {
        0
    } else {
        line_start
    };
    let context_lexer = Lexer::new(text, path);
    let mut context_at = context_start;
    let mut target_significant = None;
    while context_at <= target {
        let Some((next, _, significant)) = context_lexer.next(context_at) else {
            break;
        };
        if context_at == target {
            target_significant = Some(significant);
            break;
        }
        if next > target {
            break;
        }
        context_at = next.max(context_at + 1);
    }
    if target_significant != Some(true) {
        return None;
    }
    let open_at_target = character == open;
    let direction = if open_at_target {
        Direction::Forward
    } else {
        Direction::Backward
    };
    let start = match direction {
        Direction::Forward => target,
        Direction::Backward => target.saturating_sub(MAX_BRACKET_SCAN_BYTES),
    };
    let end = match direction {
        Direction::Forward => (target + MAX_BRACKET_SCAN_BYTES).min(text.len()),
        Direction::Backward => target.saturating_add(close.len_utf8()),
    };
    let lexer = Lexer::new(text, path);
    let mut stack = Vec::new();
    let mut at = floor_char_boundary(text, start);
    while at < end {
        let Some((next, character, significant)) = lexer.next(at) else {
            break;
        };
        if significant {
            if character == open {
                stack.push(at);
            } else if character == close
                && let Some(open_at) = stack.pop()
                && ((open_at_target && open_at == target) || (!open_at_target && at == target))
            {
                return Some((open_at.min(at), open_at.max(at)));
            }
        }
        at = next.max(at + 1);
    }
    None
}

fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

impl Model {
    pub(crate) fn bracket_pair(&self) -> Option<(usize, usize)> {
        let revision = self.session.editor.revision();
        let path = self.session.path.as_deref();
        let cursor = self.session.cursor;
        if self
            .bracket_cache
            .borrow()
            .key_matches(revision, path, cursor)
        {
            return self.bracket_cache.borrow().cached();
        }
        if !self.cursor_is_on_bracket() {
            self.bracket_cache
                .borrow_mut()
                .set_none(revision, path, cursor);
            return None;
        }
        let text = self
            .session
            .editor
            .text()
            .slice(0..self.session.editor.text().len_bytes());
        self.bracket_cache
            .borrow_mut()
            .pair(&text, revision, path, cursor)
    }

    fn cursor_is_on_bracket(&self) -> bool {
        let text = self.session.editor.text();
        let length = text.len_bytes();
        let cursor = self.session.cursor;
        if text
            .slice(cursor..cursor.saturating_add(4).min(length))
            .chars()
            .next()
            .is_some_and(bracket::is_bracket)
        {
            return true;
        }
        let Some(previous) = self.session.editor.previous_grapheme_boundary(cursor) else {
            return false;
        };
        text.slice(previous..cursor)
            .chars()
            .next()
            .is_some_and(bracket::is_bracket)
    }

    pub(crate) fn match_bracket(&mut self) {
        let Some((open, close)) = self.bracket_pair() else {
            self.status.text = "no matching bracket".to_owned();
            self.dirty = true;
            return;
        };
        self.record_jump();
        self.session.cursor = if self.session.cursor <= open {
            close
        } else {
            open
        };
        self.session
            .editor
            .set_selection_bytes(self.session.cursor, self.session.cursor);
        self.status.text = "matched bracket".to_owned();
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_nested_brackets_across_lines() {
        let text = "fn main() {\n  call((1 + 2));\n}\n";
        let pair = matching_bracket(text, 18, Some(Path::new("main.rs"))).expect("pair");
        assert_eq!(pair, (18, 26));
        let inner = matching_bracket(text, 19, Some(Path::new("main.rs"))).expect("pair");
        assert_eq!(inner, (19, 25));
    }

    #[test]
    fn ignores_brackets_in_strings_and_comments() {
        let text = "\"(a)\" // (b)\n(c)\n";
        assert_eq!(matching_bracket(text, 1, Some(Path::new("main.rs"))), None);
        assert_eq!(matching_bracket(text, 8, Some(Path::new("main.rs"))), None);
        let pair = matching_bracket(text, 13, Some(Path::new("main.rs"))).expect("pair");
        assert_eq!(pair, (13, 15));
        let after_string =
            matching_bracket("\"(a)\" (b)", 6, Some(Path::new("main.rs"))).expect("after string");
        assert_eq!(after_string, (6, 8));
    }

    #[test]
    fn bracket_adjacent_to_cursor_is_used() {
        let text = "(a)";
        assert_eq!(
            matching_bracket(text, 1, Some(Path::new("main.rs"))),
            Some((0, 2))
        );
    }

    #[test]
    fn unmatched_bracket_returns_none() {
        assert_eq!(matching_bracket("(a", 0, Some(Path::new("main.rs"))), None);
    }
}

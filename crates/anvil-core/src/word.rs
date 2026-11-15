use crate::{TextBuffer, TextDocument};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Punch,
}

fn class(character: char) -> Class {
    if character.is_whitespace() {
        Class::Space
    } else if character.is_alphanumeric() || character == '_' {
        Class::Word
    } else {
        Class::Punch
    }
}

fn forward_chars(text: &TextDocument, at: usize) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut base = at;
    text.chunks(at..text.len_bytes()).flat_map(move |chunk| {
        let start = base;
        base += chunk.len();
        chunk
            .char_indices()
            .map(move |(offset, character)| (start + offset, character))
    })
}

fn reverse_chars(text: &TextDocument, at: usize) -> impl Iterator<Item = (usize, char)> + '_ {
    let chunks: Vec<&str> = text.chunks(0..at).collect();
    let mut end = at;
    chunks.into_iter().rev().flat_map(move |chunk| {
        let chunk_end = end;
        let chunk_len = chunk.len();
        end -= chunk_len;
        chunk
            .char_indices()
            .rev()
            .map(move |(offset, character)| (chunk_end - chunk_len + offset, character))
    })
}

/// Start of the next vi word after `at`.
///
/// A word is a run of alphanumeric/underscore characters, a run of punctuation,
/// or a run of whitespace. `at` must be a UTF-8 boundary.
pub fn next_word_start(text: &TextDocument, at: usize) -> usize {
    let len = text.len_bytes();
    let at = at.min(len);
    let mut chars = forward_chars(text, at).peekable();
    let Some((_, first)) = chars.next() else {
        return len;
    };
    let mut current = class(first);
    if current == Class::Space {
        while chars
            .peek()
            .is_some_and(|(_, character)| class(*character) == Class::Space)
        {
            chars.next();
        }
        let Some((_, character)) = chars.peek() else {
            return len;
        };
        current = class(*character);
    }
    while chars
        .peek()
        .is_some_and(|(_, character)| class(*character) == current)
    {
        chars.next();
    }
    while chars
        .peek()
        .is_some_and(|(_, character)| class(*character) == Class::Space)
    {
        chars.next();
    }
    chars.peek().map_or(len, |(offset, _)| *offset)
}

/// Start of the previous vi word before `at`.
pub fn previous_word_start(text: &TextDocument, at: usize) -> usize {
    let at = at.min(text.len_bytes());
    let mut chars = reverse_chars(text, at).peekable();
    while chars
        .peek()
        .is_some_and(|(_, character)| class(*character) == Class::Space)
    {
        chars.next();
    }
    let Some((mut start, first)) = chars.next() else {
        return 0;
    };
    let current = class(first);
    while let Some((offset, character)) = chars.peek()
        && class(*character) == current
    {
        start = *offset;
        chars.next();
    }
    start
}

/// Byte offset of the last character of the current/next vi word.
pub fn end_word(text: &TextDocument, at: usize) -> usize {
    let len = text.len_bytes();
    let at = at.min(len);
    let mut chars = forward_chars(text, at).peekable();
    while chars
        .peek()
        .is_some_and(|(_, character)| class(*character) == Class::Space)
    {
        chars.next();
    }
    let Some((first_offset, first)) = chars.next() else {
        return len;
    };
    let current = class(first);
    let mut last = first_offset;
    while let Some((offset, character)) = chars.peek()
        && class(*character) == current
    {
        last = *offset;
        chars.next();
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> TextDocument {
        TextDocument::from_text(value)
    }

    #[test]
    fn words_skip_whitespace_and_punctuation() {
        let text = text("foo.bar baz");
        assert_eq!(next_word_start(&text, 0), 3);
        assert_eq!(next_word_start(&text, 3), 4);
        assert_eq!(next_word_start(&text, 4), 8);
        assert_eq!(next_word_start(&text, 8), 11);
    }

    #[test]
    fn previous_word_stops_at_word_start() {
        let text = text("foo bar.baz");
        assert_eq!(previous_word_start(&text, 11), 8);
        assert_eq!(previous_word_start(&text, 8), 7);
        assert_eq!(previous_word_start(&text, 7), 4);
        assert_eq!(previous_word_start(&text, 4), 0);
    }

    #[test]
    fn end_word_returns_last_character() {
        let text = text("foo bar");
        assert_eq!(end_word(&text, 0), 2);
        assert_eq!(end_word(&text, 4), 6);
        assert_eq!(end_word(&text, 2), 2);
    }

    #[test]
    fn empty_text_is_stable() {
        let text = text("");
        assert_eq!(next_word_start(&text, 0), 0);
        assert_eq!(previous_word_start(&text, 0), 0);
        assert_eq!(end_word(&text, 0), 0);
    }

    #[test]
    fn word_motion_handles_unicode_and_bounds() {
        let text = text("αβ.. γ");
        assert_eq!(next_word_start(&text, 0), 4);
        assert_eq!(next_word_start(&text, 4), 7);
        assert_eq!(previous_word_start(&text, 9), 7);
        assert_eq!(previous_word_start(&text, 7), 4);
        assert_eq!(end_word(&text, 0), 2);
        assert_eq!(end_word(&text, 4), 5);
        assert_eq!(next_word_start(&text, 99), text.len_bytes());
        assert_eq!(end_word(&text, 99), text.len_bytes());
    }
}

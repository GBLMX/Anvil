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

fn char_at(text: &TextDocument, at: usize) -> Option<(usize, char)> {
    if at >= text.len_bytes() {
        return None;
    }
    let chunk = text.chunks(at..text.len_bytes()).next()?;
    chunk.chars().next().map(|character| (at, character))
}

fn previous_char(text: &TextDocument, at: usize) -> Option<(usize, char)> {
    if at == 0 {
        return None;
    }
    let mut last = None;
    for chunk in text.chunks(0..at) {
        if let Some(character) = chunk.chars().last() {
            last = Some(character);
        }
    }
    let character = last?;
    Some((at - character.len_utf8(), character))
}

fn class_at(text: &TextDocument, at: usize) -> Option<Class> {
    char_at(text, at).map(|(_, character)| class(character))
}

/// Start of the next vi word after `at`.
///
/// A word is a run of alphanumeric/underscore characters, a run of punctuation,
/// or a run of whitespace. `at` must be a UTF-8 boundary.
pub fn next_word_start(text: &TextDocument, at: usize) -> usize {
    let mut position = at;
    let Some(mut current) = class_at(text, position) else {
        return text.len_bytes();
    };
    if current == Class::Space {
        while let Some((next, _)) = char_at(text, position) {
            match class_at(text, next) {
                Some(Class::Space) => position += char_at(text, position).unwrap().1.len_utf8(),
                _ => break,
            }
        }
        current = match class_at(text, position) {
            Some(class) => class,
            None => return text.len_bytes(),
        };
    }
    while let Some((next, character)) = char_at(text, position) {
        if class(character) != current {
            break;
        }
        position = next + character.len_utf8();
    }
    while let Some((next, character)) = char_at(text, position) {
        if class(character) != Class::Space {
            break;
        }
        position = next + character.len_utf8();
    }
    position
}

/// Start of the previous vi word before `at`.
pub fn previous_word_start(text: &TextDocument, at: usize) -> usize {
    let mut position = at;
    while let Some((previous, character)) = previous_char(text, position) {
        if class(character) != Class::Space {
            position = previous;
            break;
        }
        position = previous;
    }
    let Some(current) = class_at(text, position) else {
        return position;
    };
    while let Some((previous, character)) = previous_char(text, position) {
        if class(character) != current {
            break;
        }
        position = previous;
    }
    position
}

/// Byte offset of the last character of the current/next vi word.
pub fn end_word(text: &TextDocument, at: usize) -> usize {
    let mut position = at;
    while let Some((_, character)) = char_at(text, position) {
        if class(character) != Class::Space {
            break;
        }
        position += character.len_utf8();
    }
    let Some(current) = class_at(text, position) else {
        return text.len_bytes();
    };
    loop {
        let Some((next, character)) = char_at(text, position) else {
            return position;
        };
        let next_position = next + character.len_utf8();
        match char_at(text, next_position) {
            Some((_, next_character)) if class(next_character) == current => {
                position = next_position
            }
            _ => return position,
        }
    }
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
}

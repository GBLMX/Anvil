//! Buffer editing, motions, registers, and undo operations.

mod input;
mod motions;
mod objects;
mod selection;

use super::*;

fn closing_pair(character: char) -> Option<char> {
    match character {
        ')' | ']' | '}' | '"' | '\'' | '`' => Some(character),
        _ => None,
    }
}

fn matching_close(character: char) -> Option<char> {
    match character {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        _ => None,
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

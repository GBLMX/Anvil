//! Search matching and incremental search state.

use regex::RegexBuilder;
use std::{
    collections::HashSet,
    ops::Range,
    time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SearchOptions {
    pub(crate) case_sensitive: bool,
    pub(crate) whole_word: bool,
    pub(crate) regex: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SearchState {
    pub(crate) options: SearchOptions,
    pub(crate) matches: Vec<Range<usize>>,
    pub(crate) current: usize,
    pub(crate) error: Option<String>,
    origin: Option<(usize, Vec<(usize, usize)>)>,
    pub(crate) due: Option<Instant>,
}

impl SearchState {
    pub(crate) fn begin(&mut self, cursor: usize, selections: Vec<(usize, usize)>) {
        self.origin = Some((cursor, selections));
        self.matches.clear();
        self.current = 0;
        self.error = None;
        self.due = None;
    }

    pub(crate) fn schedule(&mut self) {
        self.due = Some(Instant::now() + SEARCH_DEBOUNCE);
    }

    pub(crate) fn take_due(&mut self) -> bool {
        let Some(due) = self.due else {
            return false;
        };
        if Instant::now() < due {
            return false;
        }
        self.due = None;
        true
    }

    pub(crate) fn restore_origin(&mut self) -> Option<(usize, Vec<(usize, usize)>)> {
        self.due = None;
        self.error = None;
        self.matches.clear();
        self.origin.take()
    }

    pub(crate) fn reset_for_buffer(&mut self) {
        self.matches.clear();
        self.current = 0;
        self.error = None;
        self.due = None;
        self.origin = None;
    }

    pub(crate) fn selected(&self) -> Option<&Range<usize>> {
        self.matches.get(self.current)
    }
}

pub(crate) fn find_matches(
    text: &str,
    query: &str,
    options: SearchOptions,
) -> Result<Vec<Range<usize>>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let pattern = if options.regex {
        query.to_owned()
    } else {
        regex::escape(query)
    };
    let regex = RegexBuilder::new(&pattern)
        .case_insensitive(!options.case_sensitive)
        .build()
        .map_err(|error| error.to_string())?;
    let boundaries: HashSet<usize> = text
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .chain(std::iter::once(text.len()))
        .collect();
    let mut matches = Vec::new();
    for found in regex.find_iter(text) {
        if found.start() == found.end()
            || !boundaries.contains(&found.start())
            || !boundaries.contains(&found.end())
        {
            continue;
        }
        if options.whole_word
            && (text[..found.start()]
                .chars()
                .next_back()
                .is_some_and(is_word_char)
                || text[found.end()..].chars().next().is_some_and(is_word_char))
        {
            continue;
        }
        matches.push(found.start()..found.end());
    }
    Ok(matches)
}

fn is_word_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn literal() -> SearchOptions {
        SearchOptions::default()
    }

    #[test]
    fn empty_and_non_overlapping_literal_matches() {
        assert!(
            find_matches("aaaa", "", literal())
                .expect("empty")
                .is_empty()
        );
        assert_eq!(
            find_matches("aaaa", "aa", literal()).expect("matches"),
            vec![0..2, 2..4]
        );
    }

    #[test]
    fn invalid_regex_is_reported() {
        let options = SearchOptions {
            regex: true,
            ..literal()
        };
        assert!(find_matches("text", "[a-", options).is_err());
    }

    #[test]
    fn unicode_and_options_are_respected() {
        assert_eq!(
            find_matches("你好 你好", "你好", literal()).expect("unicode"),
            vec![0..6, 7..13]
        );
        let whole = SearchOptions {
            whole_word: true,
            ..literal()
        };
        assert_eq!(
            find_matches("cat scatter cat", "cat", whole).expect("whole"),
            vec![0..3, 12..15]
        );
        let case = SearchOptions {
            case_sensitive: true,
            ..literal()
        };
        assert!(find_matches("Cat", "cat", case).expect("case").is_empty());
    }
}

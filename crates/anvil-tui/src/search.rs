//! Search matching and incremental search state.

use regex::RegexBuilder;
use std::{
    ops::Range,
    time::{Duration, Instant},
};
use unicode_segmentation::GraphemeCursor;

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(50);
pub(crate) const MAX_SEARCH_MATCHES: usize = 10_000;

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
    pub(crate) truncated: bool,
    origin: Option<(usize, Vec<(usize, usize)>)>,
    pub(crate) due: Option<Instant>,
}

impl SearchState {
    pub(crate) fn begin(&mut self, cursor: usize, selections: Vec<(usize, usize)>) {
        self.origin = Some((cursor, selections));
        self.matches.clear();
        self.current = 0;
        self.error = None;
        self.truncated = false;
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
        self.truncated = false;
        self.matches.clear();
        self.origin.take()
    }

    pub(crate) fn reset_for_buffer(&mut self) {
        self.matches.clear();
        self.current = 0;
        self.error = None;
        self.truncated = false;
        self.due = None;
        self.origin = None;
    }

    pub(crate) fn selected(&self) -> Option<&Range<usize>> {
        self.matches.get(self.current)
    }
}

#[cfg(test)]
pub(crate) fn find_matches(
    text: &str,
    query: &str,
    options: SearchOptions,
) -> Result<Vec<Range<usize>>, String> {
    find_matches_limited(text, query, options, usize::MAX).map(|(matches, _)| matches)
}

pub(crate) fn find_matches_limited(
    text: &str,
    query: &str,
    options: SearchOptions,
    limit: usize,
) -> Result<(Vec<Range<usize>>, bool), String> {
    if query.is_empty() {
        return Ok((Vec::new(), false));
    }
    if limit == 0 {
        return Ok((Vec::new(), true));
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
    let mut matches = Vec::new();
    let mut truncated = false;
    for found in regex.find_iter(text) {
        if found.start() == found.end()
            || !is_grapheme_boundary(text, found.start())
            || !is_grapheme_boundary(text, found.end())
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
        if matches.len() >= limit {
            truncated = true;
            break;
        }
        matches.push(found.start()..found.end());
    }
    Ok((matches, truncated))
}

fn is_word_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn is_grapheme_boundary(text: &str, offset: usize) -> bool {
    if offset == 0 || offset == text.len() {
        return true;
    }
    if !text.is_char_boundary(offset) {
        return false;
    }
    let mut cursor = GraphemeCursor::new(offset, text.len(), true);
    matches!(cursor.is_boundary(text, 0), Ok(true))
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

    #[test]
    fn matches_do_not_split_grapheme_clusters() {
        assert!(
            find_matches("e\u{301}", "e", literal())
                .expect("combining")
                .is_empty()
        );
        assert_eq!(
            find_matches("e\u{301} e", "e", literal()).expect("after"),
            vec![4..5]
        );
    }

    #[test]
    fn dense_matches_are_truncated() {
        for megabytes in [1, 10] {
            let text = "a".repeat(megabytes * 1024 * 1024);
            let (matches, truncated) =
                find_matches_limited(&text, "a", literal(), MAX_SEARCH_MATCHES).expect("matches");
            assert_eq!(matches.len(), MAX_SEARCH_MATCHES);
            assert!(truncated);
        }
    }

    #[test]
    #[ignore = "manual dense search boundary"]
    fn dense_100mb_search_is_bounded() {
        let text = "a".repeat(100 * 1024 * 1024);
        let start = std::time::Instant::now();
        let (matches, truncated) =
            find_matches_limited(&text, "a", literal(), MAX_SEARCH_MATCHES).expect("matches");
        println!("dense-100MB-search-ms={}", start.elapsed().as_millis());
        assert_eq!(matches.len(), MAX_SEARCH_MATCHES);
        assert!(truncated);
    }
}

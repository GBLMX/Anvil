//! Bounded file-picker query parsing and matching.

use std::ops::Range;

pub(crate) const FILE_PICKER_RESULT_LIMIT: usize = 1000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FileQuery {
    terms: Vec<QueryTerm>,
    case_sensitive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct QueryTerm {
    text: String,
    kind: TermKind,
    negated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TermKind {
    Fuzzy,
    Exact,
    Prefix,
    Suffix,
}

impl FileQuery {
    pub(crate) fn parse(raw: &str) -> Self {
        let case_sensitive = raw.chars().any(char::is_uppercase);
        let terms = raw.split_whitespace().filter_map(parse_term).collect();
        Self {
            terms,
            case_sensitive,
        }
    }

    pub(crate) fn matches(&self, path: &str) -> Option<FileMatch> {
        let mut ranges = Vec::new();
        for term in &self.terms {
            let matched = match_term(path, term, self.case_sensitive);
            if term.negated {
                if matched.is_some() {
                    return None;
                }
            } else {
                let mut matched = matched?;
                ranges.append(&mut matched);
            }
        }
        let ranges = merge_ranges(ranges);
        let basename_start = path.rfind('/').map(|index| index + 1).unwrap_or(0);
        let basename_match = ranges.iter().any(|range| range.end > basename_start);
        let basename_prefix = ranges.iter().any(|range| range.start == basename_start);
        let first_position = ranges
            .first()
            .map(|range| range.start)
            .unwrap_or(path.len());
        Some(FileMatch {
            ranges,
            basename_match,
            basename_prefix,
            first_position,
        })
    }

    pub(crate) fn highlight_ranges(&self, path: &str) -> Vec<(usize, usize)> {
        self.matches(path)
            .map(|matched| {
                matched
                    .ranges
                    .into_iter()
                    .map(|range| (range.start, range.end))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileMatch {
    pub(crate) ranges: Vec<Range<usize>>,
    pub(crate) basename_match: bool,
    pub(crate) basename_prefix: bool,
    pub(crate) first_position: usize,
}

fn parse_term(token: &str) -> Option<QueryTerm> {
    let (negated, token) = match token.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, token),
    };
    let (kind, token) = if let Some(rest) = token.strip_prefix('\'') {
        (TermKind::Exact, rest)
    } else if let Some(rest) = token.strip_prefix('^') {
        (TermKind::Prefix, rest)
    } else {
        (TermKind::Fuzzy, token)
    };
    let (kind, token) = if let Some(rest) = token.strip_suffix('$') {
        (TermKind::Suffix, rest)
    } else {
        (kind, token)
    };
    if token.is_empty() {
        return None;
    }
    Some(QueryTerm {
        text: token.to_owned(),
        kind,
        negated,
    })
}

fn match_term(path: &str, term: &QueryTerm, case_sensitive: bool) -> Option<Vec<Range<usize>>> {
    match term.kind {
        TermKind::Fuzzy => fuzzy_ranges(path, &term.text, case_sensitive),
        TermKind::Exact => {
            find_contiguous(path, &term.text, case_sensitive, false, false).map(|range| vec![range])
        }
        TermKind::Prefix => {
            find_contiguous(path, &term.text, case_sensitive, true, false).map(|range| vec![range])
        }
        TermKind::Suffix => {
            find_contiguous(path, &term.text, case_sensitive, false, true).map(|range| vec![range])
        }
    }
}

fn fuzzy_ranges(path: &str, needle: &str, case_sensitive: bool) -> Option<Vec<Range<usize>>> {
    let mut wanted = needle.chars();
    let mut next = wanted.next()?;
    let mut ranges = Vec::new();
    for (start, character) in path.char_indices() {
        if characters_equal(character, next, case_sensitive) {
            ranges.push(start..start + character.len_utf8());
            match wanted.next() {
                Some(character) => next = character,
                None => return Some(ranges),
            }
        }
    }
    None
}

fn find_contiguous(
    path: &str,
    needle: &str,
    case_sensitive: bool,
    prefix: bool,
    suffix: bool,
) -> Option<Range<usize>> {
    let mut wanted = needle.chars();
    let first = wanted.next()?;
    for (start, character) in path.char_indices() {
        if prefix && start != 0 && path.as_bytes().get(start - 1) != Some(&b'/') {
            continue;
        }
        if !characters_equal(character, first, case_sensitive) {
            continue;
        }
        if let Some(end) = match_from(path, start, needle, case_sensitive)
            && (!suffix || end == path.len())
        {
            return Some(start..end);
        }
    }
    None
}

fn match_from(path: &str, start: usize, needle: &str, case_sensitive: bool) -> Option<usize> {
    let mut offset = start;
    for wanted in needle.chars() {
        let (byte, character) = path[offset..].char_indices().next()?;
        if !characters_equal(character, wanted, case_sensitive) {
            return None;
        }
        offset += byte + character.len_utf8();
    }
    Some(offset)
}

fn characters_equal(left: char, right: char, case_sensitive: bool) -> bool {
    if case_sensitive {
        left == right
    } else {
        lower(left) == lower(right)
    }
}

fn lower(character: char) -> char {
    character.to_lowercase().next().unwrap_or(character)
}

fn merge_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
            continue;
        }
        merged.push(range);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(query: &str, path: &str) -> Vec<(usize, usize)> {
        FileQuery::parse(query).highlight_ranges(path)
    }

    #[test]
    fn matches_across_path_segments_and_merges_ranges() {
        assert_eq!(ranges("abc", "a/b/c.rs"), vec![(0, 1), (2, 3), (4, 5)]);
        assert_eq!(ranges("main", "src/main.rs"), vec![(4, 8)]);
        assert!(ranges("abc", "src/main.rs").is_empty());
    }

    #[test]
    fn supports_and_terms_and_negation() {
        assert_eq!(ranges("src main", "src/main.rs"), vec![(0, 3), (4, 8)]);
        assert!(ranges("src main", "src/lib.rs").is_empty());
        assert_eq!(ranges("!test main", "src/main.rs"), vec![(4, 8)]);
        assert!(ranges("!test main", "tests/main.rs").is_empty());
    }

    #[test]
    fn supports_exact_prefix_and_suffix_terms() {
        assert_eq!(ranges("'main", "src/main.rs"), vec![(4, 8)]);
        assert!(ranges("'main", "src/m_a_i_n.rs").is_empty());
        assert_eq!(ranges("^src/", "src/main.rs"), vec![(0, 4)]);
        assert!(!ranges("^src/", "x/src/main.rs").is_empty());
        assert!(!ranges("^src/", "/tmp/project/src/main.rs").is_empty());
        assert_eq!(ranges("main.rs$", "src/main.rs"), vec![(4, 11)]);
        assert!(ranges("main.rs$", "src/main.rs.bak").is_empty());
    }

    #[test]
    fn smart_case_is_enabled_by_uppercase_queries() {
        assert_eq!(ranges("main", "src/Main.rs"), vec![(4, 8)]);
        assert!(ranges("MAIN", "src/main.rs").is_empty());
        assert_eq!(ranges("café", "CAFÉ.txt"), vec![(0, 5)]);
    }
}

//! LSP snippet parsing and tabstop navigation.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetStop {
    pub index: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub text: String,
    pub stops: Vec<SnippetStop>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetSession {
    stops: Vec<SnippetStop>,
    groups: Vec<usize>,
    current: usize,
}

impl Snippet {
    pub fn offset_stops(&self, base: usize) -> Vec<SnippetStop> {
        self.stops
            .iter()
            .map(|stop| SnippetStop {
                index: stop.index,
                start: base + stop.start,
                end: base + stop.end,
            })
            .collect()
    }
}

impl SnippetSession {
    pub fn new(mut stops: Vec<SnippetStop>) -> Option<Self> {
        if stops.is_empty() {
            return None;
        }
        stops.sort_by_key(|stop| (stop.index == 0, stop.index));
        let mut groups = Vec::new();
        for (position, stop) in stops.iter().enumerate() {
            if position == 0 || stop.index != stops[position - 1].index {
                groups.push(position);
            }
        }
        Some(Self {
            stops,
            groups,
            current: 0,
        })
    }

    fn current_group(&self) -> &[SnippetStop] {
        let start = self.groups[self.current];
        let end = self
            .groups
            .get(self.current + 1)
            .copied()
            .unwrap_or(self.stops.len());
        &self.stops[start..end]
    }

    pub fn current(&self) -> Option<(usize, usize)> {
        self.current_group()
            .first()
            .map(|stop| (stop.start, stop.end))
    }

    pub fn mirrors(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.current_group()
            .iter()
            .map(|stop| (stop.start, stop.end))
    }

    pub fn mirror_ranges(&self) -> Vec<(usize, usize)> {
        self.mirrors().collect()
    }

    pub fn matches_ranges(&self, ranges: &[(usize, usize)]) -> bool {
        let mut expected = self.mirror_ranges();
        expected.sort_unstable();
        let mut actual = ranges
            .iter()
            .map(|(anchor, head)| ((*anchor).min(*head), (*anchor).max(*head)))
            .collect::<Vec<_>>();
        actual.sort_unstable();
        expected == actual
    }

    pub fn replace_current_with_carets(&mut self, old: &[(usize, usize)], carets: &[usize]) {
        let start = self.groups[self.current];
        let end = self
            .groups
            .get(self.current + 1)
            .copied()
            .unwrap_or(self.stops.len());
        if old.len() != carets.len() || old.len() != end - start {
            return;
        }
        let mut replacements = old
            .iter()
            .zip(carets)
            .enumerate()
            .map(|(offset, ((old_start, old_end), caret))| {
                (*old_start, *old_end, *caret, start + offset)
            })
            .collect::<Vec<_>>();
        replacements.sort_by(|left, right| right.0.cmp(&left.0));
        for (_, old_end, caret, index) in replacements {
            let delta = i128::try_from(caret).unwrap_or(i128::MAX)
                - i128::try_from(old_end).unwrap_or(i128::MAX);
            for (position, stop) in self.stops.iter_mut().enumerate() {
                if position >= start && position < end {
                    continue;
                }
                if stop.start >= old_end {
                    stop.start = Self::shift_offset(stop.start, delta);
                    stop.end = Self::shift_offset(stop.end, delta);
                }
            }
            self.stops[index].start = caret;
            self.stops[index].end = caret;
        }
    }

    fn shift_offset(offset: usize, delta: i128) -> usize {
        if delta >= 0 {
            offset.saturating_add(usize::try_from(delta).unwrap_or(usize::MAX))
        } else {
            offset.saturating_sub(usize::try_from(-delta).unwrap_or(usize::MAX))
        }
    }
    pub fn next_stop(&mut self) -> Option<(usize, usize)> {
        if self.current + 1 >= self.groups.len() {
            return None;
        }
        self.current += 1;
        self.current()
    }

    pub fn previous(&mut self) -> Option<(usize, usize)> {
        if self.current == 0 {
            return None;
        }
        self.current -= 1;
        self.current()
    }

    pub fn adjust_insert(&mut self, at: usize, length: usize) {
        for stop in &mut self.stops {
            if stop.start >= at {
                stop.start = stop.start.saturating_add(length);
                stop.end = stop.end.saturating_add(length);
            } else if stop.end >= at {
                stop.end = stop.end.saturating_add(length);
            }
        }
    }

    pub fn adjust_delete(&mut self, start: usize, end: usize) {
        let length = end.saturating_sub(start);
        for stop in &mut self.stops {
            if stop.end <= start {
                continue;
            }
            if stop.start >= end {
                stop.start = stop.start.saturating_sub(length);
                stop.end = stop.end.saturating_sub(length);
            } else {
                stop.start = stop.start.min(start);
                stop.end = stop.end.min(start);
            }
        }
    }
}

pub fn parse(text: &str) -> Result<Snippet, String> {
    let mut output = String::with_capacity(text.len());
    let mut stops = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((_index, character)) = chars.next() {
        match character {
            '\\' => match chars.next() {
                Some((_, escaped @ ('$' | '}' | '\\'))) => output.push(escaped),
                Some((_, escaped)) => {
                    output.push('\\');
                    output.push(escaped);
                }
                None => output.push('\\'),
            },
            '$' => {
                let Some((next_index, next)) = chars.peek().copied() else {
                    output.push('$');
                    continue;
                };
                if next.is_ascii_digit() {
                    let mut number = String::new();
                    while let Some((_, digit)) = chars.peek().copied() {
                        if !digit.is_ascii_digit() {
                            break;
                        }
                        number.push(digit);
                        chars.next();
                    }
                    let index = number.parse::<usize>().map_err(|_| "invalid tabstop")?;
                    let at = output.len();
                    stops.push(SnippetStop {
                        index,
                        start: at,
                        end: at,
                    });
                    continue;
                }
                if next == '{' {
                    let body_start = next_index + next.len_utf8();
                    let Some(relative_end) = text[body_start..].find('}') else {
                        return Err("unterminated placeholder".to_owned());
                    };
                    let body_end = body_start + relative_end;
                    let body = &text[body_start..body_end];
                    let (number, default) = body
                        .split_once(':')
                        .map_or((body, ""), |(number, default)| (number, default));
                    let index = number
                        .parse::<usize>()
                        .map_err(|_| format!("unsupported snippet variable {number:?}"))?;
                    if default.contains('$') || default.contains('}') {
                        return Err("nested snippet placeholders are unsupported".to_owned());
                    }
                    let start = output.len();
                    output.push_str(default);
                    let end = output.len();
                    stops.push(SnippetStop { index, start, end });
                    for (position, _) in chars.by_ref() {
                        if position == body_end {
                            break;
                        }
                    }
                    continue;
                }
                output.push('$');
            }
            _ => output.push(character),
        }
    }
    let end = output.len();
    if !stops.iter().any(|stop| stop.index == 0) {
        stops.push(SnippetStop {
            index: 0,
            start: end,
            end,
        });
    }
    stops.sort_by_key(|stop| (stop.index == 0, stop.index));
    Ok(Snippet {
        text: output,
        stops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults_and_final_stop() {
        let snippet = parse("fn ${1:name}(${2:args}) $0").expect("snippet");
        assert_eq!(snippet.text, "fn name(args) ");
        assert_eq!(
            snippet.stops,
            vec![
                SnippetStop {
                    index: 1,
                    start: 3,
                    end: 7
                },
                SnippetStop {
                    index: 2,
                    start: 8,
                    end: 12
                },
                SnippetStop {
                    index: 0,
                    start: 14,
                    end: 14
                },
            ]
        );
    }

    #[test]
    fn supports_short_tabstops_and_escapes() {
        let snippet = parse("$1 \\$2 $0").expect("snippet");
        assert_eq!(snippet.text, " $2 ");
        assert_eq!(
            snippet.stops,
            vec![
                SnippetStop {
                    index: 1,
                    start: 0,
                    end: 0
                },
                SnippetStop {
                    index: 0,
                    start: 4,
                    end: 4
                },
            ]
        );
    }

    #[test]
    fn rejects_unsupported_variables_and_nesting() {
        assert!(parse("${TM_FILENAME}").is_err());
        assert!(parse("${1:${2:x}}").is_err());
        assert!(parse("$1 ${1:name}").is_ok());
    }

    #[test]
    fn parses_repeated_tabstops_as_mirrors() {
        let snippet = parse("${1:name} + ${1:name}").expect("snippet");
        assert_eq!(snippet.text, "name + name");
        assert_eq!(
            snippet.stops.iter().filter(|stop| stop.index == 1).count(),
            2
        );
    }

    #[test]
    fn session_groups_mirrors_and_skips_them_when_navigating() {
        let mut session = SnippetSession::new(vec![
            SnippetStop {
                index: 1,
                start: 3,
                end: 7,
            },
            SnippetStop {
                index: 1,
                start: 10,
                end: 14,
            },
            SnippetStop {
                index: 2,
                start: 20,
                end: 21,
            },
        ])
        .expect("session");
        assert_eq!(session.current(), Some((3, 7)));
        assert_eq!(
            session.mirrors().collect::<Vec<_>>(),
            vec![(3, 7), (10, 14)]
        );
        assert_eq!(session.next_stop(), Some((20, 21)));
        assert_eq!(session.next_stop(), None);
        assert_eq!(session.previous(), Some((3, 7)));
        session.adjust_insert(3, 1);
        assert_eq!(session.current(), Some((4, 8)));
        assert_eq!(
            session.mirrors().collect::<Vec<_>>(),
            vec![(4, 8), (11, 15)]
        );
    }

    #[test]
    fn session_navigates_and_tracks_insertions() {
        let mut session = SnippetSession::new(vec![
            SnippetStop {
                index: 1,
                start: 3,
                end: 4,
            },
            SnippetStop {
                index: 2,
                start: 8,
                end: 10,
            },
        ])
        .expect("session");
        assert_eq!(session.current(), Some((3, 4)));
        assert_eq!(session.next_stop(), Some((8, 10)));
        assert_eq!(session.next_stop(), None);
        assert_eq!(session.previous(), Some((3, 4)));
        session.adjust_insert(3, 2);
        assert_eq!(session.current(), Some((5, 6)));
    }
}

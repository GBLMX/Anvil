//! LSP snippet parsing and tabstop navigation.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnippetStop {
    pub(crate) index: usize,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Snippet {
    pub(crate) text: String,
    pub(crate) stops: Vec<SnippetStop>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnippetSession {
    stops: Vec<(usize, usize)>,
    current: usize,
}

impl Snippet {
    pub(crate) fn offset_stops(&self, base: usize) -> Vec<(usize, usize)> {
        self.stops
            .iter()
            .map(|stop| (base + stop.start, base + stop.end))
            .collect()
    }
}

impl SnippetSession {
    pub(crate) fn new(stops: Vec<(usize, usize)>) -> Option<Self> {
        (!stops.is_empty()).then_some(Self { stops, current: 0 })
    }

    pub(crate) fn current(&self) -> Option<(usize, usize)> {
        self.stops.get(self.current).copied()
    }

    pub(crate) fn next(&mut self) -> Option<(usize, usize)> {
        if self.current + 1 >= self.stops.len() {
            return None;
        }
        self.current += 1;
        self.current()
    }

    pub(crate) fn previous(&mut self) -> Option<(usize, usize)> {
        if self.current == 0 {
            return None;
        }
        self.current -= 1;
        self.current()
    }

    pub(crate) fn adjust_insert(&mut self, at: usize, length: usize) {
        for (start, end) in &mut self.stops {
            if *start >= at {
                *start = start.saturating_add(length);
                *end = end.saturating_add(length);
            } else if *end >= at {
                *end = end.saturating_add(length);
            }
        }
    }

    pub(crate) fn adjust_delete(&mut self, start: usize, end: usize) {
        let length = end.saturating_sub(start);
        for (stop_start, stop_end) in &mut self.stops {
            if *stop_end <= start {
                continue;
            }
            if *stop_start >= end {
                *stop_start = stop_start.saturating_sub(length);
                *stop_end = stop_end.saturating_sub(length);
            } else {
                *stop_start = (*stop_start).min(start);
                *stop_end = (*stop_end).min(start);
            }
        }
    }
}

pub(crate) fn parse(text: &str) -> Result<Snippet, String> {
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
    stops.dedup_by_key(|stop| stop.index);
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
    fn session_navigates_and_tracks_insertions() {
        let mut session = SnippetSession::new(vec![(3, 4), (8, 10)]).expect("session");
        assert_eq!(session.current(), Some((3, 4)));
        assert_eq!(session.next(), Some((8, 10)));
        assert_eq!(session.next(), None);
        assert_eq!(session.previous(), Some((3, 4)));
        session.adjust_insert(3, 2);
        assert_eq!(session.current(), Some((5, 6)));
    }
}

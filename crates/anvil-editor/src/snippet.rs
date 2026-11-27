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
        replacements.sort_by_key(|replacement| std::cmp::Reverse(replacement.0));
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
    let mut parser = Parser {
        chars: text.chars().collect(),
        position: 0,
        output: String::with_capacity(text.len()),
        stops: Vec::new(),
    };
    parser.sequence(None)?;
    let end = parser.output.len();
    if !parser.stops.iter().any(|stop| stop.index == 0) {
        parser.stops.push(SnippetStop {
            index: 0,
            start: end,
            end,
        });
    }
    let mut stops = parser.stops;
    stops.sort_by_key(|stop| (stop.index == 0, stop.index));
    Ok(Snippet {
        text: parser.output,
        stops,
    })
}

struct Parser {
    chars: Vec<char>,
    position: usize,
    output: String,
    stops: Vec<SnippetStop>,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn next(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.position += 1;
        Some(character)
    }

    fn sequence(&mut self, terminator: Option<char>) -> Result<(), String> {
        while let Some(character) = self.peek() {
            if Some(character) == terminator {
                self.position += 1;
                return Ok(());
            }
            self.position += 1;
            match character {
                '\\' => match self.next() {
                    Some(escaped @ ('$' | '}' | '\\')) => self.output.push(escaped),
                    Some(escaped) => {
                        self.output.push('\\');
                        self.output.push(escaped);
                    }
                    None => self.output.push('\\'),
                },
                '$' => self.dollar()?,
                _ => self.output.push(character),
            }
        }
        if terminator.is_some() {
            Err("unterminated placeholder".to_owned())
        } else {
            Ok(())
        }
    }

    fn dollar(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(character) if character.is_ascii_digit() => {
                let index = self.number()?;
                let at = self.output.len();
                self.stops.push(SnippetStop {
                    index,
                    start: at,
                    end: at,
                });
                Ok(())
            }
            Some('{') => {
                self.position += 1;
                self.braced()
            }
            Some(character) if is_variable_start(character) => {
                let name = self.identifier();
                self.output.push_str(&name);
                Ok(())
            }
            _ => {
                self.output.push('$');
                Ok(())
            }
        }
    }

    fn braced(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(character) if character.is_ascii_digit() => {
                let index = self.number()?;
                match self.peek() {
                    Some('}') => {
                        self.position += 1;
                        let at = self.output.len();
                        self.stops.push(SnippetStop {
                            index,
                            start: at,
                            end: at,
                        });
                        Ok(())
                    }
                    Some(':') => {
                        self.position += 1;
                        let start = self.output.len();
                        self.sequence(Some('}'))?;
                        let end = self.output.len();
                        self.stops.push(SnippetStop { index, start, end });
                        Ok(())
                    }
                    Some('|') => {
                        self.position += 1;
                        self.choice(index)
                    }
                    _ => Err("invalid placeholder".to_owned()),
                }
            }
            Some(character) if is_variable_start(character) => {
                let name = self.identifier();
                match self.peek() {
                    Some('}') => {
                        self.position += 1;
                        self.output.push_str(&name);
                        Ok(())
                    }
                    Some(':') => {
                        self.position += 1;
                        self.sequence(Some('}'))
                    }
                    Some('/') => {
                        self.skip_to_closing_brace()?;
                        self.output.push_str(&name);
                        Ok(())
                    }
                    _ => Err("invalid snippet variable".to_owned()),
                }
            }
            _ => Err("invalid snippet variable".to_owned()),
        }
    }

    fn choice(&mut self, index: usize) -> Result<(), String> {
        let start = self.output.len();
        loop {
            match self.next() {
                Some('\\') => match self.next() {
                    Some(escaped @ (',' | '|' | '}' | '\\' | '$')) => self.output.push(escaped),
                    Some(escaped) => {
                        self.output.push('\\');
                        self.output.push(escaped);
                    }
                    None => return Err("unterminated choice".to_owned()),
                },
                Some(',') => {
                    let end = self.output.len();
                    self.stops.push(SnippetStop { index, start, end });
                    loop {
                        match self.next() {
                            Some('|') => break,
                            Some('\\') => {
                                self.next();
                            }
                            Some(_) => {}
                            None => return Err("unterminated choice".to_owned()),
                        }
                    }
                    return match self.next() {
                        Some('}') => Ok(()),
                        _ => Err("unterminated choice".to_owned()),
                    };
                }
                Some('|') => {
                    let end = self.output.len();
                    self.stops.push(SnippetStop { index, start, end });
                    return match self.next() {
                        Some('}') => Ok(()),
                        _ => Err("unterminated choice".to_owned()),
                    };
                }
                Some('}') => {
                    let end = self.output.len();
                    self.stops.push(SnippetStop { index, start, end });
                    return Ok(());
                }
                Some(character) => self.output.push(character),
                None => return Err("unterminated choice".to_owned()),
            }
        }
    }

    fn skip_to_closing_brace(&mut self) -> Result<(), String> {
        let mut escaped = false;
        while let Some(character) = self.next() {
            if escaped {
                escaped = false;
                continue;
            }
            match character {
                '\\' => escaped = true,
                '}' => return Ok(()),
                _ => {}
            }
        }
        Err("unterminated snippet variable".to_owned())
    }

    fn number(&mut self) -> Result<usize, String> {
        let mut number = String::new();
        while let Some(character) = self.peek() {
            if !character.is_ascii_digit() {
                break;
            }
            number.push(character);
            self.position += 1;
        }
        number.parse().map_err(|_| "invalid tabstop".to_owned())
    }

    fn identifier(&mut self) -> String {
        let mut name = String::new();
        while let Some(character) = self.peek() {
            if character.is_alphanumeric() || character == '_' {
                name.push(character);
                self.position += 1;
            } else {
                break;
            }
        }
        name
    }
}

fn is_variable_start(character: char) -> bool {
    character.is_alphabetic() || character == '_'
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
    fn parses_choices_nested_placeholders_and_variables() {
        let choice = parse("${1|one,two|}").expect("choice");
        assert_eq!(choice.text, "one");
        assert_eq!(
            choice.stops,
            vec![
                SnippetStop {
                    index: 1,
                    start: 0,
                    end: 3
                },
                SnippetStop {
                    index: 0,
                    start: 3,
                    end: 3
                },
            ]
        );

        let nested = parse("${1:${2:x}}").expect("nested");
        assert_eq!(nested.text, "x");
        assert_eq!(
            nested.stops,
            vec![
                SnippetStop {
                    index: 1,
                    start: 0,
                    end: 1
                },
                SnippetStop {
                    index: 2,
                    start: 0,
                    end: 1
                },
                SnippetStop {
                    index: 0,
                    start: 1,
                    end: 1
                },
            ]
        );

        let variable = parse("${TM_FILENAME:name.txt}").expect("variable");
        assert_eq!(variable.text, "name.txt");
        assert!(parse("${TM_FILENAME}").is_ok());
        assert!(parse("$TM_FILENAME").is_ok());
        assert!(parse("${TM_FILENAME/(.*)\\..+$/$1/}").is_ok());
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

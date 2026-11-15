//! Parse compiler/search locations from piped text.

use crate::PipeLocation;
use std::collections::HashSet;

/// Maximum retained locations.
pub const MAX_LOCATIONS: usize = 10_000;
/// Maximum input lines inspected.
pub const MAX_PARSED_LINES: usize = 100_000;

/// Parse supported `path:line[:column][: message]` and `path(line,column)` forms.
pub fn parse(text: &str) -> Vec<PipeLocation> {
    let mut locations = Vec::new();
    let mut seen = HashSet::new();
    for raw in text.lines().take(MAX_PARSED_LINES) {
        if locations.len() >= MAX_LOCATIONS {
            break;
        }
        let line = strip_ansi_csi(raw);
        let Some(location) = parse_line(&line) else {
            continue;
        };
        if seen.insert((location.path.clone(), location.line, location.column)) {
            locations.push(location);
        }
    }
    locations
}

fn parse_line(line: &str) -> Option<PipeLocation> {
    let line = line.trim_start();
    let parsed = if let Some(rest) = line.strip_prefix("-->") {
        parse_colon(rest.trim_start())
    } else {
        parse_colon(line).or_else(|| parse_parenthesized(line))
    };
    parsed.filter(|location| plausible_path(&location.path))
}

fn plausible_path(path: &str) -> bool {
    !(path.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && !path.contains('/')
        && !path.contains('.'))
}

fn parse_colon(line: &str) -> Option<PipeLocation> {
    for (index, character) in line.char_indices() {
        if character != ':' {
            continue;
        }
        let rest = &line[index + 1..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            continue;
        }
        let line_number = rest[..digits].parse::<usize>().ok()?;
        if line_number == 0 {
            return None;
        }
        let after_line = &rest[digits..];
        let path = line[..index].trim();
        if path.is_empty() {
            continue;
        }
        if let Some(after_colon) = after_line.strip_prefix(':') {
            let column_digits = after_colon.bytes().take_while(u8::is_ascii_digit).count();
            if column_digits > 0 {
                let column = after_colon[..column_digits].parse::<usize>().ok()?;
                if column == 0 {
                    return None;
                }
                let message = after_colon[column_digits..]
                    .strip_prefix(':')
                    .unwrap_or(&after_colon[column_digits..])
                    .trim_start();
                return Some(PipeLocation {
                    path: path.to_owned(),
                    line: line_number,
                    column,
                    message: message.to_owned(),
                });
            }
            return Some(PipeLocation {
                path: path.to_owned(),
                line: line_number,
                column: 1,
                message: after_colon.trim_start().to_owned(),
            });
        }
        if after_line.is_empty() || after_line.chars().next().is_some_and(char::is_whitespace) {
            return Some(PipeLocation {
                path: path.to_owned(),
                line: line_number,
                column: 1,
                message: after_line.trim_start().to_owned(),
            });
        }
    }
    None
}

fn parse_parenthesized(line: &str) -> Option<PipeLocation> {
    let open = line.find('(')?;
    let close = line[open + 1..].find(')')? + open + 1;
    let path = line[..open].trim();
    if path.is_empty() {
        return None;
    }
    let (line_part, column_part) = line[open + 1..close].split_once(',')?;
    let line_number = line_part.trim().parse::<usize>().ok()?;
    let column = column_part.trim().parse::<usize>().ok()?;
    if line_number == 0 || column == 0 {
        return None;
    }
    let message = line[close + 1..].strip_prefix(':')?.trim_start();
    Some(PipeLocation {
        path: path.to_owned(),
        line: line_number,
        column,
        message: message.to_owned(),
    })
}

fn strip_ansi_csi(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }
        output.push(character);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_location_formats() {
        let parsed = parse(
            "src/main.rs:2:3: expected semicolon\n\
             src/main.rs:4: warning\n\
             --> src/lib.rs:8:9\n\
             src/app.c(10,11): error",
        );
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].path, "src/main.rs");
        assert_eq!(parsed[0].line, 2);
        assert_eq!(parsed[0].column, 3);
        assert_eq!(parsed[0].message, "expected semicolon");
        assert_eq!(parsed[3].path, "src/app.c");
        assert_eq!(parsed[3].line, 10);
        assert_eq!(parsed[3].column, 11);
    }

    #[test]
    fn strips_ansi_and_keeps_spaces_in_paths() {
        let parsed = parse("\u{1b}[31mmy docs/main.rs:5:2:\u{1b}[0m broken\n");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].path, "my docs/main.rs");
        assert_eq!(parsed[0].message, "broken");
    }

    #[test]
    fn ignores_malformed_and_duplicate_lines() {
        let parsed = parse(
            "not a location\n\
             src/main.rs:0:1: zero line\n\
             src/main.rs:1:0: zero column\n\
             src/main.rs:1:1: first\n\
             src/main.rs:1:1: duplicate",
        );
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].message, "first");
    }

    #[test]
    fn bounds_parsed_lines_and_locations() {
        let text = (0..MAX_PARSED_LINES + 10)
            .map(|index| format!("src/main.rs:{}:1: error\n", index + 1))
            .collect::<String>();
        assert_eq!(parse(&text).len(), MAX_LOCATIONS);
    }
}

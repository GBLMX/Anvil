//! File locations parsed from piped stdin text.

use super::Model;
use ratatui::layout::Rect;
use std::collections::HashSet;
use std::path::Path;

pub(crate) const MAX_LOCATIONS: usize = 10_000;
pub(crate) const MAX_PARSED_LINES: usize = 100_000;
pub(crate) const MAX_VISIBLE_LOCATIONS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PipeLocation {
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

pub(crate) fn parse(text: &str) -> Vec<PipeLocation> {
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
    if let Some(rest) = line.strip_prefix("-->") {
        return parse_colon(rest.trim_start());
    }
    parse_parenthesized(line).or_else(|| parse_colon(line))
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

pub(crate) fn panel_height(model: &Model, content_height: u16) -> u16 {
    if model.stdin_locations.is_empty() || content_height < 6 {
        return 0;
    }
    let rows = model.stdin_locations.len().min(MAX_VISIBLE_LOCATIONS) as u16;
    let height = (rows + 2).min(content_height.saturating_sub(2));
    if height < 3 { 0 } else { height }
}

pub(crate) fn panel_rect(model: &Model, content: Rect) -> Option<Rect> {
    let height = panel_height(model, content.height);
    if height == 0 {
        return None;
    }
    Some(Rect {
        x: content.x,
        y: content.y + content.height.saturating_sub(height),
        width: content.width,
        height,
    })
}

impl Model {
    pub(crate) fn set_pipe_locations(&mut self, text: &str) {
        self.stdin_locations = parse(text);
        self.stdin_location_selected = (!self.stdin_locations.is_empty()).then_some(0);
        self.stdin_location_scroll = 0;
    }

    pub(crate) fn move_stdin_location(&mut self, delta: isize) {
        if self.stdin_locations.is_empty() {
            return;
        }
        let selected = self
            .stdin_location_selected
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(self.stdin_locations.len().saturating_sub(1));
        self.stdin_location_selected = Some(selected);
        if selected < self.stdin_location_scroll {
            self.stdin_location_scroll = selected;
        } else if selected >= self.stdin_location_scroll + MAX_VISIBLE_LOCATIONS {
            self.stdin_location_scroll = selected + 1 - MAX_VISIBLE_LOCATIONS;
        }
        self.dirty = true;
    }

    pub(crate) fn select_stdin_location(&mut self, index: usize) {
        if index >= self.stdin_locations.len() {
            return;
        }
        self.stdin_location_selected = Some(index);
        self.dirty = true;
    }

    pub(crate) fn open_stdin_location(&mut self) {
        let Some(location) = self
            .stdin_location_selected
            .and_then(|index| self.stdin_locations.get(index))
            .cloned()
        else {
            return;
        };
        let path = match self.absolute_path(Path::new(&location.path)) {
            Ok(path) => path,
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                return;
            }
        };
        if !path.starts_with(&self.workspace) {
            self.status = format!("E: location outside workspace: {}", location.path);
            self.dirty = true;
            return;
        }
        if !path.is_file() {
            self.status = format!("E: location not found: {}", location.path);
            self.dirty = true;
            return;
        }
        if self.path.as_deref() != Some(path.as_path())
            && let Err(error) = self.open_path(&path)
        {
            self.status = format!("E: {error}");
            self.dirty = true;
            return;
        }
        let line = location
            .line
            .saturating_sub(1)
            .min(self.editor.index().len_lines().saturating_sub(1));
        self.cursor = self.offset_at_utf16(line, location.column.saturating_sub(1));
        self.status = format!(
            "location {}:{}:{}",
            location.path, location.line, location.column
        );
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_core::TextBuffer;

    #[test]
    fn parses_supported_location_formats() {
        let parsed = parse(
            "src/main.rs:2:3: expected semicolon\n\
             src/main.rs:4: warning\n\
             --> src/lib.rs:8:9\n\
             src/app.c(10,11): error",
        );
        assert_eq!(
            parsed,
            vec![
                PipeLocation {
                    path: "src/main.rs".to_owned(),
                    line: 2,
                    column: 3,
                    message: "expected semicolon".to_owned(),
                },
                PipeLocation {
                    path: "src/main.rs".to_owned(),
                    line: 4,
                    column: 1,
                    message: "warning".to_owned(),
                },
                PipeLocation {
                    path: "src/lib.rs".to_owned(),
                    line: 8,
                    column: 9,
                    message: String::new(),
                },
                PipeLocation {
                    path: "src/app.c".to_owned(),
                    line: 10,
                    column: 11,
                    message: "error".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn strips_ansi_and_keeps_spaces_in_paths() {
        let parsed = parse("\u{1b}[31mmy docs/main.rs:5:2:\u{1b}[0m broken\n");
        assert_eq!(
            parsed,
            vec![PipeLocation {
                path: "my docs/main.rs".to_owned(),
                line: 5,
                column: 2,
                message: "broken".to_owned(),
            }]
        );
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
        assert_eq!(
            parsed,
            vec![PipeLocation {
                path: "src/main.rs".to_owned(),
                line: 1,
                column: 1,
                message: "first".to_owned(),
            }]
        );
    }

    #[test]
    fn bounds_parsed_lines_and_locations() {
        let text = (0..MAX_PARSED_LINES + 10)
            .map(|index| format!("src/main.rs:{}:1: error\n", index + 1))
            .collect::<String>();
        assert_eq!(parse(&text).len(), MAX_LOCATIONS);
    }

    #[test]
    fn enormous_lines_do_not_panic() {
        let text = format!("src/main.rs:1:1: {}", "x".repeat(1_000_000));
        assert_eq!(parse(&text).len(), 1);
    }

    #[test]
    fn opens_locations_inside_the_workspace_and_clamps_position() {
        use anvil_editor::EditorState;

        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src/main.rs");
        std::fs::create_dir_all(source.parent().expect("parent")).expect("mkdir");
        std::fs::write(&source, "one\ntwo\nthree\n").expect("write");
        let mut model = Model::new(EditorState::from_text("log"), None);
        model.workspace = dir.path().canonicalize().expect("canonical");
        model.set_pipe_locations("src/main.rs:99:99: error");
        model.open_stdin_location();
        assert_eq!(
            model.path.as_deref(),
            Some(source.canonicalize().expect("canonical").as_path())
        );
        assert_eq!(model.cursor, model.editor.text().len_bytes());
        assert!(model.status.starts_with("location src/main.rs:99:99"));
    }

    #[test]
    fn rejects_locations_outside_the_workspace() {
        use anvil_editor::EditorState;

        let dir = tempfile::tempdir().expect("tempdir");
        let outside = dir.path().join("outside.rs");
        std::fs::write(&outside, "outside").expect("write");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("mkdir");
        let mut model = Model::new(EditorState::from_text("log"), None);
        model.workspace = workspace.canonicalize().expect("canonical");
        model.set_pipe_locations("../outside.rs:1:1: error");
        model.open_stdin_location();
        assert!(model.path.is_none());
        assert!(model.status.contains("outside workspace"));
    }
}

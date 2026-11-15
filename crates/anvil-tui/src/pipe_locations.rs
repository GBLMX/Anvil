//! File location UI over the workspace pipe parser.

use super::Model;
use ratatui::layout::Rect;
use std::path::Path;

pub(crate) use anvil_workspace::PipeLocation;
pub(crate) use anvil_workspace::pipe_locations::parse;

pub(crate) const MAX_VISIBLE_LOCATIONS: usize = 8;

pub(crate) fn panel_height(model: &Model, content_height: u16) -> u16 {
    if model.overlays.location.stdin_locations.is_empty() || content_height < 6 {
        return 0;
    }
    let rows = model
        .overlays
        .location
        .stdin_locations
        .len()
        .min(MAX_VISIBLE_LOCATIONS) as u16;
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
        self.overlays.location.stdin_locations = parse(text);
        self.overlays.location.stdin_location_selected =
            (!self.overlays.location.stdin_locations.is_empty()).then_some(0);
        self.overlays.location.stdin_location_scroll = 0;
        self.overlays.location.location_title = "Locations".to_owned();
    }

    pub(crate) fn set_location_results(&mut self, title: String, locations: Vec<PipeLocation>) {
        self.overlays.location.stdin_locations = locations;
        self.overlays.location.stdin_location_selected =
            (!self.overlays.location.stdin_locations.is_empty()).then_some(0);
        self.overlays.location.stdin_location_scroll = 0;
        self.overlays.location.location_title = title;
        self.file.dirty = true;
    }

    pub(crate) fn set_project_search_results(&mut self, query: &str, locations: Vec<PipeLocation>) {
        self.set_location_results(format!("Search: {query}"), locations);
    }

    pub(crate) fn move_stdin_location(&mut self, delta: isize) {
        if self.overlays.location.stdin_locations.is_empty() {
            return;
        }
        let selected = self
            .overlays
            .location
            .stdin_location_selected
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(
                self.overlays
                    .location
                    .stdin_locations
                    .len()
                    .saturating_sub(1),
            );
        self.overlays.location.stdin_location_selected = Some(selected);
        if selected < self.overlays.location.stdin_location_scroll {
            self.overlays.location.stdin_location_scroll = selected;
        } else if selected >= self.overlays.location.stdin_location_scroll + MAX_VISIBLE_LOCATIONS {
            self.overlays.location.stdin_location_scroll = selected + 1 - MAX_VISIBLE_LOCATIONS;
        }
        self.file.dirty = true;
    }

    pub(crate) fn select_stdin_location(&mut self, index: usize) {
        if index >= self.overlays.location.stdin_locations.len() {
            return;
        }
        self.overlays.location.stdin_location_selected = Some(index);
        self.file.dirty = true;
    }

    pub(crate) fn open_stdin_location(&mut self) {
        let Some(location) = self
            .overlays
            .location
            .stdin_location_selected
            .and_then(|index| self.overlays.location.stdin_locations.get(index))
            .cloned()
        else {
            return;
        };
        let path = if Path::new(&location.path).is_absolute() {
            match self
                .workspace
                .roots
                .resolve(Path::new(&location.path), true)
            {
                Ok(path) => path,
                Err(error) => {
                    self.status.text = format!("E: {error}");
                    self.file.dirty = true;
                    return;
                }
            }
        } else {
            let path = match self.workspace.absolute_path(Path::new(&location.path)) {
                Ok(path) => path,
                Err(error) => {
                    self.status.text = format!("E: {error}");
                    self.file.dirty = true;
                    return;
                }
            };
            if !path.starts_with(&self.workspace.path) {
                self.status.text = format!("E: location outside workspace: {}", location.path);
                self.file.dirty = true;
                return;
            }
            path
        };
        if !path.is_file() {
            self.status.text = format!("E: location not found: {}", location.path);
            self.file.dirty = true;
            return;
        }
        self.record_jump();
        if self.session.path.as_deref() != Some(path.as_path())
            && let Err(error) = self.open_path(&path)
        {
            self.status.text = format!("E: {error}");
            self.file.dirty = true;
            return;
        }
        let line = location
            .line
            .saturating_sub(1)
            .min(self.session.editor.index().len_lines().saturating_sub(1));
        self.session.cursor = self.offset_at_utf16(line, location.column.saturating_sub(1));
        self.status.text = format!(
            "location {}:{}:{}",
            location.path, location.line, location.column
        );
        self.file.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_core::TextBuffer;
    use anvil_workspace::pipe_locations::{MAX_LOCATIONS, MAX_PARSED_LINES};

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
    fn colon_form_wins_over_parenthesized_text_in_the_message() {
        let parsed = parse("foo.rs:2:3: error: expected (1,2): value");
        assert_eq!(
            parsed,
            vec![PipeLocation {
                path: "foo.rs".to_owned(),
                line: 2,
                column: 3,
                message: "error: expected (1,2): value".to_owned(),
            }]
        );
    }

    #[test]
    fn timestamped_log_lines_are_not_locations() {
        assert!(parse("2026-11-11T18:08:59 INFO startup").is_empty());
        assert!(parse("18:08:59 INFO startup").is_empty());
        assert_eq!(
            parse("src/2026.rs:1:2: error").len(),
            1,
            "numeric file names with a path separator stay valid"
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
        model.workspace.path = dir.path().canonicalize().expect("canonical");
        model.set_pipe_locations("src/main.rs:99:99: error");
        model.open_stdin_location();
        assert_eq!(
            model.session.path.as_deref(),
            Some(source.canonicalize().expect("canonical").as_path())
        );
        assert_eq!(
            model.session.cursor,
            model.session.editor.text().len_bytes()
        );
        assert!(model.status.text.starts_with("location src/main.rs:99:99"));
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
        model.workspace.path = workspace.canonicalize().expect("canonical");
        model.set_pipe_locations("../outside.rs:1:1: error");
        model.open_stdin_location();
        assert!(model.session.path.is_none());
        assert!(model.status.text.contains("outside workspace"));
    }

    #[test]
    fn opens_locations_in_trusted_secondary_root() {
        use anvil_editor::EditorState;

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("root");
        let source = root.join("src/lib.rs");
        std::fs::create_dir_all(source.parent().expect("parent")).expect("mkdir");
        std::fs::write(&source, "pub fn lib() {}\n").expect("write");
        let root = root.canonicalize().expect("canonical");
        let source = source.canonicalize().expect("canonical");
        let mut model = Model::new(EditorState::from_text("log"), None);
        model.workspace.roots.add(&root, true).expect("add root");
        let location = format!("{}:1:8: needle", source.display());
        model.set_pipe_locations(&location);
        model.open_stdin_location();
        assert_eq!(model.session.path.as_deref(), Some(source.as_path()));
        assert!(model.status.text.starts_with("location"));
    }
}

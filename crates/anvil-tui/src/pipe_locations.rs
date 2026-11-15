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

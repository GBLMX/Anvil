//! Reviewable literal replace across workspace files.

use super::*;
use crate::file_worker::{FileRequest, ProjectReplacePlan};
use crate::pipe_locations::PipeLocation;

impl Model {
    pub(crate) fn preview_project_replace(&mut self, spec: &str) {
        let Some((old, new)) = spec.split_once('/') else {
            self.status = "E: usage: :replace-project OLD/NEW/".to_owned();
            self.dirty = true;
            return;
        };
        let new = new.strip_suffix('/').unwrap_or(new);
        if old.is_empty() {
            self.status = "E: replace search text is empty".to_owned();
            self.dirty = true;
            return;
        }
        if !self.ensure_file_worker() {
            self.status = "E: gateway unavailable".to_owned();
            self.dirty = true;
            return;
        }
        let Some(worker) = self.file_worker.as_ref() else {
            self.status = "E: file worker unavailable".to_owned();
            self.dirty = true;
            return;
        };
        self.project_replace_generation = self.project_replace_generation.wrapping_add(1);
        let generation = self.project_replace_generation;
        if let Err(error) = worker.request(FileRequest::ProjectReplacePreview {
            old: old.to_owned(),
            new: new.to_owned(),
            generation,
        }) {
            self.status = format!("E: replace preview: {error}");
            self.dirty = true;
            return;
        }
        self.project_replace_pending = Some(generation);
        self.status = "building replace preview...".to_owned();
        self.dirty = true;
    }

    pub(crate) fn handle_project_replace_preview(
        &mut self,
        generation: u64,
        result: Result<ProjectReplacePlan, String>,
    ) {
        if self.project_replace_pending != Some(generation) {
            return;
        }
        self.project_replace_pending = None;
        match result {
            Ok(plan) => {
                if plan.files.is_empty() {
                    self.project_replace_plan = None;
                    self.stdin_locations.clear();
                    self.stdin_location_selected = None;
                    self.location_title = "Locations".to_owned();
                    self.status = format!("replace: no matches for {}", plan.old);
                } else {
                    let locations = plan
                        .files
                        .iter()
                        .map(|file| PipeLocation {
                            path: file.relative.clone(),
                            line: 1,
                            column: 1,
                            message: format!("{} matches", file.matches),
                        })
                        .collect::<Vec<_>>();
                    let count = locations.len();
                    self.set_location_results(
                        format!("Replace: {} -> {}", plan.old, plan.new),
                        locations,
                    );
                    self.project_replace_plan = Some(plan);
                    self.status = format!(
                        "replace preview: {count} files; apply with :replace-project-apply"
                    );
                }
            }
            Err(error) => {
                self.status = format!("E: replace preview: {error}");
                self.project_replace_plan = None;
            }
        }
        self.dirty = true;
    }

    pub(crate) fn apply_project_replace(&mut self) {
        let Some(plan) = self.project_replace_plan.clone() else {
            self.status = "E: no replace preview to apply".to_owned();
            self.dirty = true;
            return;
        };
        if self.any_modified() {
            self.status = "E: save or discard modified buffers first".to_owned();
            self.dirty = true;
            return;
        }
        if !self.ensure_file_worker() {
            self.status = "E: gateway unavailable".to_owned();
            self.dirty = true;
            return;
        }
        let Some(worker) = self.file_worker.as_ref() else {
            self.status = "E: file worker unavailable".to_owned();
            self.dirty = true;
            return;
        };
        self.project_replace_generation = self.project_replace_generation.wrapping_add(1);
        let generation = self.project_replace_generation;
        if let Err(error) = worker.request(FileRequest::ProjectReplaceApply { plan, generation }) {
            self.status = format!("E: replace apply: {error}");
            self.dirty = true;
            return;
        }
        self.project_replace_pending = Some(generation);
        self.status = "applying project replace...".to_owned();
        self.dirty = true;
    }

    pub(crate) fn handle_project_replace_applied(
        &mut self,
        generation: u64,
        result: Result<(), String>,
    ) {
        if self.project_replace_pending != Some(generation) {
            return;
        }
        self.project_replace_pending = None;
        match result {
            Ok(()) => {
                if let Some(plan) = self.project_replace_plan.take() {
                    for file in &plan.files {
                        let path = self.workspace.join(&file.relative);
                        let _ = self.reload_external(&path);
                    }
                    self.status = format!("replaced {} files", plan.files.len());
                }
                self.stdin_locations.clear();
                self.stdin_location_selected = None;
                self.location_title = "Locations".to_owned();
            }
            Err(error) => {
                self.status = format!("E: replace apply: {error}");
            }
        }
    }

    pub(crate) fn cancel_project_replace(&mut self) {
        self.project_replace_plan = None;
        self.project_replace_pending = None;
        self.stdin_locations.clear();
        self.stdin_location_selected = None;
        self.stdin_location_scroll = 0;
        self.location_title = "Locations".to_owned();
        self.status = "replace preview cancelled".to_owned();
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_worker::ProjectReplaceFile;
    use anvil_editor::EditorState;

    fn plan() -> ProjectReplacePlan {
        ProjectReplacePlan {
            old: "old".to_owned(),
            new: "new".to_owned(),
            files: vec![ProjectReplaceFile {
                relative: "a.txt".to_owned(),
                matches: 2,
                hash: [0; 32],
            }],
        }
    }

    #[test]
    fn preview_populates_the_location_panel() {
        let mut model = Model::new(EditorState::from_text("x"), None);
        model.project_replace_pending = Some(3);
        model.handle_project_replace_preview(3, Ok(plan()));
        assert!(model.project_replace_plan.is_some());
        assert_eq!(model.location_title, "Replace: old -> new");
        assert_eq!(model.stdin_locations.len(), 1);
        model.cancel_project_replace();
        assert!(model.project_replace_plan.is_none());
        assert!(model.stdin_locations.is_empty());
    }

    #[test]
    fn applied_replace_reloads_open_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("mkdir");
        let path = workspace.join("a.txt");
        std::fs::write(&path, "new\n").expect("write");
        let mut model = Model::new(EditorState::from_text("old\n"), Some(path));
        model.workspace = workspace;
        model.project_replace_plan = Some(plan());
        model.project_replace_pending = Some(4);
        model.handle_project_replace_applied(4, Ok(()));
        assert_eq!(
            model
                .editor
                .text()
                .slice(0..model.editor.text().len_bytes()),
            "new\n"
        );
        assert!(model.project_replace_plan.is_none());
    }
}

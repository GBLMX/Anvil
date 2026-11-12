//! Reviewable literal replace across workspace files.

use super::*;
use crate::file_worker::{FileRequest, ProjectReplacePlan};
use crate::pipe_locations::PipeLocation;

impl Model {
    pub(crate) fn preview_project_replace(&mut self, spec: &str) {
        let Some((old, new)) = spec.split_once('/') else {
            self.status.text = "E: usage: :replace-project OLD/NEW/".to_owned();
            self.dirty = true;
            return;
        };
        let new = new.strip_suffix('/').unwrap_or(new);
        if old.is_empty() {
            self.status.text = "E: replace search text is empty".to_owned();
            self.dirty = true;
            return;
        }
        if !self.ensure_file_worker() {
            self.status.text = "E: gateway unavailable".to_owned();
            self.dirty = true;
            return;
        }
        let Some(worker) = self.workspace.file_worker.as_ref() else {
            self.status.text = "E: file worker unavailable".to_owned();
            self.dirty = true;
            return;
        };
        self.project.replace_generation = self.project.replace_generation.wrapping_add(1);
        let generation = self.project.replace_generation;
        if let Err(error) = worker.request(FileRequest::ProjectReplacePreview {
            old: old.to_owned(),
            new: new.to_owned(),
            generation,
        }) {
            self.status.text = format!("E: replace preview: {error}");
            self.dirty = true;
            return;
        }
        self.project.replace_pending = Some(generation);
        self.status.text = "building replace preview...".to_owned();
        self.dirty = true;
    }

    pub(crate) fn handle_project_replace_preview(
        &mut self,
        generation: u64,
        result: Result<ProjectReplacePlan, String>,
    ) {
        if self.project.replace_pending != Some(generation) {
            return;
        }
        self.project.replace_pending = None;
        match result {
            Ok(plan) => {
                if plan.files.is_empty() {
                    self.project.replace_plan = None;
                    self.overlays.location.stdin_locations.clear();
                    self.overlays.location.stdin_location_selected = None;
                    self.overlays.location.location_title = "Locations".to_owned();
                    self.status.text = format!("replace: no matches for {}", plan.old);
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
                    self.project.replace_plan = Some(plan);
                    self.status.text = format!(
                        "replace preview: {count} files; apply with :replace-project-apply"
                    );
                }
            }
            Err(error) => {
                self.status.text = format!("E: replace preview: {error}");
                self.project.replace_plan = None;
            }
        }
        self.dirty = true;
    }

    pub(crate) fn apply_project_replace(&mut self) {
        let Some(plan) = self.project.replace_plan.clone() else {
            self.status.text = "E: no replace preview to apply".to_owned();
            self.dirty = true;
            return;
        };
        if self.any_modified() {
            self.status.text = "E: save or discard modified buffers first".to_owned();
            self.dirty = true;
            return;
        }
        if !self.ensure_file_worker() {
            self.status.text = "E: gateway unavailable".to_owned();
            self.dirty = true;
            return;
        }
        let Some(worker) = self.workspace.file_worker.as_ref() else {
            self.status.text = "E: file worker unavailable".to_owned();
            self.dirty = true;
            return;
        };
        self.project.replace_generation = self.project.replace_generation.wrapping_add(1);
        let generation = self.project.replace_generation;
        if let Err(error) = worker.request(FileRequest::ProjectReplaceApply { plan, generation }) {
            self.status.text = format!("E: replace apply: {error}");
            self.dirty = true;
            return;
        }
        self.project.replace_pending = Some(generation);
        self.status.text = "applying project replace...".to_owned();
        self.dirty = true;
    }

    pub(crate) fn handle_project_replace_applied(
        &mut self,
        generation: u64,
        result: Result<(), String>,
    ) {
        if self.project.replace_pending != Some(generation) {
            return;
        }
        self.project.replace_pending = None;
        match result {
            Ok(()) => {
                if let Some(plan) = self.project.replace_plan.take() {
                    if self.any_modified() {
                        for file in &plan.files {
                            self.workspace
                                .stale_paths
                                .insert(self.workspace.path.join(&file.relative));
                        }
                        self.status.text =
                            "replace applied; modified buffers marked stale (reload before saving)"
                                .to_owned();
                    } else {
                        let mut reload_errors = Vec::new();
                        for file in &plan.files {
                            let path = self.workspace.path.join(&file.relative);
                            if let Err(error) = self.reload_external(&path) {
                                self.workspace.stale_paths.insert(path);
                                reload_errors.push(format!("{}: {error}", file.relative));
                            }
                        }
                        self.status.text = if reload_errors.is_empty() {
                            format!("replaced {} files", plan.files.len())
                        } else {
                            format!(
                                "replace applied; reload failed for: {}",
                                reload_errors.join(", ")
                            )
                        };
                    }
                }
                self.overlays.location.stdin_locations.clear();
                self.overlays.location.stdin_location_selected = None;
                self.overlays.location.location_title = "Locations".to_owned();
            }
            Err(error) => {
                self.status.text = format!("E: replace apply: {error}");
            }
        }
    }

    pub(crate) fn cancel_project_replace(&mut self) {
        self.project.replace_plan = None;
        self.project.replace_pending = None;
        self.overlays.location.stdin_locations.clear();
        self.overlays.location.stdin_location_selected = None;
        self.overlays.location.stdin_location_scroll = 0;
        self.overlays.location.location_title = "Locations".to_owned();
        self.status.text = "replace preview cancelled".to_owned();
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
        model.project.replace_pending = Some(3);
        model.handle_project_replace_preview(3, Ok(plan()));
        assert!(model.project.replace_plan.is_some());
        assert_eq!(
            model.overlays.location.location_title,
            "Replace: old -> new"
        );
        assert_eq!(model.overlays.location.stdin_locations.len(), 1);
        model.cancel_project_replace();
        assert!(model.project.replace_plan.is_none());
        assert!(model.overlays.location.stdin_locations.is_empty());
    }

    #[test]
    fn applied_replace_reloads_open_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("mkdir");
        let path = workspace.join("a.txt");
        std::fs::write(&path, "new\n").expect("write");
        let mut model = Model::new(EditorState::from_text("old\n"), Some(path));
        model.workspace.path = workspace;
        model.project.replace_plan = Some(plan());
        model.project.replace_pending = Some(4);
        model.handle_project_replace_applied(4, Ok(()));
        assert_eq!(
            model
                .session
                .editor
                .text()
                .slice(0..model.session.editor.text().len_bytes()),
            "new\n"
        );
        assert!(model.project.replace_plan.is_none());
    }

    #[test]
    fn missing_file_reports_reload_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("mkdir");
        let path = workspace.join("a.txt");
        let mut model = Model::new(EditorState::from_text("x"), Some(path));
        model.workspace.path = workspace;
        model.project.replace_plan = Some(plan());
        model.project.replace_pending = Some(5);
        model.handle_project_replace_applied(5, Ok(()));
        assert!(model.status.text.contains("reload failed"));
        assert!(!model.status.text.contains("replaced 1 files"));
        assert!(!model.save());
        assert!(model.status.text.contains("reload before saving"));
    }

    #[test]
    fn modified_buffer_skips_reload_after_apply() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("mkdir");
        let path = workspace.join("a.txt");
        std::fs::write(&path, "new\n").expect("write");
        let mut model = Model::new(EditorState::from_text("old\n"), Some(path));
        model.workspace.path = workspace;
        model.modified = true;
        model.project.replace_plan = Some(plan());
        model.project.replace_pending = Some(6);
        model.handle_project_replace_applied(6, Ok(()));
        assert_eq!(
            model
                .session
                .editor
                .text()
                .slice(0..model.session.editor.text().len_bytes()),
            "old\n"
        );
        assert!(model.status.text.contains("modified buffers"));
        assert!(!model.save());
        assert!(model.status.text.contains("reload before saving"));
    }
}

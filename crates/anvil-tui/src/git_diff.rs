//! Read-only Git diff view for the current buffer.

use super::*;
use crate::file_worker::FileRequest;

impl Model {
    pub(crate) fn request_git_diff(&mut self) {
        if self.help_back.is_some() {
            self.status = "already viewing a read-only view".to_owned();
            self.dirty = true;
            return;
        }
        let Some(path) = self.path.clone() else {
            self.status = "E: no file name".to_owned();
            self.dirty = true;
            return;
        };
        let relative = match self.relative_path(&path) {
            Ok(relative) => relative,
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                return;
            }
        };
        let Some(worker) = self.file_worker.as_ref() else {
            self.status = "E: file worker unavailable".to_owned();
            self.dirty = true;
            return;
        };
        self.git_diff_generation = self.git_diff_generation.wrapping_add(1);
        let generation = self.git_diff_generation;
        if let Err(error) = worker.request(FileRequest::GitDiffText {
            relative,
            generation,
        }) {
            self.status = format!("E: git diff: {error}");
            self.dirty = true;
            return;
        }
        self.git_diff_pending = Some(generation);
        self.status = "loading git diff...".to_owned();
        self.dirty = true;
    }

    pub(crate) fn handle_git_diff_text(
        &mut self,
        generation: u64,
        result: Result<(String, bool), String>,
    ) {
        if self.git_diff_pending != Some(generation) {
            return;
        }
        self.git_diff_pending = None;
        match result {
            Ok((text, truncated)) => {
                let title = if truncated {
                    "Git diff (truncated): :q returns"
                } else {
                    "Git diff: :q returns"
                };
                let view = format!("Git diff\n\n{text}");
                self.show_read_only(&view, title, "Git diff");
            }
            Err(error) => {
                self.status = format!("E: git diff: {error}");
                self.dirty = true;
            }
        }
    }
}

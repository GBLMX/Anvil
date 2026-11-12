//! Read-only Git diff view with hunk selection and index staging.

use super::*;
use crate::file_worker::FileRequest;

#[derive(Debug, Clone)]
pub(crate) struct GitDiffView {
    relative: String,
    staged: bool,
    truncated: bool,
    hunks: Vec<GitDiffHunk>,
}

#[derive(Debug, Clone)]
struct GitDiffHunk {
    line: usize,
    patch: String,
}

impl GitDiffView {
    fn parse(raw: &str, relative: String, staged: bool, truncated: bool) -> Self {
        let lines = raw.lines().collect::<Vec<_>>();
        let Some(first) = lines.iter().position(|line| line.starts_with("@@ ")) else {
            return Self {
                relative,
                staged,
                truncated,
                hunks: Vec::new(),
            };
        };
        let header = lines[..first].join("\n");
        let mut hunks = Vec::new();
        let mut start = first;
        while start < lines.len() {
            let end = lines[start + 1..]
                .iter()
                .position(|line| line.starts_with("@@ "))
                .map(|offset| start + 1 + offset)
                .unwrap_or(lines.len());
            let mut patch = String::new();
            patch.push_str(&header);
            patch.push('\n');
            for line in &lines[start..end] {
                patch.push_str(line);
                patch.push('\n');
            }
            hunks.push(GitDiffHunk {
                line: start + 2,
                patch,
            });
            start = end;
        }
        Self {
            relative,
            staged,
            truncated,
            hunks,
        }
    }
}

impl Model {
    pub(crate) fn request_git_diff(&mut self, staged: bool) {
        if self.buffers.help_back.is_some() {
            if let Some(view) = self.git.diff_view.as_ref() {
                let relative = view.relative.clone();
                self.request_diff(relative, staged);
                return;
            }
            self.status.text = "already viewing a read-only view".to_owned();
            self.file.dirty = true;
            return;
        }
        let Some(path) = self.session.path.clone() else {
            self.status.text = "E: no file name".to_owned();
            self.file.dirty = true;
            return;
        };
        let relative = match self.relative_path(&path) {
            Ok(relative) => relative,
            Err(error) => {
                self.status.text = format!("E: {error}");
                self.file.dirty = true;
                return;
            }
        };
        self.request_diff(relative, staged);
    }

    fn request_diff(&mut self, relative: String, staged: bool) {
        let Some(worker) = self.workspace.file_worker.as_ref() else {
            self.status.text = "E: file worker unavailable".to_owned();
            self.file.dirty = true;
            return;
        };
        self.git.diff_generation = self.git.diff_generation.wrapping_add(1);
        let generation = self.git.diff_generation;
        if let Err(error) = worker.request(FileRequest::GitDiffText {
            relative,
            staged,
            generation,
        }) {
            self.status.text = format!("E: git diff: {error}");
            self.file.dirty = true;
            return;
        }
        self.git.diff_pending = Some(generation);
        self.status.text = if staged {
            "loading staged git diff...".to_owned()
        } else {
            "loading git diff...".to_owned()
        };
        self.file.dirty = true;
    }

    pub(crate) fn handle_git_diff_text(
        &mut self,
        relative: String,
        staged: bool,
        generation: u64,
        result: Result<(String, bool), String>,
    ) {
        if self.git.diff_pending != Some(generation) {
            return;
        }
        self.git.diff_pending = None;
        match result {
            Ok((text, truncated)) => {
                let view = GitDiffView::parse(&text, relative, staged, truncated);
                self.git.diff_view = Some(view);
                let title = if truncated {
                    "Git diff (truncated): :q returns"
                } else {
                    "Git diff: :q returns"
                };
                let header = if staged {
                    "Git diff (staged)"
                } else {
                    "Git diff"
                };
                let body = format!("{header}\n\n{text}");
                if self.buffers.help_back.is_some()
                    && self.buffers.read_only_title.as_deref() != Some("Git diff")
                {
                    self.git.diff_view = None;
                    return;
                }
                self.replace_read_only(&body, title, "Git diff");
            }
            Err(error) => {
                self.status.text = format!("E: git diff: {error}");
                self.file.dirty = true;
            }
        }
    }

    pub(crate) fn move_git_hunk(&mut self, forward: bool) {
        let Some(view) = self.git.diff_view.as_ref() else {
            self.status.text = "E: not viewing a git diff".to_owned();
            self.file.dirty = true;
            return;
        };
        if view.hunks.is_empty() {
            self.status.text = "E: diff has no hunks".to_owned();
            self.file.dirty = true;
            return;
        }
        let current = self
            .session
            .editor
            .index()
            .offset_to_line(self.session.cursor)
            .map(|line| line.line)
            .unwrap_or(0);
        let index = if forward {
            view.hunks.iter().position(|hunk| hunk.line > current)
        } else {
            view.hunks.iter().rposition(|hunk| hunk.line < current)
        };
        let Some(index) = index else {
            self.status.text = if forward {
                "last git hunk"
            } else {
                "first git hunk"
            }
            .to_owned();
            self.file.dirty = true;
            return;
        };
        let line = view.hunks[index].line;
        if let Some(offset) = self.session.editor.index().line_to_offset(line) {
            self.session.cursor = offset;
            self.session.editor.set_selection_bytes(offset, offset);
            self.status.text = format!("git hunk {}/{}", index + 1, view.hunks.len());
            self.file.dirty = true;
        }
    }

    pub(crate) fn apply_current_hunk(&mut self, unstage: bool) {
        let Some(view) = self.git.diff_view.as_ref() else {
            self.status.text = "E: not viewing a git diff".to_owned();
            self.file.dirty = true;
            return;
        };
        if view.truncated {
            self.status.text = "E: diff is truncated; hunk staging disabled".to_owned();
            self.file.dirty = true;
            return;
        }
        if unstage != view.staged {
            self.status.text = if unstage {
                "E: open :git-diff --staged to unstage".to_owned()
            } else {
                "E: open :git-diff to stage".to_owned()
            };
            self.file.dirty = true;
            return;
        }
        if view.hunks.is_empty() {
            self.status.text = "E: diff has no hunks".to_owned();
            self.file.dirty = true;
            return;
        }
        let current = self
            .session
            .editor
            .index()
            .offset_to_line(self.session.cursor)
            .map(|line| line.line)
            .unwrap_or(0);
        let Some(index) = view.hunks.iter().rposition(|hunk| hunk.line <= current) else {
            self.status.text = "E: cursor is before the first hunk".to_owned();
            self.file.dirty = true;
            return;
        };
        let patch = view.hunks[index].patch.clone();
        let Some(worker) = self.workspace.file_worker.as_ref() else {
            self.status.text = "E: file worker unavailable".to_owned();
            self.file.dirty = true;
            return;
        };
        self.git.apply_generation = self.git.apply_generation.wrapping_add(1);
        let generation = self.git.apply_generation;
        if let Err(error) = worker.request(FileRequest::GitApplyHunk {
            patch,
            reverse: unstage,
            generation,
        }) {
            self.status.text = format!("E: git apply: {error}");
            self.file.dirty = true;
            return;
        }
        self.git.apply_pending = Some(generation);
        self.status.text = if unstage {
            "unstaging hunk...".to_owned()
        } else {
            "staging hunk...".to_owned()
        };
        self.file.dirty = true;
    }

    pub(crate) fn handle_git_applied(&mut self, generation: u64, result: Result<(), String>) {
        if self.git.apply_pending != Some(generation) {
            return;
        }
        self.git.apply_pending = None;
        match result {
            Ok(()) => {
                self.status.text = "git hunk applied".to_owned();
                if let Some(view) = self.git.diff_view.as_ref() {
                    let relative = view.relative.clone();
                    let staged = view.staged;
                    self.request_diff(relative, staged);
                }
            }
            Err(error) => {
                self.status.text = format!("E: git apply: {error}");
                self.file.dirty = true;
            }
        }
    }
}

impl Model {
    pub(crate) fn request_git_blame(&mut self) {
        if self.buffers.help_back.is_some() {
            self.status.text = "already viewing a read-only view".to_owned();
            self.file.dirty = true;
            return;
        }
        let Some(path) = self.session.path.clone() else {
            self.status.text = "E: no file name".to_owned();
            self.file.dirty = true;
            return;
        };
        let relative = match self.relative_path(&path) {
            Ok(relative) => relative,
            Err(error) => {
                self.status.text = format!("E: {error}");
                self.file.dirty = true;
                return;
            }
        };
        let Some(worker) = self.workspace.file_worker.as_ref() else {
            self.status.text = "E: file worker unavailable".to_owned();
            self.file.dirty = true;
            return;
        };
        self.git.blame_generation = self.git.blame_generation.wrapping_add(1);
        let generation = self.git.blame_generation;
        if let Err(error) = worker.request(FileRequest::GitBlameText {
            relative,
            generation,
        }) {
            self.status.text = format!("E: git blame: {error}");
            self.file.dirty = true;
            return;
        }
        self.git.blame_pending = Some(generation);
        self.status.text = "loading git blame...".to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn handle_git_blame_text(
        &mut self,
        generation: u64,
        result: Result<(String, bool), String>,
    ) {
        if self.git.blame_pending != Some(generation) {
            return;
        }
        self.git.blame_pending = None;
        match result {
            Ok((text, truncated)) => {
                let title = if truncated {
                    "Git blame (truncated): :q returns"
                } else {
                    "Git blame: :q returns"
                };
                let body = if truncated {
                    format!("Git blame (truncated)\n\n{text}")
                } else {
                    format!("Git blame\n\n{text}")
                };
                self.show_read_only(&body, title, "Git blame");
            }
            Err(error) => {
                self.status.text = format!("E: git blame: {error}");
                self.file.dirty = true;
            }
        }
    }
}

impl Model {
    pub(crate) fn request_git_conflicts(&mut self) {
        if self.buffers.help_back.is_some() {
            self.status.text = "already viewing a read-only view".to_owned();
            self.file.dirty = true;
            return;
        }
        if !self.ensure_file_worker() {
            self.status.text = "E: gateway unavailable".to_owned();
            self.file.dirty = true;
            return;
        }
        let Some(worker) = self.workspace.file_worker.as_ref() else {
            self.status.text = "E: file worker unavailable".to_owned();
            self.file.dirty = true;
            return;
        };
        self.git.conflicts_generation = self.git.conflicts_generation.wrapping_add(1);
        let generation = self.git.conflicts_generation;
        if let Err(error) = worker.request(FileRequest::GitConflictsText { generation }) {
            self.status.text = format!("E: git conflicts: {error}");
            self.file.dirty = true;
            return;
        }
        self.git.conflicts_pending = Some(generation);
        self.status.text = "loading git conflicts...".to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn handle_git_conflicts_text(
        &mut self,
        generation: u64,
        result: Result<(String, bool), String>,
    ) {
        if self.git.conflicts_pending != Some(generation) {
            return;
        }
        self.git.conflicts_pending = None;
        match result {
            Ok((text, truncated)) => {
                let title = if truncated {
                    "Git conflicts (truncated): :q returns"
                } else {
                    "Git conflicts: :q returns"
                };
                let body = if truncated {
                    format!("Git conflicts (truncated)\n\n{text}")
                } else {
                    format!("Git conflicts\n\n{text}")
                };
                self.show_read_only(&body, title, "Git conflicts");
            }
            Err(error) => {
                self.status.text = format!("E: git conflicts: {error}");
                self.file.dirty = true;
            }
        }
    }
}

impl Model {
    pub(crate) fn request_git_conflict_stage(&mut self, stage: u8) {
        if self.buffers.help_back.is_some() {
            self.status.text = "already viewing a read-only view".to_owned();
            self.file.dirty = true;
            return;
        }
        let Some(path) = self.session.path.clone() else {
            self.status.text = "E: no file name".to_owned();
            self.file.dirty = true;
            return;
        };
        let relative = match self.relative_path(&path) {
            Ok(relative) => relative,
            Err(error) => {
                self.status.text = format!("E: {error}");
                self.file.dirty = true;
                return;
            }
        };
        if !self.ensure_file_worker() {
            self.status.text = "E: gateway unavailable".to_owned();
            self.file.dirty = true;
            return;
        }
        let Some(worker) = self.workspace.file_worker.as_ref() else {
            self.status.text = "E: file worker unavailable".to_owned();
            self.file.dirty = true;
            return;
        };
        self.git.stage_generation = self.git.stage_generation.wrapping_add(1);
        let generation = self.git.stage_generation;
        if let Err(error) = worker.request(FileRequest::GitConflictStageText {
            relative,
            stage,
            generation,
        }) {
            self.status.text = format!("E: git conflict stage: {error}");
            self.file.dirty = true;
            return;
        }
        self.git.stage_pending = Some(generation);
        self.status.text = "loading conflict stage...".to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn handle_git_conflict_stage(
        &mut self,
        relative: String,
        stage: u8,
        generation: u64,
        result: Result<(String, bool), String>,
    ) {
        if self.git.stage_pending != Some(generation) {
            return;
        }
        self.git.stage_pending = None;
        let name = match stage {
            1 => "base",
            2 => "ours",
            3 => "theirs",
            _ => "unknown",
        };
        match result {
            Ok((text, truncated)) => {
                let title = if truncated {
                    format!("Git conflict {name} (truncated): :q returns")
                } else {
                    format!("Git conflict {name}: :q returns")
                };
                let body = if truncated {
                    format!("Git conflict {name}: {relative} (truncated)\n\n{text}")
                } else {
                    format!("Git conflict {name}: {relative}\n\n{text}")
                };
                self.show_read_only(&body, &title, &format!("Git conflict {name}"));
            }
            Err(error) => {
                self.status.text = format!("E: git conflict stage {name}: {error}");
                self.file.dirty = true;
            }
        }
    }

    pub(crate) fn resolve_git_conflict(&mut self, stage: u8, force: bool) {
        if self.buffers.help_back.is_some() {
            self.status.text = "E: close the read-only view first".to_owned();
            self.file.dirty = true;
            return;
        }
        let Some(path) = self.session.path.clone() else {
            self.status.text = "E: no file name".to_owned();
            self.file.dirty = true;
            return;
        };
        if self.file.modified && !force {
            self.status.text = "E: buffer is modified; save/discard or use the ! form".to_owned();
            self.file.dirty = true;
            return;
        }
        if self.git.resolve_pending.is_some() {
            self.status.text = "E: conflict resolution already in progress".to_owned();
            self.file.dirty = true;
            return;
        }
        let relative = match self.relative_path(&path) {
            Ok(relative) => relative,
            Err(error) => {
                self.status.text = format!("E: {error}");
                self.file.dirty = true;
                return;
            }
        };
        if !self.ensure_file_worker() {
            self.status.text = "E: gateway unavailable".to_owned();
            self.file.dirty = true;
            return;
        }
        let Some(worker) = self.workspace.file_worker.as_ref() else {
            self.status.text = "E: file worker unavailable".to_owned();
            self.file.dirty = true;
            return;
        };
        self.git.resolve_generation = self.git.resolve_generation.wrapping_add(1);
        let generation = self.git.resolve_generation;
        if let Err(error) = worker.request(FileRequest::GitResolveConflict {
            relative,
            stage,
            generation,
        }) {
            self.status.text = format!("E: git resolve: {error}");
            self.file.dirty = true;
            return;
        }
        self.git.resolve_pending = Some(generation);
        self.status.text = "resolving conflict...".to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn handle_git_resolved(
        &mut self,
        relative: String,
        stage: u8,
        generation: u64,
        result: Result<(), crate::file_worker::ResolveConflictError>,
    ) {
        if self.git.resolve_pending != Some(generation) {
            return;
        }
        self.git.resolve_pending = None;
        match result {
            Ok(()) => {
                let path = self.workspace.path.join(&relative);
                match self.reload_external(&path) {
                    Ok(()) => {
                        let name = if stage == 2 { "ours" } else { "theirs" };
                        self.status.text = format!("resolved {relative} using {name}");
                    }
                    Err(error) => {
                        self.status.text = format!("E: conflict resolved, reload failed: {error}");
                    }
                }
            }
            Err(error) => {
                if error.working_written {
                    let path = self.workspace.path.join(&relative);
                    let reload = self.reload_external(&path);
                    self.status.text = match reload {
                        Ok(()) => format!(
                            "E: git resolve: {} (working tree updated; index not staged)",
                            error.message
                        ),
                        Err(reload_error) => format!(
                            "E: git resolve: {} (working tree updated; reload failed: {reload_error})",
                            error.message
                        ),
                    };
                } else {
                    self.status.text = format!("E: git resolve: {}", error.message);
                }
            }
        }
        self.file.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = "\
diff --git a/a.rs b/a.rs
index 1111111..2222222 100644
--- a/a.rs
+++ b/a.rs
@@ -1 +1 @@
-one
+ONE
@@ -10 +10 @@
-ten
+TEN
";

    #[test]
    fn parses_hunks_with_file_header_and_view_lines() {
        let view = GitDiffView::parse(RAW, "a.rs".to_owned(), false, false);
        assert_eq!(view.hunks.len(), 2);
        assert_eq!(view.hunks[0].line, 6);
        assert_eq!(view.hunks[1].line, 9);
        assert!(view.hunks[0].patch.contains("-one\n+ONE\n"));
        assert!(!view.hunks[0].patch.contains("-ten"));
        assert!(view.hunks[1].patch.contains("--- a/a.rs"));
        assert!(view.hunks[1].patch.contains("-ten\n+TEN\n"));
        assert!(!view.hunks[1].patch.contains("-one"));
    }

    #[test]
    fn diff_without_hunks_has_no_staging_target() {
        let view = GitDiffView::parse(
            "diff --git a/a.bin b/a.bin\nBinary files differ\n",
            "a.bin".to_owned(),
            false,
            false,
        );
        assert!(view.hunks.is_empty());
    }

    #[test]
    fn recognizes_conflict_resolution_commands() {
        let mut model = Model::new(EditorState::from_text("x"), None);
        model.execute_command("git-resolve-ours");
        assert_eq!(model.status.text, "E: no file name");
        model.execute_command("git-resolve-theirs!");
        assert_eq!(model.status.text, "E: no file name");
    }

    #[test]
    fn reloads_working_file_when_staging_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("mkdir");
        let path = workspace.join("a.txt");
        std::fs::write(&path, "ours\n").expect("write");
        let mut model = Model::new(
            EditorState::from_text("<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> side\n"),
            Some(path),
        );
        model.workspace.path = workspace;
        model.git.resolve_pending = Some(7);
        model.handle_git_resolved(
            "a.txt".to_owned(),
            2,
            7,
            Err(crate::file_worker::ResolveConflictError {
                message: "git add failed".to_owned(),
                working_written: true,
            }),
        );
        assert_eq!(
            model
                .session
                .editor
                .text()
                .slice(0..model.session.editor.text().len_bytes()),
            "ours\n"
        );
        assert!(model.status.text.contains("working tree updated"));
    }
}

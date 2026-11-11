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
        if self.help_back.is_some() {
            if let Some(view) = self.git_diff_view.as_ref() {
                let relative = view.relative.clone();
                self.request_diff(relative, staged);
                return;
            }
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
        self.request_diff(relative, staged);
    }

    fn request_diff(&mut self, relative: String, staged: bool) {
        let Some(worker) = self.file_worker.as_ref() else {
            self.status = "E: file worker unavailable".to_owned();
            self.dirty = true;
            return;
        };
        self.git_diff_generation = self.git_diff_generation.wrapping_add(1);
        let generation = self.git_diff_generation;
        if let Err(error) = worker.request(FileRequest::GitDiffText {
            relative,
            staged,
            generation,
        }) {
            self.status = format!("E: git diff: {error}");
            self.dirty = true;
            return;
        }
        self.git_diff_pending = Some(generation);
        self.status = if staged {
            "loading staged git diff...".to_owned()
        } else {
            "loading git diff...".to_owned()
        };
        self.dirty = true;
    }

    pub(crate) fn handle_git_diff_text(
        &mut self,
        relative: String,
        staged: bool,
        generation: u64,
        result: Result<(String, bool), String>,
    ) {
        if self.git_diff_pending != Some(generation) {
            return;
        }
        self.git_diff_pending = None;
        match result {
            Ok((text, truncated)) => {
                let view = GitDiffView::parse(&text, relative, staged, truncated);
                self.git_diff_view = Some(view);
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
                if self.help_back.is_some() && self.read_only_title.as_deref() != Some("Git diff") {
                    self.git_diff_view = None;
                    return;
                }
                self.replace_read_only(&body, title, "Git diff");
            }
            Err(error) => {
                self.status = format!("E: git diff: {error}");
                self.dirty = true;
            }
        }
    }

    pub(crate) fn move_git_hunk(&mut self, forward: bool) {
        let Some(view) = self.git_diff_view.as_ref() else {
            self.status = "E: not viewing a git diff".to_owned();
            self.dirty = true;
            return;
        };
        if view.hunks.is_empty() {
            self.status = "E: diff has no hunks".to_owned();
            self.dirty = true;
            return;
        }
        let current = self
            .editor
            .index()
            .offset_to_line(self.cursor)
            .map(|line| line.line)
            .unwrap_or(0);
        let index = if forward {
            view.hunks.iter().position(|hunk| hunk.line > current)
        } else {
            view.hunks.iter().rposition(|hunk| hunk.line < current)
        };
        let Some(index) = index else {
            self.status = if forward {
                "last git hunk"
            } else {
                "first git hunk"
            }
            .to_owned();
            self.dirty = true;
            return;
        };
        let line = view.hunks[index].line;
        if let Some(offset) = self.editor.index().line_to_offset(line) {
            self.cursor = offset;
            self.editor.set_selection_bytes(offset, offset);
            self.status = format!("git hunk {}/{}", index + 1, view.hunks.len());
            self.dirty = true;
        }
    }

    pub(crate) fn apply_current_hunk(&mut self, unstage: bool) {
        let Some(view) = self.git_diff_view.as_ref() else {
            self.status = "E: not viewing a git diff".to_owned();
            self.dirty = true;
            return;
        };
        if view.truncated {
            self.status = "E: diff is truncated; hunk staging disabled".to_owned();
            self.dirty = true;
            return;
        }
        if unstage != view.staged {
            self.status = if unstage {
                "E: open :git-diff --staged to unstage".to_owned()
            } else {
                "E: open :git-diff to stage".to_owned()
            };
            self.dirty = true;
            return;
        }
        if view.hunks.is_empty() {
            self.status = "E: diff has no hunks".to_owned();
            self.dirty = true;
            return;
        }
        let current = self
            .editor
            .index()
            .offset_to_line(self.cursor)
            .map(|line| line.line)
            .unwrap_or(0);
        let Some(index) = view.hunks.iter().rposition(|hunk| hunk.line <= current) else {
            self.status = "E: cursor is before the first hunk".to_owned();
            self.dirty = true;
            return;
        };
        let patch = view.hunks[index].patch.clone();
        let Some(worker) = self.file_worker.as_ref() else {
            self.status = "E: file worker unavailable".to_owned();
            self.dirty = true;
            return;
        };
        self.git_apply_generation = self.git_apply_generation.wrapping_add(1);
        let generation = self.git_apply_generation;
        if let Err(error) = worker.request(FileRequest::GitApplyHunk {
            patch,
            reverse: unstage,
            generation,
        }) {
            self.status = format!("E: git apply: {error}");
            self.dirty = true;
            return;
        }
        self.git_apply_pending = Some(generation);
        self.status = if unstage {
            "unstaging hunk...".to_owned()
        } else {
            "staging hunk...".to_owned()
        };
        self.dirty = true;
    }

    pub(crate) fn handle_git_applied(&mut self, generation: u64, result: Result<(), String>) {
        if self.git_apply_pending != Some(generation) {
            return;
        }
        self.git_apply_pending = None;
        match result {
            Ok(()) => {
                self.status = "git hunk applied".to_owned();
                if let Some(view) = self.git_diff_view.as_ref() {
                    let relative = view.relative.clone();
                    let staged = view.staged;
                    self.request_diff(relative, staged);
                }
            }
            Err(error) => {
                self.status = format!("E: git apply: {error}");
                self.dirty = true;
            }
        }
    }
}

impl Model {
    pub(crate) fn request_git_blame(&mut self) {
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
        self.git_blame_generation = self.git_blame_generation.wrapping_add(1);
        let generation = self.git_blame_generation;
        if let Err(error) = worker.request(FileRequest::GitBlameText {
            relative,
            generation,
        }) {
            self.status = format!("E: git blame: {error}");
            self.dirty = true;
            return;
        }
        self.git_blame_pending = Some(generation);
        self.status = "loading git blame...".to_owned();
        self.dirty = true;
    }

    pub(crate) fn handle_git_blame_text(
        &mut self,
        generation: u64,
        result: Result<(String, bool), String>,
    ) {
        if self.git_blame_pending != Some(generation) {
            return;
        }
        self.git_blame_pending = None;
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
                self.status = format!("E: git blame: {error}");
                self.dirty = true;
            }
        }
    }
}

impl Model {
    pub(crate) fn request_git_conflicts(&mut self) {
        if self.help_back.is_some() {
            self.status = "already viewing a read-only view".to_owned();
            self.dirty = true;
            return;
        }
        let Some(worker) = self.file_worker.as_ref() else {
            self.status = "E: file worker unavailable".to_owned();
            self.dirty = true;
            return;
        };
        self.git_conflicts_generation = self.git_conflicts_generation.wrapping_add(1);
        let generation = self.git_conflicts_generation;
        if let Err(error) = worker.request(FileRequest::GitConflictsText { generation }) {
            self.status = format!("E: git conflicts: {error}");
            self.dirty = true;
            return;
        }
        self.git_conflicts_pending = Some(generation);
        self.status = "loading git conflicts...".to_owned();
        self.dirty = true;
    }

    pub(crate) fn handle_git_conflicts_text(
        &mut self,
        generation: u64,
        result: Result<(String, bool), String>,
    ) {
        if self.git_conflicts_pending != Some(generation) {
            return;
        }
        self.git_conflicts_pending = None;
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
                self.status = format!("E: git conflicts: {error}");
                self.dirty = true;
            }
        }
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
}

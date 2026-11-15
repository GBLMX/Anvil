//! Asynchronous Git gutter state.

use super::*;
use anvil_workspace::FileRequest;

impl Model {
    pub(crate) fn poll_git(&mut self) {
        if let Some(pending) = &self.workspace.git.diff_request
            && self.session.path.as_ref() != Some(&pending.0)
        {
            self.workspace.git.diff_request = None;
            self.workspace.git.diff_due = Some(Instant::now());
        }
        if self.file.modified {
            if !self.workspace.git.marks.is_empty() {
                self.workspace.git.marks.clear();
                self.file.dirty = true;
            }
            return;
        }
        let Some(path) = self.session.path.clone() else {
            return;
        };
        let revision = self.session.editor.revision();
        if self.workspace.git.diff_checked.as_ref() == Some(&(path.clone(), revision)) {
            return;
        }
        if self.workspace.git.diff_request.is_some() {
            return;
        }
        let now = Instant::now();
        let due = *self
            .workspace
            .git
            .diff_due
            .get_or_insert(now + Duration::from_millis(300));
        if now < due || !self.ensure_file_worker() {
            return;
        }
        let relative = match self.workspace.relative_path(&path) {
            Ok(relative) => relative,
            Err(_) => return,
        };
        let Some(worker) = self.file_worker_for_path(&path) else {
            return;
        };
        if worker
            .request(FileRequest::GitDiff {
                path: path.clone(),
                relative,
                revision,
            })
            .is_ok()
        {
            self.workspace.git.diff_request = Some((path.clone(), revision));
            self.workspace.git.diff_checked = Some((path, revision));
            self.workspace.git.diff_due = None;
        }
    }

    pub(crate) fn handle_git_diff(
        &mut self,
        path: PathBuf,
        revision: u64,
        result: Result<Vec<(usize, char)>, String>,
    ) {
        if self.session.path.as_ref() != Some(&path) || self.session.editor.revision() != revision {
            self.workspace.git.diff_request = None;
            self.workspace.git.marks.clear();
            self.workspace.git.diff_due = Some(Instant::now());
            self.file.dirty = true;
            return;
        }
        self.workspace.git.diff_request = None;
        self.workspace.git.marks = result.unwrap_or_default().into_iter().collect();
        self.file.dirty = true;
    }

    pub(crate) fn git_marker(&self, line: usize) -> Option<char> {
        self.workspace.git.marks.get(&line).copied()
    }
}

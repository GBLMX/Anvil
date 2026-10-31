//! Asynchronous Git gutter state.

use super::*;
use crate::file_worker::FileRequest;

impl Model {
    pub(crate) fn poll_git(&mut self) {
        if let Some(pending) = &self.git_pending
            && self.path.as_ref() != Some(&pending.0)
        {
            self.git_pending = None;
            self.git_due = Some(Instant::now());
        }
        if self.modified {
            if !self.git_marks.is_empty() {
                self.git_marks.clear();
                self.dirty = true;
            }
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        let revision = self.editor.revision();
        if self.git_checked.as_ref() == Some(&(path.clone(), revision)) {
            return;
        }
        if self.git_pending.is_some() {
            return;
        }
        let now = Instant::now();
        let due = *self.git_due.get_or_insert(now + Duration::from_millis(300));
        if now < due || !self.ensure_file_worker() {
            return;
        }
        let relative = match self.relative_path(&path) {
            Ok(relative) => relative,
            Err(_) => return,
        };
        let Some(worker) = self.file_worker.as_ref() else {
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
            self.git_pending = Some((path.clone(), revision));
            self.git_checked = Some((path, revision));
            self.git_due = None;
        }
    }

    pub(crate) fn handle_git_diff(
        &mut self,
        path: PathBuf,
        revision: u64,
        result: Result<Vec<(usize, char)>, String>,
    ) {
        if self.path.as_ref() != Some(&path) || self.editor.revision() != revision {
            self.git_pending = None;
            self.git_marks.clear();
            self.git_due = Some(Instant::now());
            self.dirty = true;
            return;
        }
        self.git_pending = None;
        self.git_marks = result.unwrap_or_default().into_iter().collect();
        self.dirty = true;
    }

    pub(crate) fn git_marker(&self, line: usize) -> Option<char> {
        self.git_marks.get(&line).copied()
    }
}

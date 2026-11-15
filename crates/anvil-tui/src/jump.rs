//! Jump and change navigation over frontend-neutral session history.

use super::*;

pub(crate) use anvil_session::jump::{JumpEntry, JumpList};

impl Model {
    pub(crate) fn record_jump(&mut self) {
        let Some(entry) = self.current_jump_entry() else {
            return;
        };
        self.session.jump_list.record(entry);
    }

    pub(crate) fn jump_back(&mut self) {
        self.jump_history(false);
    }

    pub(crate) fn jump_forward(&mut self) {
        self.jump_history(true);
    }

    fn jump_history(&mut self, forward: bool) {
        if self.session.path.is_none() && self.file.modified {
            self.status.text = "E: save or discard the unnamed buffer before jumping".to_owned();
            self.file.dirty = true;
            return;
        }
        let current = self.current_jump_entry();
        let target = if forward {
            self.session.jump_list.take_forward()
        } else {
            self.session.jump_list.take_back()
        };
        let Some(target) = target else {
            self.status.text = if forward {
                "no forward jump".to_owned()
            } else {
                "no back jump".to_owned()
            };
            self.file.dirty = true;
            return;
        };
        if !self.jump_to(&target) {
            if forward {
                self.session.jump_list.push_forward(target);
            } else {
                self.session.jump_list.push_back(target);
            }
            return;
        }
        if let Some(current) = current {
            if forward {
                self.session.jump_list.push_back(current);
            } else {
                self.session.jump_list.push_forward(current);
            }
        }
        self.status.text = format!(
            "{} [{}|{}]",
            if forward { "jump forward" } else { "jump back" },
            self.session.jump_list.back_len(),
            self.session.jump_list.forward_len()
        );
        self.file.dirty = true;
    }

    fn current_jump_entry(&self) -> Option<JumpEntry> {
        Some(JumpEntry {
            path: self.session.path.clone()?,
            cursor: self.session.cursor,
        })
    }

    fn jump_to(&mut self, entry: &JumpEntry) -> bool {
        if self.pager.back.is_some() {
            self.close_help();
        }
        if self.session.path.as_ref() != Some(&entry.path)
            && let Err(error) = self.open_path(&entry.path)
        {
            self.status.text = format!("E: {error}");
            self.file.dirty = true;
            return false;
        }
        let length = self.session.editor.text().len_bytes();
        self.session.cursor = entry.cursor.min(length);
        let _ = self
            .session
            .editor
            .try_set_selection_bytes(self.session.cursor, self.session.cursor);
        self.file.dirty = true;
        true
    }

    pub(crate) fn record_change(
        &mut self,
        before_path: Option<PathBuf>,
        before_cursor: usize,
        after_path: Option<PathBuf>,
        after_cursor: usize,
    ) {
        if let Some(path) = before_path {
            self.session.change_list.record(JumpEntry {
                path,
                cursor: before_cursor,
            });
        }
        if let Some(path) = after_path {
            self.session.change_list.record(JumpEntry {
                path,
                cursor: after_cursor,
            });
        }
    }

    pub(crate) fn change_back(&mut self) {
        self.change_history(false);
    }

    pub(crate) fn change_forward(&mut self) {
        self.change_history(true);
    }

    fn change_history(&mut self, forward: bool) {
        if self.session.path.is_none() && self.file.modified {
            self.status.text = "E: save or discard the unnamed buffer before jumping".to_owned();
            self.file.dirty = true;
            return;
        }
        let current = self.current_jump_entry();
        let target = if forward {
            self.session.change_list.take_forward()
        } else {
            self.session.change_list.take_back()
        };
        let Some(mut target) = target else {
            self.status.text = if forward {
                "no next change".to_owned()
            } else {
                "no previous change".to_owned()
            };
            self.file.dirty = true;
            return;
        };
        if current.as_ref() == Some(&target) {
            let Some(next) = (if forward {
                self.session.change_list.take_forward()
            } else {
                self.session.change_list.take_back()
            }) else {
                self.status.text = if forward {
                    "no next change".to_owned()
                } else {
                    "no previous change".to_owned()
                };
                self.file.dirty = true;
                return;
            };
            target = next;
        }
        if !self.jump_to(&target) {
            if forward {
                self.session.change_list.push_forward(target);
            } else {
                self.session.change_list.push_back(target);
            }
            return;
        }
        if let Some(current) = current {
            if forward {
                self.session.change_list.push_back(current);
            } else {
                self.session.change_list.push_forward(current);
            }
        }
        self.status.text = format!(
            "{} [{}|{}]",
            if forward {
                "next change"
            } else {
                "previous change"
            },
            self.session.change_list.back_len(),
            self.session.change_list.forward_len()
        );
        self.file.dirty = true;
    }
}

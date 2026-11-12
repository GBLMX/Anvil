//! Bounded back/forward jump history.

use super::*;
use std::collections::VecDeque;
use std::path::PathBuf;

pub(crate) const MAX_JUMP_ENTRIES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JumpEntry {
    pub path: PathBuf,
    pub cursor: usize,
}

#[derive(Default)]
pub(crate) struct JumpList {
    back: VecDeque<JumpEntry>,
    forward: VecDeque<JumpEntry>,
}

impl JumpList {
    pub(crate) fn record(&mut self, entry: JumpEntry) {
        if self.back.back() == Some(&entry) {
            self.forward.clear();
            return;
        }
        push_bounded(&mut self.back, entry);
        self.forward.clear();
    }

    pub(crate) fn take_back(&mut self) -> Option<JumpEntry> {
        self.back.pop_back()
    }

    pub(crate) fn take_forward(&mut self) -> Option<JumpEntry> {
        self.forward.pop_back()
    }

    pub(crate) fn push_back(&mut self, entry: JumpEntry) {
        push_bounded(&mut self.back, entry);
    }

    pub(crate) fn push_forward(&mut self, entry: JumpEntry) {
        push_bounded(&mut self.forward, entry);
    }

    pub(crate) fn back_len(&self) -> usize {
        self.back.len()
    }

    pub(crate) fn forward_len(&self) -> usize {
        self.forward.len()
    }
}

fn push_bounded(stack: &mut VecDeque<JumpEntry>, entry: JumpEntry) {
    if stack.len() >= MAX_JUMP_ENTRIES {
        stack.pop_front();
    }
    stack.push_back(entry);
}

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
        if self.buffers.help_back.is_some() {
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
        self.session
            .editor
            .set_selection_bytes(self.session.cursor, self.session.cursor);
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
#[cfg(test)]
mod tests {
    use super::*;

    fn entry(cursor: usize) -> JumpEntry {
        JumpEntry {
            path: PathBuf::from("main.rs"),
            cursor,
        }
    }

    #[test]
    fn records_back_entries_and_bounds_the_stack() {
        let mut list = JumpList::default();
        for cursor in 0..MAX_JUMP_ENTRIES + 10 {
            list.record(entry(cursor));
        }
        assert_eq!(list.back_len(), MAX_JUMP_ENTRIES);
    }

    #[test]
    fn taking_back_then_forward_round_trips() {
        let mut list = JumpList::default();
        list.record(entry(1));
        let target = list.take_back().expect("back");
        list.push_forward(entry(2));
        assert_eq!(list.back_len(), 0);
        assert_eq!(list.forward_len(), 1);
        assert_eq!(target, entry(1));
        assert_eq!(list.take_forward(), Some(entry(2)));
    }

    #[test]
    fn a_new_jump_clears_the_forward_stack() {
        let mut list = JumpList::default();
        list.record(entry(1));
        list.take_back();
        list.push_forward(entry(2));
        list.record(entry(3));
        assert_eq!(list.forward_len(), 0);
        assert_eq!(list.back_len(), 1);
    }
}

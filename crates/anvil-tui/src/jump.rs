//! Jump and change navigation over frontend-neutral session history.

use super::*;

pub(crate) use anvil_session::jump::{JumpEntry, JumpList};
use anvil_session::provider::{CancellationToken, PickerEntry};

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

    pub(crate) fn open_jump_picker(&mut self) {
        self.cancel_completion();
        let mut entries = Vec::new();
        let mut targets = Vec::new();
        for (index, entry) in self.session.jump_list.back_entries().enumerate() {
            let picker_index = targets.len();
            targets.push((false, index));
            entries.push(jump_picker_entry(
                &self.workspace.path,
                false,
                picker_index,
                entry,
            ));
        }
        for (index, entry) in self.session.jump_list.forward_entries().enumerate() {
            let picker_index = targets.len();
            targets.push((true, index));
            entries.push(jump_picker_entry(
                &self.workspace.path,
                true,
                picker_index,
                entry,
            ));
        }
        if entries.is_empty() {
            self.status.text = "no jumps".to_owned();
            self.file.dirty = true;
            return;
        }
        self.overlays.buffer_picker = Some(BufferPickerState {
            query: String::new(),
            filtered: (0..entries.len()).collect(),
            entries,
            selected: 0,
            cancel: CancellationToken::new(),
            mode: BufferPickerMode::Jumps { targets },
        });
        self.push_overlay(OverlayKind::BufferPicker);
        self.status.text = "jump list".to_owned();
        self.file.dirty = true;
    }

    fn jump_history(&mut self, forward: bool) {
        self.jump_history_to(forward, 0);
    }

    pub(crate) fn jump_history_to(&mut self, forward: bool, index: usize) {
        if self.session.path.is_none() && self.file.modified {
            self.status.text = "E: save or discard the unnamed buffer before jumping".to_owned();
            self.file.dirty = true;
            return;
        }
        let current = self.current_jump_entry();
        let popped = if forward {
            self.session.jump_list.take_forward_to(index)
        } else {
            self.session.jump_list.take_back_to(index)
        };
        let Some(popped) = popped else {
            self.status.text = if forward {
                "no forward jump".to_owned()
            } else {
                "no back jump".to_owned()
            };
            self.file.dirty = true;
            return;
        };
        let target = popped.last().cloned().expect("jump target");
        if !self.jump_to(&target) {
            for entry in popped.into_iter().rev() {
                if forward {
                    self.session.jump_list.push_forward(entry);
                } else {
                    self.session.jump_list.push_back(entry);
                }
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
        let skipped = popped.len().saturating_sub(1);
        for entry in popped.into_iter().take(skipped) {
            if forward {
                self.session.jump_list.push_back(entry);
            } else {
                self.session.jump_list.push_forward(entry);
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

fn jump_picker_entry(root: &Path, forward: bool, index: usize, entry: &JumpEntry) -> PickerEntry {
    let relative = entry.path.strip_prefix(root).unwrap_or(&entry.path);
    let display = format!(
        "{} {} @ {}",
        if forward { "→" } else { "←" },
        relative.display(),
        entry.cursor
    );
    PickerEntry {
        value: serde_json::Value::from(index as u64),
        ordinal: display.to_lowercase(),
        display,
    }
}

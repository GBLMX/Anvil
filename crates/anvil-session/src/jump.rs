//! Bounded back/forward jump and change history.

use std::collections::VecDeque;
use std::path::PathBuf;

/// Maximum retained entries in one direction.
pub const MAX_JUMP_ENTRIES: usize = 256;

/// One jump/change destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JumpEntry {
    /// File identity at the destination.
    pub path: PathBuf,
    /// Byte offset in that file.
    pub cursor: usize,
}

/// Bounded back/forward stack.
#[derive(Default)]
pub struct JumpList {
    back: VecDeque<JumpEntry>,
    forward: VecDeque<JumpEntry>,
}

impl JumpList {
    /// Record a new destination and clear forward history.
    pub fn record(&mut self, entry: JumpEntry) {
        if self.back.back() == Some(&entry) {
            self.forward.clear();
            return;
        }
        push_bounded(&mut self.back, entry);
        self.forward.clear();
    }

    /// Pop the latest back entry.
    pub fn take_back(&mut self) -> Option<JumpEntry> {
        self.back.pop_back()
    }

    /// Pop the latest forward entry.
    pub fn take_forward(&mut self) -> Option<JumpEntry> {
        self.forward.pop_back()
    }

    /// Push a back entry without clearing forward history.
    pub fn push_back(&mut self, entry: JumpEntry) {
        push_bounded(&mut self.back, entry);
    }

    /// Push a forward entry without clearing back history.
    pub fn push_forward(&mut self, entry: JumpEntry) {
        push_bounded(&mut self.forward, entry);
    }

    /// Number of back entries.
    pub fn back_len(&self) -> usize {
        self.back.len()
    }

    /// Number of forward entries.
    pub fn forward_len(&self) -> usize {
        self.forward.len()
    }
}

fn push_bounded(stack: &mut VecDeque<JumpEntry>, entry: JumpEntry) {
    if stack.len() >= MAX_JUMP_ENTRIES {
        stack.pop_front();
    }
    stack.push_back(entry);
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
        assert_eq!(list.take_back(), Some(entry(MAX_JUMP_ENTRIES + 9)));
    }

    #[test]
    fn duplicate_record_clears_forward_without_growing() {
        let mut list = JumpList::default();
        list.record(entry(1));
        list.push_forward(entry(2));
        list.record(entry(1));
        assert_eq!(list.back_len(), 1);
        assert_eq!(list.forward_len(), 0);
    }

    #[test]
    fn forward_stack_is_bounded() {
        let mut list = JumpList::default();
        for cursor in 0..MAX_JUMP_ENTRIES + 1 {
            list.push_forward(entry(cursor));
        }
        assert_eq!(list.forward_len(), MAX_JUMP_ENTRIES);
        assert_eq!(list.take_forward(), Some(entry(MAX_JUMP_ENTRIES)));
    }
}

//! Frontend-neutral focus and overlay state.

use std::path::PathBuf;

/// Focus and overlay state owned by a frontend session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiState {
    pub focus: FocusState,
    pub notifications: NotificationQueue,
    pub progress: ProgressState,
    pub problems: ProblemsState,
    pub diagnostic_peek: Option<DiagnosticPeek>,
    pub input_mode: InputMode,
}

/// Which surface currently owns keyboard focus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FocusState {
    /// The workspace tree owns focus.
    pub tree: bool,
    /// The right-hand split pane owns focus.
    pub split: bool,
}

/// Kind of entry created from the workspace tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateKind {
    File,
    Directory,
}

/// The small frontend-neutral input and overlay state machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum InputMode {
    /// Navigation, insert entry, command entry.
    #[default]
    Normal,
    /// Text insertion at the cursor.
    Insert,
    /// Rename symbol input.
    Rename { name: String },
    /// Ex-style command line.
    CommandLine { buffer: String },
    /// Incremental substring search.
    Search { query: String },
    /// Create a file or directory in the tree.
    Create {
        directory: PathBuf,
        name: String,
        kind: CreateKind,
    },
    /// Rename a tree entry.
    RenameEntry { path: PathBuf, name: String },
    /// Confirm deletion of a tree entry.
    ConfirmDelete { path: PathBuf },
    /// Confirm closing a modified buffer.
    ConfirmClose { index: usize },
    /// Confirm reloading a dirty buffer changed on disk.
    ConfirmReload { path: PathBuf },
    /// Move or copy a tree entry into a destination directory.
    TransferEntry {
        path: PathBuf,
        copy: bool,
        directory: String,
    },
}

pub const NOTIFICATION_CAPACITY: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub severity: NotificationSeverity,
    pub source: String,
    pub message: String,
    pub timestamp: u64,
    pub count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationQueue {
    items: Vec<Notification>,
}

impl NotificationQueue {
    pub fn push(
        &mut self,
        severity: NotificationSeverity,
        source: impl Into<String>,
        message: impl Into<String>,
        timestamp: u64,
    ) {
        let source = source.into();
        let message = message.into();
        if let Some(existing) = self.items.iter_mut().find(|item| {
            item.severity == severity && item.source == source && item.message == message
        }) {
            existing.timestamp = timestamp;
            existing.count = existing.count.saturating_add(1);
            return;
        }
        if self.items.len() >= NOTIFICATION_CAPACITY {
            self.items.remove(0);
        }
        self.items.push(Notification {
            severity,
            source,
            message,
            timestamp,
            count: 1,
        });
    }

    pub fn items(&self) -> &[Notification] {
        &self.items
    }

    pub fn dismiss(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressItem {
    pub id: u64,
    pub title: String,
    pub message: String,
    pub fraction: Option<u8>,
    pub cancellable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProgressState {
    items: Vec<ProgressItem>,
    next_id: u64,
}

impl ProgressState {
    pub fn start(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        cancellable: bool,
    ) -> u64 {
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        self.items.push(ProgressItem {
            id,
            title: title.into(),
            message: message.into(),
            fraction: None,
            cancellable,
        });
        id
    }

    pub fn update(&mut self, id: u64, fraction: Option<u8>, message: impl Into<String>) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.fraction = fraction.map(|value| value.min(100));
            item.message = message.into();
        }
    }

    pub fn finish(&mut self, id: u64) -> bool {
        let Some(index) = self.items.iter().position(|item| item.id == id) else {
            return false;
        };
        self.items.remove(index);
        true
    }

    pub fn cancel(&mut self, id: u64) -> bool {
        let Some(index) = self
            .items
            .iter()
            .position(|item| item.id == id && item.cancellable)
        else {
            return false;
        };
        self.items.remove(index);
        true
    }

    pub fn items(&self) -> &[ProgressItem] {
        &self.items
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub path: PathBuf,
    pub line: u32,
    pub character: u32,
    pub severity: Option<u32>,
    pub source: Option<String>,
    pub code: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProblemsState {
    pub visible: bool,
    pub selected: usize,
    pub items: Vec<Problem>,
}

impl ProblemsState {
    pub fn replace(&mut self, mut items: Vec<Problem>) {
        items.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.line.cmp(&right.line))
                .then(left.character.cmp(&right.character))
                .then(left.severity.cmp(&right.severity))
        });
        self.items = items;
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.items.is_empty() {
            self.selected = 0;
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.items.len() - 1);
    }

    pub fn selected(&self) -> Option<&Problem> {
        self.items.get(self.selected)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeekDiagnostic {
    pub severity: Option<u32>,
    pub source: Option<String>,
    pub code: Option<String>,
    pub message: String,
    pub related: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticPeek {
    pub path: PathBuf,
    pub revision: u64,
    pub line: u32,
    pub character: u32,
    pub focus: FocusState,
    pub diagnostics: Vec<PeekDiagnostic>,
    pub selected: usize,
    pub line_count: usize,
    pub scroll: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_deduplicate_and_cap() {
        let mut queue = NotificationQueue::default();
        queue.push(NotificationSeverity::Info, "test", "same", 1);
        queue.push(NotificationSeverity::Info, "test", "same", 2);
        assert_eq!(queue.items().len(), 1);
        assert_eq!(queue.items()[0].count, 2);
        assert_eq!(queue.items()[0].timestamp, 2);
        for index in 0..NOTIFICATION_CAPACITY {
            queue.push(
                NotificationSeverity::Warning,
                "test",
                format!("message {index}"),
                index as u64,
            );
        }
        assert_eq!(queue.items().len(), NOTIFICATION_CAPACITY);
        assert_eq!(queue.items().last().expect("last").message, "message 49");
    }

    #[test]
    fn progress_finish_and_cancel_remove_items() {
        let mut progress = ProgressState::default();
        let first = progress.start("copy", "starting", false);
        let second = progress.start("exec", "running", true);
        assert!(!progress.cancel(first));
        assert!(progress.finish(first));
        assert_eq!(progress.items().len(), 1);
        assert!(progress.cancel(second));
        assert!(progress.items().is_empty());
    }

    #[test]
    fn problems_sort_and_clamp_selection() {
        let mut problems = ProblemsState::default();
        problems.replace(vec![
            Problem {
                path: PathBuf::from("b.rs"),
                line: 1,
                character: 0,
                severity: Some(1),
                source: None,
                code: None,
                message: "b".to_owned(),
            },
            Problem {
                path: PathBuf::from("a.rs"),
                line: 2,
                character: 0,
                severity: Some(2),
                source: None,
                code: None,
                message: "a".to_owned(),
            },
        ]);
        assert_eq!(problems.items[0].path, PathBuf::from("a.rs"));
        problems.selected = 9;
        problems.move_selection(1);
        assert_eq!(problems.selected, 1);
    }
}

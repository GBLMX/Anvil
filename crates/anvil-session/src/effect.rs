//! Bounded typed effects for frontend-neutral command execution.

use crate::ui::CreateKind;
use std::collections::VecDeque;
use std::path::PathBuf;

/// A side effect requested by a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Write the active buffer to its current path.
    Save,
    /// Write the active buffer to a new path.
    SaveAs(PathBuf),
    /// Write every modified buffer.
    SaveAll,
    /// Open a file in the session.
    Open(PathBuf),
    /// Request completion at the cursor.
    LspCompletion,
    /// Request signature help after a trigger character.
    LspSignatureHelp(char),
    /// Request definition.
    LspDefinition,
    /// Request hover.
    LspHover,
    /// Request code actions.
    LspCodeActions,
    /// Request document symbols.
    LspDocumentSymbols,
    /// Request workspace symbols.
    LspWorkspaceSymbols,
    /// Rename the symbol at the cursor.
    LspRename(String),
    /// Request formatting for the active buffer.
    LspFormat,
    /// Create a file or directory from the tree.
    TreeCreate {
        directory: PathBuf,
        name: String,
        kind: CreateKind,
    },
    /// Rename a tree entry.
    TreeRename { path: PathBuf, name: String },
    /// Move or copy a tree entry.
    TreeTransfer {
        path: PathBuf,
        copy: bool,
        directory: String,
    },
    /// Delete a tree entry.
    TreeDelete(PathBuf),
    /// Save a buffer, optionally closing it after a successful write.
    SaveBuffer { index: usize, close: bool },
    /// Save, then quit if no other buffers remain dirty.
    SaveAndQuit,
    /// Save to a path, then quit if no other buffers remain dirty.
    SaveAsAndQuit(PathBuf),
    /// Reload a buffer after an external change.
    Reload(PathBuf),
    /// Reconnect the gateway.
    ReconnectGateway,
    /// Start the active language server.
    LspStart,
    /// Stop the active language server.
    LspStop,
    /// Start the default plugin host.
    PluginStart,
    /// List discovered plugins.
    PluginList,
    /// Start a plugin host with the OOM probe.
    PluginOom,
    /// Execute one request on the debug plugin host.
    PluginExec,
    /// Stop the plugin host.
    PluginStop,
    /// Re-enable a disabled plugin.
    PluginEnable(String),
}

/// One queued effect with its request id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedEffect {
    pub id: u64,
    pub effect: Effect,
}

/// Bounded FIFO queue. Overflow is explicit and never drops silently.
#[derive(Debug)]
pub struct EffectQueue {
    capacity: usize,
    next_id: u64,
    items: VecDeque<QueuedEffect>,
}

impl EffectQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            next_id: 1,
            items: VecDeque::new(),
        }
    }

    pub fn push(&mut self, effect: Effect) -> Result<u64, Effect> {
        if self.items.len() >= self.capacity {
            return Err(effect);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.items.push_back(QueuedEffect { id, effect });
        Ok(id)
    }

    pub fn pop(&mut self) -> Option<QueuedEffect> {
        self.items.pop_front()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_assigns_request_ids_and_preserves_order() {
        let mut queue = EffectQueue::new(4);
        assert_eq!(queue.push(Effect::Save), Ok(1));
        assert_eq!(queue.push(Effect::Open(PathBuf::from("a.txt"))), Ok(2));
        assert_eq!(
            queue.pop(),
            Some(QueuedEffect {
                id: 1,
                effect: Effect::Save
            })
        );
        assert_eq!(
            queue.pop(),
            Some(QueuedEffect {
                id: 2,
                effect: Effect::Open(PathBuf::from("a.txt"))
            })
        );
        assert!(queue.is_empty());
    }

    #[test]
    fn overflow_returns_the_rejected_effect() {
        let mut queue = EffectQueue::new(1);
        assert_eq!(queue.push(Effect::Save), Ok(1));
        assert_eq!(queue.push(Effect::SaveAll), Err(Effect::SaveAll));
        assert_eq!(queue.len(), 1);
    }
}

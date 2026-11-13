//! Workspace-index extension-point types.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use anvil_ipc::GatewayClient;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceIndexSnapshot {
    pub generation: u64,
    pub paths: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceIndexError {
    Cancelled,
    Stale,
    Io(String),
}

impl std::fmt::Display for WorkspaceIndexError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("workspace index cancelled"),
            Self::Stale => formatter.write_str("stale workspace index generation"),
            Self::Io(error) => formatter.write_str(error),
        }
    }
}

impl std::error::Error for WorkspaceIndexError {}

/// UI-neutral workspace index service.
pub trait WorkspaceIndexService: Send + Sync {
    fn build(
        &self,
        client: &mut GatewayClient,
        root: &Path,
        hidden: bool,
        generation: u64,
        cancel: &AtomicBool,
    ) -> Result<WorkspaceIndexSnapshot, WorkspaceIndexError>;

    fn search(&self, root: &Path, query: &str, limit: usize) -> Vec<String>;
}

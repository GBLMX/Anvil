//! Git extension-point types shared by the module contract and implementations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anvil_ipc::GatewayClient;

/// Failure while resolving a conflict, including whether the working file was
/// already replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveConflictError {
    pub message: String,
    pub working_written: bool,
}

impl ResolveConflictError {
    pub fn before_write(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            working_written: false,
        }
    }

    pub fn after_write(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            working_written: true,
        }
    }
}

/// UI-neutral Git operations consumed by the file worker.
pub trait GitService: Send + Sync {
    fn git_status(
        &self,
        client: &mut GatewayClient,
        root: &Path,
    ) -> Result<HashMap<PathBuf, char>, String>;

    fn git_diff(
        &self,
        client: &mut GatewayClient,
        relative: &str,
    ) -> Result<Vec<(usize, char)>, String>;

    fn git_diff_text(
        &self,
        client: &mut GatewayClient,
        relative: &str,
        staged: bool,
    ) -> Result<(String, bool), String>;

    fn git_blame_text(
        &self,
        client: &mut GatewayClient,
        relative: &str,
    ) -> Result<(String, bool), String>;

    fn git_conflicts_text(&self, client: &mut GatewayClient) -> Result<(String, bool), String>;

    fn git_conflict_stage_text(
        &self,
        client: &mut GatewayClient,
        relative: &str,
        stage: u8,
    ) -> Result<(String, bool), String>;

    fn resolve_git_conflict(
        &self,
        client: &mut GatewayClient,
        relative: &str,
        stage: u8,
    ) -> Result<(), ResolveConflictError>;

    fn apply_git_hunk(
        &self,
        client: &mut GatewayClient,
        patch: &str,
        reverse: bool,
    ) -> Result<(), String>;
}

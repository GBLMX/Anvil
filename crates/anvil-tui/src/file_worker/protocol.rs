//! Background file-worker request and event types.

use std::collections::HashMap;
use std::path::PathBuf;

use anvil_ipc::DirEntry;

use super::PreviewData;
use crate::pipe_locations::PipeLocation;

use anvil_module::git::ResolveConflictError;

/// One file in a project replace preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectReplaceFile {
    pub(crate) relative: String,
    pub(crate) matches: usize,
    pub(crate) hash: [u8; 32],
}

/// A reviewable project replace plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectReplacePlan {
    pub(crate) old: String,
    pub(crate) new: String,
    pub(crate) files: Vec<ProjectReplaceFile>,
}

/// A background file-tree request.
pub enum FileRequest {
    /// List one directory.
    List(PathBuf),
    /// List one directory for command-line path completion.
    ListCompletion { path: PathBuf, generation: u64 },
    /// Enumerate workspace files with `rg --files`, or a bounded fallback.
    Files {
        hidden: bool,
        generation: u64,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    },
    /// Run a bounded ripgrep project search.
    ProjectSearch { query: String, generation: u64 },
    /// Read a bounded preview for one workspace-relative file.
    Preview { path: String, generation: u64 },
    /// Read workspace git status for tree decorations.
    GitStatus { root: PathBuf, generation: u64 },
    /// Compare one workspace file with HEAD for gutter marks.
    GitDiff {
        path: PathBuf,
        relative: String,
        revision: u64,
    },
    /// Read a bounded unified diff for one workspace-relative file.
    GitDiffText {
        relative: String,
        staged: bool,
        generation: u64,
    },
    /// Apply one hunk patch to the index.
    GitApplyHunk {
        patch: String,
        reverse: bool,
        generation: u64,
    },
    /// List unmerged paths in the index for one workspace root.
    GitConflictsText { root: PathBuf, generation: u64 },
    /// Read one index stage for a conflicted workspace file.
    GitConflictStageText {
        relative: String,
        stage: u8,
        generation: u64,
    },
    /// Resolve a conflicted file to one index stage and stage it.
    GitResolveConflict {
        relative: String,
        stage: u8,
        generation: u64,
    },
    /// Read a bounded blame view for one workspace-relative file.
    GitBlameText { relative: String, generation: u64 },
    /// Move or copy one workspace entry.
    Transfer {
        from: PathBuf,
        to: PathBuf,
        copy: bool,
    },
    /// Delete one workspace entry or directory tree.
    RemoveAll(PathBuf),
    /// Replace the workspace-tree directory watch set.
    WatchDirs(Vec<PathBuf>),
    /// Capture root and expanded directory listings for a tree refresh.
    TreeSnapshot {
        generation: u64,
        root: PathBuf,
        expanded: Vec<PathBuf>,
    },
    /// Watch open buffers for external metadata changes.
    Watch(Vec<PathBuf>),
    /// Build a project replace preview.
    ProjectReplacePreview {
        old: String,
        new: String,
        generation: u64,
    },
    /// Apply a reviewed project replace plan.
    ProjectReplaceApply {
        plan: ProjectReplacePlan,
        generation: u64,
    },
}

/// A background file-tree result.
pub enum FileEvent {
    /// Directory listing for the requested path.
    Listed(PathBuf, Result<Vec<DirEntry>, String>),
    /// Directory listing for command-line path completion.
    CompletionListed {
        generation: u64,
        result: Result<Vec<DirEntry>, String>,
    },
    /// Workspace-relative files in sorted order for one request generation.
    Files {
        generation: u64,
        result: Result<Vec<String>, String>,
    },
    /// Bounded project search results.
    ProjectSearch {
        query: String,
        generation: u64,
        result: Result<(Vec<PipeLocation>, bool), String>,
    },
    /// Bounded preview result for one workspace-relative file.
    Preview {
        path: String,
        generation: u64,
        result: Result<PreviewData, String>,
    },
    /// Git gutter marks keyed by zero-based line.
    GitDiff {
        path: PathBuf,
        revision: u64,
        result: Result<Vec<(usize, char)>, String>,
    },
    /// Bounded unified diff text and whether it was truncated.
    GitDiffText {
        relative: String,
        staged: bool,
        generation: u64,
        result: Result<(String, bool), String>,
    },
    /// Result of applying one hunk patch to the index.
    GitApplied {
        generation: u64,
        result: Result<(), String>,
    },
    /// Bounded blame text and whether it was truncated.
    GitBlameText {
        generation: u64,
        result: Result<(String, bool), String>,
    },
    /// Bounded unmerged-path list for one workspace root.
    GitConflictsText {
        root: PathBuf,
        generation: u64,
        result: Result<(String, bool), String>,
    },
    /// Bounded index-stage text for a conflicted file.
    GitConflictStageText {
        relative: String,
        stage: u8,
        generation: u64,
        result: Result<(String, bool), String>,
    },
    /// Result of resolving and staging one conflicted file.
    GitResolveConflict {
        relative: String,
        stage: u8,
        generation: u64,
        result: Result<(), ResolveConflictError>,
    },
    /// Project replace preview result.
    ProjectReplacePreview {
        generation: u64,
        result: Result<ProjectReplacePlan, String>,
    },
    /// Project replace apply result.
    ProjectReplaceApplied {
        generation: u64,
        result: Result<(), String>,
    },
    /// A move or copy completed.
    Transferred {
        from: PathBuf,
        to: PathBuf,
        copy: bool,
        result: Result<(), String>,
    },
    /// A recursive delete completed.
    Removed {
        path: PathBuf,
        result: Result<(), String>,
    },
    /// A watched file changed on disk.
    ExternalChanged { path: PathBuf },
    /// One or more watched directories changed; refresh once.
    TreeChanged { paths: Vec<PathBuf> },
    /// Directory watching is unsupported or failed; the tree is stale.
    TreeWatchUnavailable(String),
    /// Listings for a generation-scoped tree refresh.
    TreeSnapshot {
        generation: u64,
        listings: Vec<(PathBuf, Result<Vec<DirEntry>, String>)>,
    },
    /// Workspace git status keyed by absolute path.
    GitStatus {
        generation: u64,
        result: Result<HashMap<PathBuf, char>, String>,
    },
}

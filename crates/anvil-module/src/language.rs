//! Language-tooling extension-point types.

use std::path::Path;

/// UI-neutral description of one language-server launch profile.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageServerSpec {
    pub tool: String,
    pub args: Vec<String>,
    pub language_id: String,
    pub root_markers: Vec<String>,
    pub label: String,
    pub initialization_options: Option<serde_json::Value>,
}

/// UI-neutral language-tooling provider.
pub trait LanguageToolingService: Send + Sync {
    fn server_for(&self, path: &Path) -> Option<LanguageServerSpec>;
}

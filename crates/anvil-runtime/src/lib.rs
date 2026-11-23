//! UI-neutral composition root for project discovery, integration grants, and
//! the workspace symbol index.

use anvil_integrations::{
    Authorization, Capability, Denial, Grant, GrantStore, IntegrationDescriptor, authorize,
};
use anvil_mod_workspace_index::{SymbolIndexCaps, WorkspaceSymbolIndex};
pub use anvil_mod_workspace_index::{
    SymbolKind, SymbolOccurrence, SymbolRole, TextPosition, TextRange, WorkspaceSymbol,
};
use anvil_project::Project;
use std::path::{Path, PathBuf};

/// Runtime services shared by the TUI and future frontends.
pub struct Runtime {
    project: Project,
    workspace_root: PathBuf,
    workspace_trusted: bool,
    grants: GrantStore,
    symbols: WorkspaceSymbolIndex,
}

impl Runtime {
    /// Compose an already discovered project.
    pub fn new(project: Project, symbol_caps: SymbolIndexCaps) -> Result<Self, String> {
        let workspace_root = project
            .roots
            .first()
            .cloned()
            .ok_or_else(|| "project has no roots".to_owned())?;
        Ok(Self {
            project,
            workspace_root,
            workspace_trusted: false,
            grants: GrantStore::default(),
            symbols: WorkspaceSymbolIndex::new(symbol_caps),
        })
    }

    /// Discover a Cargo project with default symbol caps.
    pub fn open_cargo(start: &Path) -> Result<Self, String> {
        Self::open_cargo_with_caps(start, SymbolIndexCaps::default())
    }

    /// Discover a Cargo project with explicit symbol caps.
    pub fn open_cargo_with_caps(
        start: &Path,
        symbol_caps: SymbolIndexCaps,
    ) -> Result<Self, String> {
        Self::new(Project::cargo(start)?, symbol_caps)
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn workspace_trusted(&self) -> bool {
        self.workspace_trusted
    }

    pub fn trust_workspace(&mut self) {
        self.workspace_trusted = true;
    }

    pub fn grants(&self) -> &GrantStore {
        &self.grants
    }

    pub fn grant(&mut self, grant: Grant) {
        self.grants.grant(grant);
    }

    pub fn symbols(&self) -> &WorkspaceSymbolIndex {
        &self.symbols
    }

    pub fn symbols_mut(&mut self) -> &mut WorkspaceSymbolIndex {
        &mut self.symbols
    }

    /// Remove indexed symbols for a file or directory subtree.
    pub fn remove_symbols_under_path(&mut self, path: &Path) -> usize {
        let Ok(relative) = path.strip_prefix(&self.workspace_root) else {
            return 0;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        self.symbols.remove_paths_under(&relative)
    }

    /// Authorize one integration launch against this root's trust and grants.
    pub fn authorize(
        &self,
        descriptor: &IntegrationDescriptor,
        capability: Capability,
        resolved_executable: &Path,
        installed: bool,
        enabled: bool,
    ) -> Result<(), Denial> {
        authorize(Authorization {
            descriptor,
            installed,
            enabled,
            workspace_trusted: self.workspace_trusted,
            grants: &self.grants,
            workspace_root: &self.workspace_root,
            capability,
            resolved_executable,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_integrations::IntegrationKind;
    use anvil_mod_workspace_index::{
        PushOutcome, SymbolKind, SymbolOccurrence, SymbolRole, TextPosition, TextRange,
        WorkspaceSymbol,
    };

    fn project() -> Project {
        Project {
            roots: vec![PathBuf::from("/work")],
            targets: Vec::new(),
            run_configurations: Vec::new(),
            test_configurations: Vec::new(),
        }
    }

    fn descriptor() -> IntegrationDescriptor {
        IntegrationDescriptor {
            id: "rust-analyzer".to_owned(),
            version: "1.0.0".to_owned(),
            kind: IntegrationKind::Lsp,
            program: PathBuf::from("/usr/bin/rust-analyzer"),
            args: vec!["--stdio".to_owned()],
            cwd: None,
            env: Vec::new(),
            languages: vec!["rust".to_owned()],
        }
    }

    fn runtime() -> Runtime {
        Runtime::new(project(), SymbolIndexCaps::default()).expect("runtime")
    }

    #[test]
    fn composes_project_and_symbols() {
        let mut runtime = runtime();
        assert_eq!(runtime.workspace_root(), Path::new("/work"));
        assert!(!runtime.workspace_trusted());
        assert_eq!(
            runtime.symbols_mut().push_symbol(WorkspaceSymbol {
                name: "main".to_owned(),
                kind: SymbolKind::Function,
                path: "src/main.rs".to_owned(),
                range: TextRange {
                    start: TextPosition {
                        line: 0,
                        character: 3
                    },
                    end: TextPosition {
                        line: 0,
                        character: 7
                    },
                },
            }),
            PushOutcome::Inserted
        );
        assert_eq!(runtime.symbols().len(), 1);
        assert_eq!(
            runtime.symbols_mut().push_occurrence(SymbolOccurrence {
                symbol: "main".to_owned(),
                role: SymbolRole::Definition,
                path: "src/main.rs".to_owned(),
                range: TextRange {
                    start: TextPosition {
                        line: 0,
                        character: 3
                    },
                    end: TextPosition {
                        line: 0,
                        character: 7
                    },
                },
            }),
            PushOutcome::Inserted
        );
        assert_eq!(runtime.symbols().occurrence_len(), 1);
    }

    #[test]
    fn authorizes_only_after_trust_and_grant() {
        let descriptor = descriptor();
        let mut runtime = runtime();
        let executable = Path::new("/usr/bin/rust-analyzer");
        assert_eq!(
            runtime.authorize(&descriptor, Capability::Lsp, executable, true, true),
            Err(Denial::UntrustedWorkspace)
        );
        runtime.trust_workspace();
        assert_eq!(
            runtime.authorize(&descriptor, Capability::Lsp, executable, true, true),
            Err(Denial::MissingGrant)
        );
        runtime
            .grant(Grant::issue(&descriptor, "/work", Capability::Lsp, executable).expect("grant"));
        assert!(
            runtime
                .authorize(&descriptor, Capability::Lsp, executable, true, true)
                .is_ok()
        );
        assert_eq!(
            runtime.authorize(
                &descriptor,
                Capability::Lsp,
                Path::new("/usr/bin/other"),
                true,
                true
            ),
            Err(Denial::MissingGrant)
        );
    }

    #[test]
    fn runtime_does_not_reuse_grants_across_roots() {
        let descriptor = descriptor();
        let executable = Path::new("/usr/bin/rust-analyzer");
        let mut runtime = runtime();
        runtime.trust_workspace();
        runtime
            .grant(Grant::issue(&descriptor, "/work", Capability::Lsp, executable).expect("grant"));
        let mut other = Runtime::new(
            Project {
                roots: vec![PathBuf::from("/other")],
                targets: Vec::new(),
                run_configurations: Vec::new(),
                test_configurations: Vec::new(),
            },
            SymbolIndexCaps::default(),
        )
        .expect("runtime");
        other.trust_workspace();
        assert_eq!(
            other.authorize(&descriptor, Capability::Lsp, executable, true, true),
            Err(Denial::MissingGrant)
        );
        assert_eq!(other.workspace_root(), Path::new("/other"));
    }
}

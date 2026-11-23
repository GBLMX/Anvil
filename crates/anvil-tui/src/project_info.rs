//! Project metadata command backed by the UI-neutral runtime root.

use super::*;
use anvil_runtime::{SymbolKind, TextPosition, TextRange, WorkspaceSymbol};

impl Model {
    pub(crate) fn load_project_info(&mut self) {
        self.project.platform_load_attempted = true;
        match self.load_project_platform() {
            Ok((roots, targets)) => {
                self.status.text = format!("project: {targets} targets, {roots} roots");
            }
            Err(error) => {
                self.status.text = format!("E: project: {error}");
            }
        }
        self.file.dirty = true;
    }

    /// Load Cargo metadata once for open/save symbol refreshes.
    pub(crate) fn ensure_project_platform(&mut self) -> bool {
        if self.project.platform.is_some() {
            return true;
        }
        if self.project.platform_load_attempted {
            return false;
        }
        self.project.platform_load_attempted = true;
        self.load_project_platform().is_ok()
    }
    fn load_project_platform(&mut self) -> Result<(usize, usize), String> {
        let mut runtime = match anvil_runtime::Runtime::open_cargo(&self.workspace.path) {
            Ok(runtime) => runtime,
            Err(error) => {
                self.project.platform = None;
                return Err(error);
            }
        };
        let roots = runtime.project().roots.len();
        let targets = runtime.project().targets.clone();
        for target in &targets {
            let _ = runtime.symbols_mut().push_symbol(WorkspaceSymbol {
                name: target.name.clone(),
                kind: SymbolKind::Other(target.kind.as_str().to_owned()),
                path: "Cargo.toml".to_owned(),
                range: TextRange {
                    start: TextPosition {
                        line: 0,
                        character: 0,
                    },
                    end: TextPosition {
                        line: 0,
                        character: 1,
                    },
                },
            });
        }
        let cache = self
            .symbol_cache
            .clone()
            .unwrap_or_else(|| runtime.symbol_cache());
        let _ = cache.load_into(runtime.symbols_mut());
        self.project.platform = Some(runtime);
        self.persist_symbol_cache();
        Ok((roots, targets.len()))
    }
}

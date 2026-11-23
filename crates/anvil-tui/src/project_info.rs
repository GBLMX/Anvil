//! Project metadata command backed by the UI-neutral runtime root.

use super::*;
use anvil_runtime::{SymbolKind, TextPosition, TextRange, WorkspaceSymbol};

impl Model {
    pub(crate) fn load_project_info(&mut self) {
        match anvil_runtime::Runtime::open_cargo(&self.workspace.path) {
            Ok(mut runtime) => {
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
                self.project.platform = Some(runtime);
                self.status.text = format!("project: {} targets, {roots} roots", targets.len());
            }
            Err(error) => {
                self.project.platform = None;
                self.status.text = format!("E: project: {error}");
            }
        }
        self.file.dirty = true;
    }
}

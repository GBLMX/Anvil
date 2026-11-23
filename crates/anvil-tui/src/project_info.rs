//! Project metadata command backed by the UI-neutral runtime root.

use super::*;

impl Model {
    pub(crate) fn load_project_info(&mut self) {
        match anvil_runtime::Runtime::open_cargo(&self.workspace.path) {
            Ok(runtime) => {
                let roots = runtime.project().roots.len();
                let targets = runtime.project().targets.len();
                self.project.platform = Some(runtime);
                self.status.text = format!("project: {targets} targets, {roots} roots");
            }
            Err(error) => {
                self.project.platform = None;
                self.status.text = format!("E: project: {error}");
            }
        }
        self.file.dirty = true;
    }
}

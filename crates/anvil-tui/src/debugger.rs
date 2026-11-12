//! Local breakpoint state and optional DAP synchronization.

use super::*;
use crate::dap::DapClient;

impl Model {
    #[allow(dead_code)]
    pub(crate) fn attach_dap_client(&mut self, client: DapClient) {
        self.dap_client = Some(client);
        self.sync_all_breakpoints();
    }

    pub(crate) fn toggle_breakpoint(&mut self) {
        let Some(path) = self.path.clone() else {
            self.status = "E: breakpoint requires a file".to_owned();
            self.dirty = true;
            return;
        };
        let Some(line) = self.current_line().map(|line| line as u32 + 1) else {
            self.status = "E: breakpoint requires a line".to_owned();
            self.dirty = true;
            return;
        };
        let entry = self.breakpoints.entry(path.clone()).or_default();
        if !entry.remove(&line) {
            entry.insert(line);
        }
        let enabled = entry.contains(&line);
        if entry.is_empty() {
            self.breakpoints.remove(&path);
        }
        self.sync_breakpoints(&path);
        self.status = if enabled {
            format!("breakpoint {}:{line}", path.display())
        } else {
            format!("breakpoint cleared {}:{line}", path.display())
        };
        self.dirty = true;
    }

    pub(crate) fn breakpoint_marker(&self, path: &Path, line: usize) -> bool {
        self.breakpoints
            .get(path)
            .is_some_and(|lines| lines.contains(&(line as u32 + 1)))
    }

    fn sync_breakpoints(&mut self, path: &Path) {
        let Some(dap) = self.dap_client.as_mut() else {
            return;
        };
        let lines = self
            .breakpoints
            .get(path)
            .map(|lines| lines.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        if let Err(error) = dap.set_breakpoints(path, &lines) {
            self.status = format!("E: breakpoint sync: {error}");
            self.dirty = true;
        }
    }

    fn sync_all_breakpoints(&mut self) {
        let paths = self.breakpoints.keys().cloned().collect::<Vec<_>>();
        for path in paths {
            self.sync_breakpoints(&path);
        }
    }
}

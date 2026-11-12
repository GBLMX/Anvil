//! Multi-root registration and per-root gateway ownership.

use super::*;

impl Model {
    pub(crate) fn add_workspace_root(&mut self, path: &str) {
        let path = path.trim();
        if path.is_empty() {
            self.status = "E: usage: :workspace-add <absolute-path>".to_owned();
            self.dirty = true;
            return;
        }
        let Ok(canonical) = Path::new(path).canonicalize() else {
            self.status = format!("E: workspace root not found: {path}");
            self.dirty = true;
            return;
        };
        if let Err(error) = self.workspace_roots.add(&canonical, false) {
            self.status = format!("E: {error}");
            self.dirty = true;
            return;
        }
        match GatewaySession::start(&canonical) {
            Ok((session, gateway, _lsp)) => {
                self.root_gateways
                    .insert(canonical.clone(), (session, gateway));
                self.status = format!("workspace root added (untrusted): {}", canonical.display());
            }
            Err(error) => {
                let _ = self.workspace_roots.remove(&canonical);
                self.status = format!("E: start root gateway: {error}");
            }
        }
        self.dirty = true;
    }

    pub(crate) fn trust_workspace_root(&mut self, path: &str, trusted: bool) {
        let path = path.trim();
        if path.is_empty() {
            self.status = "E: usage: :workspace-trust <absolute-path>".to_owned();
            self.dirty = true;
            return;
        }
        let Ok(canonical) = Path::new(path).canonicalize() else {
            self.status = format!("E: workspace root not found: {path}");
            self.dirty = true;
            return;
        };
        match self.workspace_roots.set_trusted(&canonical, trusted) {
            Ok(()) => {
                self.status = format!(
                    "workspace root {}: {}",
                    canonical.display(),
                    if trusted { "trusted" } else { "untrusted" }
                );
            }
            Err(error) => self.status = format!("E: {error}"),
        }
        self.dirty = true;
    }

    pub(crate) fn remove_workspace_root(&mut self, path: &str) {
        let path = path.trim();
        if path.is_empty() {
            self.status = "E: usage: :workspace-remove <absolute-path>".to_owned();
            self.dirty = true;
            return;
        }
        let Ok(canonical) = Path::new(path).canonicalize() else {
            self.status = format!("E: workspace root not found: {path}");
            self.dirty = true;
            return;
        };
        match self.workspace_roots.remove(&canonical) {
            Ok(()) => {
                self.root_gateways.remove(&canonical);
                self.status = format!("workspace root removed: {}", canonical.display());
            }
            Err(error) => self.status = format!("E: {error}"),
        }
        self.dirty = true;
    }

    pub(crate) fn list_workspace_roots(&mut self) {
        let roots = self.workspace_roots.list();
        if roots.is_empty() {
            self.status = "no workspace roots".to_owned();
        } else {
            self.status = roots
                .iter()
                .map(|root| {
                    format!(
                        "{} [{}]",
                        root.path.display(),
                        if root.trusted { "trusted" } else { "untrusted" }
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            self.status
                .push_str(&format!("; gateways: {}", self.root_gateways.len()));
        }
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_start_with_primary() {
        let model = Model::new(EditorState::from_text(""), None);
        assert_eq!(model.workspace_roots.list().len(), 1);
        assert!(model.workspace_roots.primary().is_some());
    }
}

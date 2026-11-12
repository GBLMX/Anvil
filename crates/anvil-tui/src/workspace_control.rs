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
        if let Err(error) = self.workspace.roots.add(&canonical, false) {
            self.status = format!("E: {error}");
            self.dirty = true;
            return;
        }
        self.status = format!(
            "workspace root added (untrusted, no gateway): {}",
            canonical.display()
        );
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
        if !trusted {
            self.stop_root_file_worker(&canonical);
            self.workspace.root_gateways.remove(&canonical);
            match self.workspace.roots.set_trusted(&canonical, false) {
                Ok(()) => {
                    self.status = format!(
                        "workspace root {}: untrusted (gateway stopped)",
                        canonical.display()
                    );
                }
                Err(error) => self.status = format!("E: {error}"),
            }
        } else if self.workspace.root_gateways.contains_key(&canonical) {
            self.status = format!("workspace root {}: already trusted", canonical.display());
        } else {
            match GatewaySession::start(&canonical) {
                Ok((session, gateway, _lsp)) => {
                    self.workspace.roots.set_trusted(&canonical, true).ok();
                    self.workspace
                        .root_gateways
                        .insert(canonical.clone(), (session, gateway));
                    self.ensure_root_file_workers();
                    self.status = format!(
                        "workspace root {}: trusted, gateway started",
                        canonical.display()
                    );
                }
                Err(error) => {
                    self.status = format!("E: start root gateway: {error}");
                }
            }
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
        match self.workspace.roots.remove(&canonical) {
            Ok(()) => {
                self.stop_root_file_worker(&canonical);
                self.workspace.root_gateways.remove(&canonical);
                self.status = format!("workspace root removed: {}", canonical.display());
            }
            Err(error) => self.status = format!("E: {error}"),
        }
        self.dirty = true;
    }

    pub(crate) fn list_workspace_roots(&mut self) {
        let roots = self.workspace.roots.list();
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
            self.status.push_str(&format!(
                "; gateways: {}",
                self.workspace.root_gateways.len()
            ));
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
        assert_eq!(model.workspace.roots.list().len(), 1);
        assert!(model.workspace.roots.primary().is_some());
    }
}

#[cfg(test)]
mod trust_tests {
    use super::*;

    #[test]
    fn untrusted_root_has_no_gateway() {
        let root = tempfile::tempdir().expect("root");
        let canonical = root.path().canonicalize().expect("canonical");
        let mut model = Model::new(EditorState::from_text(""), None);
        model.add_workspace_root(canonical.to_str().expect("utf8"));
        assert_eq!(model.workspace.roots.list().len(), 2);
        assert!(model.workspace.root_gateways.is_empty());
        assert!(model.workspace.root_file_workers.is_empty());
        let added = model
            .workspace
            .roots
            .list()
            .iter()
            .find(|root| root.path == canonical)
            .unwrap();
        assert!(!added.trusted);
    }
}

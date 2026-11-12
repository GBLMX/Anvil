//! Package download review and installation flow.

use anvil_ipc::ClientError;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};
use serde::Deserialize;

use crate::gateway::GatewaySession;
use crate::{Model, theme};
#[cfg(target_os = "linux")]
use anvil_session::plugin_supervisor::HostState;

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct PkgManifest {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) version: String,
    #[serde(default)]
    pub(crate) permissions: Vec<String>,
}

#[derive(Clone)]
pub(crate) enum PkgInstallState {
    Downloading {
        job_id: String,
        progress: u64,
    },
    Review {
        package_path: String,
        signature_path: String,
        public_key: String,
        digest: String,
        manifest: PkgManifest,
        trusted: bool,
        error: Option<String>,
    },
}

impl Model {
    pub(crate) fn start_pkg_install(&mut self, url: &str) {
        let Some(host) = url_host(url) else {
            self.status = "E: pkg install requires an https URL".to_owned();
            self.dirty = true;
            return;
        };
        self.plugins.installer = None;
        match GatewaySession::start_installer(&self.workspace.path, &host) {
            Ok((session, client)) => self.plugins.installer = Some((session, client)),
            Err(error) => {
                self.status = format!("E: installer gateway: {error}");
                self.dirty = true;
                return;
            }
        }
        let Some((_, gateway)) = self.plugins.installer.as_mut() else {
            return;
        };
        let signature_url = format!("{url}.sig");
        match gateway.call(
            "pkg.download",
            serde_json::json!({"url": url, "signature_url": signature_url}),
        ) {
            Ok(result) => {
                let Some(job_id) = result.get("job_id").and_then(|value| value.as_str()) else {
                    self.status = "E: pkg download: missing job id".to_owned();
                    self.dirty = true;
                    return;
                };
                let progress = self.ui.progress.start("pkg download", url, false);
                self.plugins.pkg_install = Some(PkgInstallState::Downloading {
                    job_id: job_id.to_owned(),
                    progress,
                });
                self.status = format!("pkg download queued: {url}");
            }
            Err(error) => self.pkg_error(error),
        }
        self.dirty = true;
    }

    pub(crate) fn start_pkg_install_file(
        &mut self,
        package_path: &str,
        signature_path: &str,
        public_key: &str,
    ) {
        self.plugins.installer = None;
        match GatewaySession::start_local_installer(&self.workspace.path) {
            Ok((session, client)) => self.plugins.installer = Some((session, client)),
            Err(error) => {
                self.status = format!("E: installer gateway: {error}");
                self.dirty = true;
                return;
            }
        }
        let Some((_, gateway)) = self.plugins.installer.as_mut() else {
            return;
        };
        match gateway.call(
            "pkg.inspect",
            serde_json::json!({
                "package_path": package_path,
                "signature_path": signature_path,
                "public_key": public_key,
            }),
        ) {
            Ok(result) => {
                let text = |name: &str| {
                    result
                        .get(name)
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_owned()
                };
                let Some(manifest) = result
                    .get("manifest")
                    .cloned()
                    .and_then(|value| serde_json::from_value::<PkgManifest>(value).ok())
                else {
                    self.pkg_error_message("package manifest is malformed");
                    return;
                };
                let digest = text("digest");
                if digest.is_empty() || manifest.id.is_empty() || manifest.version.is_empty() {
                    self.pkg_error_message("package inspection is incomplete");
                    return;
                }
                self.plugins.pkg_install = Some(PkgInstallState::Review {
                    package_path: package_path.to_owned(),
                    signature_path: signature_path.to_owned(),
                    public_key: text("public_key"),
                    digest,
                    manifest,
                    trusted: result
                        .get("trusted")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false),
                    error: None,
                });
                self.status = "Pkg: review capabilities, Enter installs".to_owned();
            }
            Err(error) => self.pkg_error(error),
        }
        self.dirty = true;
    }

    pub(crate) fn poll_pkg_notifications(&mut self) {
        let Some((_, gateway)) = self.plugins.installer.as_mut() else {
            return;
        };
        let notifications = match gateway.poll_notifications() {
            Ok(notifications) => notifications,
            Err(error) => {
                self.pkg_error(error);
                return;
            }
        };
        for notification in notifications {
            match notification.method.as_str() {
                "pkg.progress" => self.pkg_progress(&notification.params),
                "pkg.downloaded" => self.pkg_downloaded(&notification.params),
                "pkg.failed" => self.pkg_failed(&notification.params),
                _ => {}
            }
        }
    }

    fn pkg_progress(&mut self, params: &serde_json::Value) {
        let Some(state) = &self.plugins.pkg_install else {
            return;
        };
        let PkgInstallState::Downloading {
            job_id, progress, ..
        } = state
        else {
            return;
        };
        if params.get("job_id").and_then(|value| value.as_str()) != Some(job_id.as_str()) {
            return;
        }
        let fraction = params
            .get("fraction")
            .and_then(|value| value.as_u64())
            .and_then(|value| u8::try_from(value).ok());
        let message = params
            .get("stage")
            .and_then(|value| value.as_str())
            .unwrap_or("downloading");
        let progress = *progress;
        self.ui.progress.update(progress, fraction, message);
        self.dirty = true;
    }

    fn pkg_downloaded(&mut self, params: &serde_json::Value) {
        let Some(state) = &self.plugins.pkg_install else {
            return;
        };
        let PkgInstallState::Downloading {
            job_id, progress, ..
        } = state
        else {
            return;
        };
        if params.get("job_id").and_then(|value| value.as_str()) != Some(job_id.as_str()) {
            return;
        }
        let progress = *progress;
        self.ui.progress.finish(progress);
        let text = |name: &str| {
            params
                .get(name)
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_owned()
        };
        let public_key = text("public_key");
        let Some(manifest) = params
            .get("manifest")
            .cloned()
            .and_then(|value| serde_json::from_value::<PkgManifest>(value).ok())
        else {
            self.plugins.pkg_install = None;
            self.status = "E: package manifest is malformed".to_owned();
            self.dirty = true;
            return;
        };
        let trusted = params
            .get("trusted")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if public_key.is_empty() {
            self.plugins.pkg_install = None;
            self.status = "E: package signature has no public key".to_owned();
            self.dirty = true;
            return;
        }
        let digest = text("digest");
        if manifest.id.is_empty() || manifest.version.is_empty() {
            self.plugins.pkg_install = None;
            self.status = "E: package manifest has no id or version".to_owned();
            self.dirty = true;
            return;
        }
        if digest.is_empty() {
            self.plugins.pkg_install = None;
            self.status = "E: package download has no digest".to_owned();
            self.dirty = true;
            return;
        }
        self.plugins.pkg_install = Some(PkgInstallState::Review {
            package_path: text("package_path"),
            signature_path: text("signature_path"),
            public_key,
            manifest,
            trusted,
            error: None,
            digest,
        });
        self.status = "Pkg: review capabilities, Enter installs".to_owned();
        self.dirty = true;
    }

    pub(crate) fn confirm_pkg_install(&mut self) {
        let Some(PkgInstallState::Review {
            package_path,
            signature_path,
            public_key,
            digest,
            manifest,
            ..
        }) = self.plugins.pkg_install.clone()
        else {
            return;
        };
        #[cfg(target_os = "linux")]
        if self
            .plugins
            .entries
            .iter()
            .any(|plugin| plugin.manifest.id == manifest.id)
            && matches!(
                self.plugins.supervisor.state(&manifest.id),
                HostState::Queued
                    | HostState::Registering
                    | HostState::Starting
                    | HostState::Ready
                    | HostState::Draining
            )
        {
            let Some(gateway) = self.workspace.gateway.as_mut() else {
                self.status = "E: gateway unavailable".to_owned();
                self.dirty = true;
                return;
            };
            if let Err(error) = self.plugins.supervisor.stop(&manifest.id, Some(gateway)) {
                self.status = format!("E: stop plugin {}: {error}", manifest.id);
                self.dirty = true;
                return;
            }
        }
        let Some((_, gateway)) = self.plugins.installer.as_mut() else {
            self.status = "E: pkg install requires installer gateway".to_owned();
            return;
        };
        match gateway.call(
            "pkg.install",
            serde_json::json!({
                "package_path": package_path,
                "signature_path": signature_path,
                "public_key": public_key,
                "digest": digest,
            }),
        ) {
            Ok(result) => {
                let plugin_id = result
                    .get("plugin_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_owned();
                self.plugins.pkg_install = None;
                self.plugins.installer = None;
                let installed =
                    format!("plugin installed: {plugin_id}; run :plugin-enable {plugin_id}");
                self.status = installed.clone();
                if let Err(error) = self.load_plugins() {
                    self.status = format!("{installed}; discovery: {error}");
                }
            }
            Err(ClientError::Rpc {
                code: -32010,
                message,
            }) => {
                if let Some(PkgInstallState::Review { trusted, error, .. }) =
                    self.plugins.pkg_install.as_mut()
                {
                    *trusted = false;
                    *error = Some(message.clone());
                }
                self.status = "E: untrusted signing key; press t to trust".to_owned();
            }
            Err(error) => self.pkg_error(error),
        }
        self.dirty = true;
    }

    pub(crate) fn trust_pkg_key(&mut self) {
        let Some(PkgInstallState::Review {
            public_key,
            manifest,
            ..
        }) = self.plugins.pkg_install.clone()
        else {
            return;
        };
        let Some(gateway) = self.workspace.gateway.as_mut() else {
            self.status = "E: pkg trust requires gateway".to_owned();
            return;
        };
        match gateway.call(
            "pkg.trust",
            serde_json::json!({"name": manifest.id, "public_key": public_key}),
        ) {
            Ok(_) => {
                if let Some(PkgInstallState::Review { trusted, error, .. }) =
                    self.plugins.pkg_install.as_mut()
                {
                    *trusted = true;
                    *error = None;
                }
                self.status = "signing key trusted; press Enter to install".to_owned();
            }
            Err(error) => self.pkg_error(error),
        }
        self.dirty = true;
    }

    pub(crate) fn cancel_pkg_install(&mut self) {
        if let Some(PkgInstallState::Downloading { progress, .. }) = &self.plugins.pkg_install {
            let progress = *progress;
            self.ui.progress.finish(progress);
        }
        self.plugins.pkg_install = None;
        self.plugins.installer = None;
        self.dirty = true;
    }

    fn pkg_failed(&mut self, params: &serde_json::Value) {
        let Some(PkgInstallState::Downloading { job_id, .. }) = &self.plugins.pkg_install else {
            return;
        };
        if params.get("job_id").and_then(|value| value.as_str()) != Some(job_id.as_str()) {
            return;
        }
        if let Some(message) = params.get("message").and_then(|value| value.as_str()) {
            self.pkg_error_message(message);
        }
    }

    fn pkg_error(&mut self, error: ClientError) {
        self.pkg_error_message(&error.to_string());
    }

    fn pkg_error_message(&mut self, message: &str) {
        if let Some(PkgInstallState::Downloading { progress, .. }) = &self.plugins.pkg_install {
            let progress = *progress;
            self.ui.progress.finish(progress);
        }
        self.plugins.pkg_install = None;
        self.plugins.installer = None;
        self.status = format!("E: pkg: {message}");
        self.dirty = true;
    }
}

fn url_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

pub(crate) fn render(model: &Model, frame: &mut Frame, area: Rect) {
    let Some(state) = &model.plugins.pkg_install else {
        return;
    };
    let PkgInstallState::Review {
        manifest,
        public_key,
        trusted,
        error,
        ..
    } = state
    else {
        return;
    };
    let width = (area.width * 3 / 5)
        .max(40)
        .min(area.width.saturating_sub(2));
    let height = (manifest.permissions.len() as u16 + 10)
        .max(12)
        .min(area.height.saturating_sub(2));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);
    let safe = |value: &str| value.escape_default().to_string();
    let key_preview = public_key.chars().take(24).collect::<String>();
    let mut lines = vec![
        Line::from(vec![Span::styled(
            format!("{} {}", safe(&manifest.id), safe(&manifest.version)),
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(format!(
            "signing key: {}{} [{}]",
            safe(&key_preview),
            if public_key.chars().count() > 24 {
                "…"
            } else {
                ""
            },
            if *trusted { "trusted" } else { "untrusted" }
        )),
        Line::raw(""),
        Line::from("Capabilities:"),
    ];
    if manifest.permissions.is_empty() {
        lines.push(Line::raw("  (none declared)"));
    }
    for permission in &manifest.permissions {
        lines.push(Line::from(format!("  • {}", safe(permission))));
    }
    if let Some(error) = error {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            safe(error),
            Style::default().fg(theme::error()),
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(if *trusted {
        "[Enter] install  [Esc] cancel"
    } else {
        "[t] trust key  [Esc] cancel"
    }));
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::panel())
            .block(crate::render::panel_block("Install plugin")),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EditorState;

    fn downloading() -> Model {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.plugins.pkg_install = Some(PkgInstallState::Downloading {
            job_id: "job".to_owned(),
            progress: 0,
        });
        model
    }

    #[test]
    fn downloaded_package_enters_review_with_digest() {
        let mut model = downloading();
        model.pkg_downloaded(&serde_json::json!({
            "job_id": "job",
            "package_path": "/tmp/pkg",
            "signature_path": "/tmp/pkg.sig",
            "public_key": "aa",
            "digest": "bb",
            "trusted": true,
            "manifest": {
                "id": "demo",
                "version": "1.0.0",
                "permissions": ["fs.read:**"]
            }
        }));
        let Some(PkgInstallState::Review {
            digest,
            trusted,
            manifest,
            ..
        }) = &model.plugins.pkg_install
        else {
            panic!("expected review state");
        };
        assert_eq!(digest, "bb");
        assert!(*trusted);
        assert_eq!(manifest.id, "demo");
        assert_eq!(manifest.permissions, ["fs.read:**"]);
    }

    #[test]
    fn download_without_digest_is_rejected() {
        let mut model = downloading();
        model.pkg_downloaded(&serde_json::json!({
            "job_id": "job",
            "package_path": "/tmp/pkg",
            "signature_path": "/tmp/pkg.sig",
            "public_key": "aa",
            "manifest": {"id": "demo", "version": "1.0.0"}
        }));
        assert!(model.plugins.pkg_install.is_none());
        assert!(
            model
                .status
                .starts_with("E: package download has no digest")
        );
    }

    #[test]
    fn unrelated_download_failure_keeps_active_job() {
        let mut model = downloading();
        model.pkg_failed(&serde_json::json!({"job_id": "other", "message": "boom"}));
        assert!(matches!(
            model.plugins.pkg_install,
            Some(PkgInstallState::Downloading { .. })
        ));
        model.pkg_failed(&serde_json::json!({"job_id": "job", "message": "boom"}));
        assert!(model.plugins.pkg_install.is_none());
        assert!(model.status.contains("boom"));
    }

    #[test]
    fn review_overlay_lists_capabilities() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut model = Model::new(EditorState::from_text(""), None);
        model.plugins.pkg_install = Some(PkgInstallState::Review {
            package_path: "/tmp/pkg".to_owned(),
            signature_path: "/tmp/pkg.sig".to_owned(),
            public_key: "abcdef".to_owned(),
            digest: "00".to_owned(),
            manifest: PkgManifest {
                id: "demo".to_owned(),
                version: "1.0.0".to_owned(),
                permissions: vec!["fs.read:**".to_owned()],
            },
            trusted: false,
            error: None,
        });
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render(&model, frame, frame.area()))
            .expect("draw");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("demo 1.0.0"), "{rendered}");
        assert!(rendered.contains("fs.read:**"), "{rendered}");
        assert!(rendered.contains("[t] trust key"), "{rendered}");
    }

    #[test]
    fn installer_url_host_requires_https() {
        assert_eq!(
            url_host("https://Example.COM:8443/plugin.anvil-plugin"),
            Some("example.com".to_owned())
        );
        assert_eq!(url_host("http://example.com/plugin"), None);
        assert_eq!(url_host("https:///plugin"), None);
    }

    #[test]
    fn official_manifest_without_permissions_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/official/trim-whitespace/plugin.json");
        let data = std::fs::read_to_string(path).expect("official manifest");
        let manifest: PkgManifest = serde_json::from_str(&data).expect("parse manifest");
        assert_eq!(manifest.id, "anvil.official.trim-whitespace");
        assert!(manifest.permissions.is_empty());
    }
}

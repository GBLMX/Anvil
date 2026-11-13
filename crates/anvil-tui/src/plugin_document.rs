//! Editor-side document snapshot and transaction contract for plugins.

use anvil_api::{
    ApplyEditRequest, ApplyEditResponse, DocumentSnapshot, EmptyResponse, NotificationSeverity,
    NotifyRequest, RegisterCommandRequest, RegisterCommandResponse, RegisterProviderRequest,
    RegisterProviderResponse, SetModeRequest, SetModeResponse, StatusRequest,
};
use anvil_session::Session;

use crate::Model;

impl Model {
    /// Current active-buffer snapshot for a plugin API call.
    pub fn plugin_snapshot(&self) -> DocumentSnapshot {
        Session::snapshot(&self.session.editor, self.plugin_buffer_id())
    }

    /// Applies one plugin transaction against the active buffer.
    pub fn apply_plugin_edit(&mut self, request: ApplyEditRequest) -> Result<u64, String> {
        let buffer_id = self.plugin_buffer_id();
        let count = request.edits.len();
        match Session::new(&mut self.session.editor, buffer_id).apply(request) {
            Ok(revision) => {
                self.sync_cursor_from_selection();
                self.file.modified = true;
                self.file.dirty = true;
                self.status.text = format!("plugin apply_edit: {count} edits, revision {revision}");
                Ok(revision)
            }
            Err(error) => {
                self.status.text = format!("E: plugin apply_edit: {error}");
                self.file.dirty = true;
                Err(error.to_string())
            }
        }
    }

    pub(crate) fn poll_editor_requests(&mut self) {
        while let Some(request) = self.plugins.supervisor.poll_editor_request() {
            let result =
                self.handle_editor_request(&request.plugin_id, &request.method, request.params);
            let _ = request.reply.send(result);
        }
    }

    fn handle_editor_request(
        &mut self,
        plugin_id: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, (i32, String)> {
        match method {
            "editor.get_view" => Ok(serde_json::json!(self.session.editor.snapshot().view())),
            "editor.get_snapshot" => Ok(serde_json::json!(self.plugin_snapshot())),
            "editor.set_mode" => {
                let request: SetModeRequest =
                    serde_json::from_value(params).map_err(|error| (-32602, error.to_string()))?;
                self.session.editor.set_interaction_mode(request.mode);
                self.file.dirty = true;
                Ok(serde_json::json!(SetModeResponse { mode: request.mode }))
            }
            "editor.apply_edit" => {
                let request: ApplyEditRequest =
                    serde_json::from_value(params).map_err(|error| (-32602, error.to_string()))?;
                let applied_edits = request.edits.len() as u32;
                self.apply_plugin_edit(request)
                    .map(|revision| {
                        serde_json::json!(ApplyEditResponse {
                            revision,
                            applied_edits,
                        })
                    })
                    .map_err(|error| (-32009, error))
            }
            "editor.register_command" => {
                let request: RegisterCommandRequest =
                    serde_json::from_value(params).map_err(|error| (-32602, error.to_string()))?;
                self.register_plugin_command(plugin_id, request)
                    .map(|command_id| serde_json::json!(RegisterCommandResponse { command_id }))
                    .map_err(|message| (-32000, message))
            }
            "editor.register_provider" => {
                let request: RegisterProviderRequest =
                    serde_json::from_value(params).map_err(|error| (-32602, error.to_string()))?;
                self.register_plugin_provider(plugin_id, request)
                    .map(|provider_id| serde_json::json!(RegisterProviderResponse { provider_id }))
                    .map_err(|message| (-32000, message))
            }
            "ui.notify" => {
                let request: NotifyRequest =
                    serde_json::from_value(params).map_err(|error| (-32602, error.to_string()))?;
                if request.message.is_empty() || request.message.len() > 4096 {
                    return Err((-32602, "invalid notification message".to_owned()));
                }
                let severity = match request.severity {
                    NotificationSeverity::Info => anvil_session::ui::NotificationSeverity::Info,
                    NotificationSeverity::Warning => {
                        anvil_session::ui::NotificationSeverity::Warning
                    }
                    NotificationSeverity::Error => anvil_session::ui::NotificationSeverity::Error,
                    _ => anvil_session::ui::NotificationSeverity::Info,
                };
                self.ui
                    .notifications
                    .push(severity, plugin_id, request.message, 0);
                self.file.dirty = true;
                Ok(serde_json::json!(EmptyResponse {}))
            }
            "ui.status" => {
                let request: StatusRequest =
                    serde_json::from_value(params).map_err(|error| (-32602, error.to_string()))?;
                if request.message.len() > 1024 {
                    return Err((-32602, "invalid status message".to_owned()));
                }
                self.status.text = request.message;
                self.file.dirty = true;
                Ok(serde_json::json!(EmptyResponse {}))
            }
            _ => Err((-32601, format!("method not found: {method}"))),
        }
    }
    fn register_plugin_command(
        &mut self,
        plugin_id: &str,
        request: RegisterCommandRequest,
    ) -> Result<String, String> {
        let command_id = request.id.trim().to_owned();
        let title = request.title.trim().to_owned();
        if command_id.is_empty() || command_id.len() > 128 {
            return Err("invalid plugin command id".to_owned());
        }
        if title.is_empty() || title.len() > 256 {
            return Err("invalid plugin command title".to_owned());
        }
        if !self
            .plugins
            .entries
            .iter()
            .any(|plugin| plugin.enabled && plugin.manifest.id == plugin_id)
        {
            return Err("plugin is not loaded or enabled".to_owned());
        }
        if !command_id.starts_with(&format!("{plugin_id}.")) {
            return Err("plugin command id must use the caller's namespace".to_owned());
        }
        if self
            .plugins
            .entries
            .iter()
            .flat_map(|plugin| plugin.manifest.commands.iter())
            .any(|command| command.id == command_id)
            || self.plugins.dynamic_commands.contains_key(&command_id)
        {
            return Err(format!("duplicate plugin command: {command_id}"));
        }
        if self.plugins.dynamic_commands.len() >= 512 {
            return Err("too many dynamic plugin commands".to_owned());
        }
        self.plugins.dynamic_commands.insert(
            command_id.clone(),
            crate::state::DynamicPluginCommand {
                plugin_id: plugin_id.to_owned(),
                title,
            },
        );
        Ok(command_id)
    }
    fn register_plugin_provider(
        &mut self,
        plugin_id: &str,
        request: RegisterProviderRequest,
    ) -> Result<String, String> {
        if !self
            .plugins
            .entries
            .iter()
            .any(|plugin| plugin.enabled && plugin.manifest.id == plugin_id)
        {
            return Err("plugin is not loaded or enabled".to_owned());
        }
        self.plugins
            .plugin_providers
            .register(plugin_id, request)
            .map(|registration| registration.id.as_str().to_owned())
            .map_err(|error| error.to_string())
    }

    fn plugin_buffer_id(&self) -> u64 {
        self.buffers.active as u64 + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_api::{DocumentEdit, TextRange};
    use anvil_core::TextBuffer;
    use anvil_editor::EditorState;

    #[test]
    fn snapshot_and_transaction_use_editor_revision() {
        let mut model = Model::new(EditorState::from_text("abcd"), None);
        let snapshot = model.plugin_snapshot();
        assert_eq!(snapshot.text, "abcd");
        let revision = model
            .apply_plugin_edit(ApplyEditRequest {
                buffer_id: snapshot.buffer_id,
                base_revision: snapshot.revision,
                edits: vec![DocumentEdit {
                    range: TextRange { start: 1, end: 3 },
                    text: "X".to_owned(),
                }],
            })
            .expect("apply");
        assert_eq!(model.session.editor.text().slice(0..3), "aXd");
        assert_eq!(revision, snapshot.revision + 1);
        assert_eq!(model.plugin_snapshot().revision, revision);
    }

    #[test]
    fn stale_transaction_is_rejected_without_mutation() {
        let mut model = Model::new(EditorState::from_text("abcd"), None);
        let snapshot = model.plugin_snapshot();
        let request = ApplyEditRequest {
            buffer_id: snapshot.buffer_id,
            base_revision: snapshot.revision + 1,
            edits: vec![DocumentEdit {
                range: TextRange { start: 0, end: 1 },
                text: "X".to_owned(),
            }],
        };
        assert!(model.apply_plugin_edit(request).is_err());
        assert_eq!(model.session.editor.text().slice(0..4), "abcd");
        assert!(model.status.text.contains("stale revision"));
    }

    #[test]
    fn wrong_buffer_is_rejected() {
        let mut model = Model::new(EditorState::from_text("abcd"), None);
        let snapshot = model.plugin_snapshot();
        let request = ApplyEditRequest {
            buffer_id: snapshot.buffer_id + 1,
            base_revision: snapshot.revision,
            edits: Vec::new(),
        };
        assert!(model.apply_plugin_edit(request).is_err());
        assert!(model.status.text.contains("buffer mismatch"));
    }

    #[test]
    fn editor_service_round_trips_get_view() {
        use sha2::{Digest, Sha256};

        let mut model = Model::new(EditorState::from_text("abcd"), None);
        let token = [7u8; 32];
        model
            .plugins
            .supervisor
            .register_editor_identity(Sha256::digest(token).into(), "test");
        model.plugins.supervisor.ensure_editor_service();
        let path = model
            .plugins
            .supervisor
            .editor_socket()
            .expect("editor socket")
            .to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut connection = anvil_ipc::Connection::connect(path).expect("connect");
            connection
                .set_read_timeout(Some(std::time::Duration::from_secs(1)))
                .expect("timeout");
            let hello = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "session.hello",
                "params": {
                    "plugin_id": "test",
                    "token": token.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
                }
            });
            connection
                .send(serde_json::to_string(&hello).expect("hello").as_bytes())
                .expect("hello");
            connection
                .send(br#"{"jsonrpc":"2.0","id":1,"method":"editor.get_view","params":{}}"#)
                .expect("request");
            let frame = connection.recv().expect("read").expect("response");
            let _ = tx.send(String::from_utf8(frame).expect("utf8"));
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            model.poll_editor_requests();
            if let Ok(response) = rx.try_recv() {
                assert!(response.contains("\"revision\":0"), "{response}");
                assert!(
                    response.contains("\"interaction_mode\":\"select\""),
                    "{response}"
                );
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "editor service timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[cfg(target_os = "linux")]
    fn plugin(id: &str) -> crate::plugin_discovery::PluginDescriptor {
        crate::plugin_discovery::PluginDescriptor {
            root: std::path::PathBuf::from("/tmp"),
            manifest: anvil_plugin_host::manifest::PluginManifest {
                id: id.to_owned(),
                version: "0.1.0".to_owned(),
                entry: "run.luau".to_owned(),
                api: "1.0".to_owned(),
                activation_events: Vec::new(),
                commands: Vec::new(),
            },
            enabled: true,
            digest: None,
            source: None,
            previous_digest: None,
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn plugin_command_registration_is_namespaced_and_stored() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.plugins.entries.push(plugin("demo"));
        let response = model
            .handle_editor_request(
                "demo",
                "editor.register_command",
                serde_json::json!({"id": "demo.run", "title": "Run"}),
            )
            .expect("register");
        assert_eq!(response["command_id"], "demo.run");
        assert_eq!(
            model
                .plugins
                .dynamic_commands
                .get("demo.run")
                .map(|command| command.plugin_id.as_str()),
            Some("demo")
        );
    }

    #[test]
    fn plugin_notifications_and_status_are_applied() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model
            .handle_editor_request(
                "demo",
                "ui.notify",
                serde_json::json!({"message": "hello", "severity": "warning"}),
            )
            .expect("notify");
        let notification = &model.ui.notifications.items()[0];
        assert_eq!(notification.message, "hello");
        assert_eq!(notification.source, "demo");
        assert_eq!(
            notification.severity,
            anvil_session::ui::NotificationSeverity::Warning
        );
        model
            .handle_editor_request(
                "demo",
                "ui.status",
                serde_json::json!({"message": "plugin status"}),
            )
            .expect("status");
        assert_eq!(model.status.text, "plugin status");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn plugin_stop_cancels_pending_provider_requests() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.plugins.entries.push(plugin("demo"));
        model
            .handle_editor_request(
                "demo",
                "editor.register_provider",
                serde_json::json!({"id": "demo.buffers", "kind": "picker", "title": "Buffers", "handler": "provide"}),
            )
            .expect("register provider");
        let request_id = model
            .plugins
            .plugin_provider_requests
            .start(
                anvil_session::provider::ProviderId::new("demo.buffers"),
                "",
                1,
            )
            .expect("start request");
        model.release_plugin_providers("demo");
        assert!(model.plugins.plugin_providers.is_empty());
        assert!(
            model
                .plugins
                .plugin_provider_requests
                .accept(anvil_api::ProviderResponse {
                    request_id,
                    items: Vec::new(),
                })
                .is_err()
        );
    }

    #[test]
    fn provider_snapshot_must_still_match() {
        let mut model = Model::new(EditorState::from_text(""), None);
        let revision = model.session.editor.revision();
        let cursor = model.session.cursor;
        model
            .plugins
            .provider_snapshots
            .insert(1, (revision, cursor));
        assert!(model.take_provider_snapshot(1));
        assert!(!model.take_provider_snapshot(1));
        model
            .plugins
            .provider_snapshots
            .insert(2, (revision, cursor));
        model.session.editor.insert(0, "x");
        assert!(!model.take_provider_snapshot(2));
    }

    #[test]
    fn plugin_completion_items_install_into_overlay() {
        let mut model = Model::new(EditorState::from_text("buf"), None);
        model.install_plugin_completion(
            &anvil_session::provider::ProviderId::new("demo.completion"),
            7,
            vec![anvil_api::ProviderItem {
                label: "buffer".to_owned(),
                value: None,
                detail: Some("plugin".to_owned()),
                insert_text: Some("buffer".to_owned()),
            }],
        );
        let completion = model
            .overlays
            .lsp_results
            .completion
            .as_ref()
            .expect("completion");
        assert_eq!(completion.id, 7);
        assert_eq!(completion.items[0].label, "buffer");
        assert_eq!(completion.items[0].insert, "buffer");
    }

    #[test]
    fn provider_query_unknown_reports_error() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.execute_command("provider-query demo.buffers");
        assert!(
            model.status.text.contains("unknown provider"),
            "{}",
            model.status.text
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn plugin_provider_registration_is_namespaced_and_stored() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.plugins.entries.push(plugin("demo"));
        let response = model
            .handle_editor_request(
                "demo",
                "editor.register_provider",
                serde_json::json!({"id": "demo.buffers", "kind": "picker", "title": "Buffers", "handler": "provide"}),
            )
            .expect("register provider");
        assert_eq!(response["provider_id"], "demo.buffers");
        assert_eq!(model.plugins.plugin_providers.len(), 1);
        assert!(
            model
                .handle_editor_request(
                    "demo",
                    "editor.register_provider",
                    serde_json::json!({"id": "other.buffers", "kind": "picker", "title": "Buffers", "handler": "provide"}),
                )
                .is_err()
        );
        assert!(
            model
                .handle_editor_request(
                    "demo",
                    "editor.register_provider",
                    serde_json::json!({"id": "demo.buffers", "kind": "picker", "title": "Buffers", "handler": "provide"}),
                )
                .is_err()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn plugin_command_registration_rejects_another_plugin_identity() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.plugins.entries.push(plugin("demo"));
        assert!(
            model
                .handle_editor_request(
                    "other",
                    "editor.register_command",
                    serde_json::json!({"id": "demo.run", "title": "Run"}),
                )
                .is_err()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn plugin_command_registration_rejects_unknown_namespace() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.plugins.entries.push(plugin("demo"));
        assert!(
            model
                .handle_editor_request(
                    "demo",
                    "editor.register_command",
                    serde_json::json!({"id": "other.run", "title": "Run"}),
                )
                .is_err()
        );
    }
}

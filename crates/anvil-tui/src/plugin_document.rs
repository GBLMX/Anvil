//! Editor-side document snapshot and transaction contract for plugins.

use anvil_api::{
    ApplyEditRequest, ApplyEditResponse, DocumentSnapshot, RegisterCommandRequest,
    RegisterCommandResponse, SetModeRequest, SetModeResponse,
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
            let result = self.handle_editor_request(&request.method, request.params);
            let _ = request.reply.send(result);
        }
    }

    fn handle_editor_request(
        &mut self,
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
                self.register_plugin_command(request)
                    .map(|command_id| serde_json::json!(RegisterCommandResponse { command_id }))
                    .map_err(|message| (-32000, message))
            }
            _ => Err((-32601, format!("method not found: {method}"))),
        }
    }
    fn register_plugin_command(
        &mut self,
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
        let plugin_id = self
            .plugins
            .entries
            .iter()
            .filter(|plugin| {
                plugin.enabled && command_id.starts_with(&format!("{}.", plugin.manifest.id))
            })
            .max_by_key(|plugin| plugin.manifest.id.len())
            .map(|plugin| plugin.manifest.id.clone())
            .ok_or_else(|| "plugin command id must be namespaced by a loaded plugin".to_owned())?;
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
            crate::state::DynamicPluginCommand { plugin_id, title },
        );
        Ok(command_id)
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
        let mut model = Model::new(EditorState::from_text("abcd"), None);
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

    #[cfg(target_os = "linux")]
    #[test]
    fn plugin_command_registration_rejects_unknown_namespace() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.plugins.entries.push(plugin("demo"));
        assert!(
            model
                .handle_editor_request(
                    "editor.register_command",
                    serde_json::json!({"id": "other.run", "title": "Run"}),
                )
                .is_err()
        );
    }
}

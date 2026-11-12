//! Editor-side document snapshot and transaction contract for plugins.

use anvil_api::{
    ApplyEditRequest, ApplyEditResponse, DocumentSnapshot, SetModeRequest, SetModeResponse,
};
use anvil_session::Session;

use crate::Model;

impl Model {
    /// Current active-buffer snapshot for a plugin API call.
    pub fn plugin_snapshot(&self) -> DocumentSnapshot {
        Session::snapshot(&self.editor, self.plugin_buffer_id())
    }

    /// Applies one plugin transaction against the active buffer.
    pub fn apply_plugin_edit(&mut self, request: ApplyEditRequest) -> Result<u64, String> {
        let buffer_id = self.plugin_buffer_id();
        let count = request.edits.len();
        match Session::new(&mut self.editor, buffer_id).apply(request) {
            Ok(revision) => {
                self.sync_cursor_from_selection();
                self.modified = true;
                self.dirty = true;
                self.status = format!("plugin apply_edit: {count} edits, revision {revision}");
                Ok(revision)
            }
            Err(error) => {
                self.status = format!("E: plugin apply_edit: {error}");
                self.dirty = true;
                Err(error.to_string())
            }
        }
    }

    pub(crate) fn poll_editor_requests(&mut self) {
        while let Some(request) = self.plugin_supervisor.poll_editor_request() {
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
            "editor.get_view" => Ok(serde_json::json!(self.editor.snapshot().view())),
            "editor.get_snapshot" => Ok(serde_json::json!(self.plugin_snapshot())),
            "editor.set_mode" => {
                let request: SetModeRequest =
                    serde_json::from_value(params).map_err(|error| (-32602, error.to_string()))?;
                self.editor.set_interaction_mode(request.mode);
                self.dirty = true;
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
            _ => Err((-32601, format!("method not found: {method}"))),
        }
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
        assert_eq!(model.editor.text().slice(0..3), "aXd");
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
        assert_eq!(model.editor.text().slice(0..4), "abcd");
        assert!(model.status.contains("stale revision"));
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
        assert!(model.status.contains("buffer mismatch"));
    }

    #[test]
    fn editor_service_round_trips_get_view() {
        let mut model = Model::new(EditorState::from_text("abcd"), None);
        model.plugin_supervisor.ensure_editor_service();
        let path = model
            .plugin_supervisor
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
}

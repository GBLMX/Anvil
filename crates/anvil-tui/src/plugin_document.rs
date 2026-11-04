//! Editor-side document snapshot and transaction contract for plugins.

use anvil_api::{ApplyEditRequest, DocumentSnapshot};
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

    fn plugin_buffer_id(&self) -> u64 {
        self.active as u64 + 1
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
}

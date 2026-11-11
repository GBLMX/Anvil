//! Remote control request handling for the TUI event loop.

use super::*;
use anvil_control::{ControlReply, ControlRequest, RpcError};
use anvil_ipc::codes;
use serde_json::{Value, json};
use std::path::Path;

const MAX_QUERY_TEXT: usize = 256 * 1024;

impl Model {
    pub(crate) fn handle_control(&mut self, request: ControlRequest) -> ControlReply {
        match request {
            ControlRequest::Status => Ok(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "workspace": self.workspace,
                "file": self.path,
                "dirty": self.modified,
            })),
            ControlRequest::Save { path } => self.remote_save(path.as_deref()),
            ControlRequest::Edit { mode, text } => self.remote_edit(&mode, &text),
            ControlRequest::Query => Ok(self.remote_query()),
            ControlRequest::Quit { force } => self.remote_quit(force),
            ControlRequest::Open { path, line, column } => self.remote_open(&path, line, column),
            ControlRequest::Exec { command } => self.remote_exec(&command),
        }
    }

    fn remote_open(&mut self, path: &Path, line: usize, column: usize) -> ControlReply {
        if !self.remote_ready() {
            return Err(RpcError::new(
                codes::UNAVAILABLE,
                "editor is busy; finish the current prompt first",
            ));
        }
        if self.help_back.is_some() {
            return Err(RpcError::new(
                codes::POLICY_DENIED,
                "close the read-only view first",
            ));
        }
        let absolute = self
            .absolute_path(path)
            .map_err(|error| RpcError::new(codes::INVALID_PARAMS, error))?;
        if !absolute.starts_with(&self.workspace) {
            return Err(RpcError::new(
                codes::POLICY_DENIED,
                format!("path is outside workspace: {}", path.display()),
            ));
        }
        if !absolute.is_file() {
            return Err(RpcError::new(
                codes::NOT_FOUND,
                format!("file not found: {}", path.display()),
            ));
        }
        if self.path.as_deref() != Some(absolute.as_path())
            && let Err(error) = self.open_path(&absolute)
        {
            return Err(RpcError::new(codes::INTERNAL_ERROR, error));
        }
        let line = line
            .max(1)
            .saturating_sub(1)
            .min(self.editor.index().len_lines().saturating_sub(1));
        self.cursor = self.offset_at_utf16(line, column.saturating_sub(1));
        self.status = format!(
            "remote open {}:{}:{}",
            self.path.as_deref().unwrap_or(absolute.as_path()).display(),
            line + 1,
            column.max(1)
        );
        self.dirty = true;
        Ok(json!({"accepted": true, "path": absolute}))
    }

    fn remote_exec(&mut self, command: &str) -> ControlReply {
        if !self.remote_ready() {
            return Err(RpcError::new(
                codes::UNAVAILABLE,
                "editor is busy; finish the current prompt first",
            ));
        }
        let command = command.trim().trim_start_matches(':').trim();
        if command.is_empty() {
            return Err(RpcError::new(codes::INVALID_PARAMS, "command is empty"));
        }
        self.execute_command(command);
        if self.status.starts_with("E:") {
            return Err(RpcError::new(codes::POLICY_DENIED, self.status.clone()));
        }
        Ok(json!({"accepted": true, "status": self.status}))
    }

    fn remote_save(&mut self, path: Option<&Path>) -> ControlReply {
        if !self.remote_ready() {
            return Err(RpcError::new(
                codes::UNAVAILABLE,
                "editor is busy; finish the current prompt first",
            ));
        }
        if self.help_back.is_some() {
            return Err(RpcError::new(
                codes::POLICY_DENIED,
                "close the read-only view first",
            ));
        }
        let saved = match path {
            Some(path) => {
                let absolute = self
                    .absolute_path(path)
                    .map_err(|error| RpcError::new(codes::INVALID_PARAMS, error))?;
                if !absolute.starts_with(&self.workspace) {
                    return Err(RpcError::new(
                        codes::POLICY_DENIED,
                        format!("path is outside workspace: {}", path.display()),
                    ));
                }
                self.save_to(absolute)
            }
            None => self.save(),
        };
        if !saved {
            return Err(RpcError::new(codes::INTERNAL_ERROR, self.status.clone()));
        }
        Ok(json!({
            "accepted": true,
            "path": self.path,
            "status": self.status,
        }))
    }

    fn remote_edit(&mut self, mode: &str, text: &str) -> ControlReply {
        if !self.remote_ready() {
            return Err(RpcError::new(
                codes::UNAVAILABLE,
                "editor is busy; finish the current prompt first",
            ));
        }
        if self.read_only || self.help_back.is_some() {
            return Err(RpcError::new(codes::POLICY_DENIED, "buffer is read-only"));
        }
        match mode {
            "set" => {
                let end = self.editor.text().len_bytes();
                let changed = self
                    .editor
                    .apply_transaction(vec![(0..end, text.to_owned())]);
                self.editor.set_selection_bytes(0, 0);
                self.cursor = 0;
                self.scroll = 0;
                self.hscroll = 0;
                if changed {
                    self.modified = true;
                }
                self.missing = false;
                self.syntax
                    .borrow_mut()
                    .switch_path(self.path.as_deref(), false);
                self.git_checked = None;
                self.git_due = Some(Instant::now());
                self.sync_lsp_active();
                self.status = "remote edit: set".to_owned();
            }
            "insert" => {
                if text.is_empty() {
                    return Err(RpcError::new(codes::INVALID_PARAMS, "text is empty"));
                }
                self.insert_text(text);
                self.status = "remote edit: insert".to_owned();
            }
            _ => {
                return Err(RpcError::new(
                    codes::INVALID_PARAMS,
                    "mode must be set or insert",
                ));
            }
        }
        self.dirty = true;
        Ok(json!({
            "accepted": true,
            "revision": self.editor.revision(),
            "modified": self.modified,
            "cursor": self.cursor,
        }))
    }

    fn remote_query(&mut self) -> Value {
        if self.help_back.is_none() {
            self.sync_active();
        }
        let full = self.editor.text().slice(0..self.editor.text().len_bytes());
        let truncated = full.len() > MAX_QUERY_TEXT;
        let mut text = full;
        if truncated {
            let mut end = MAX_QUERY_TEXT;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        let mut buffers = self
            .buffers
            .iter()
            .enumerate()
            .map(|(index, buffer)| {
                json!({
                    "path": buffer.path,
                    "modified": buffer.modified,
                    "active": index == self.active,
                })
            })
            .collect::<Vec<Value>>();
        if buffers.is_empty() {
            buffers.push(json!({
                "path": self.path,
                "modified": self.modified,
                "active": true,
            }));
        }
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "workspace": self.workspace,
            "path": self.path,
            "modified": self.modified,
            "read_only": self.read_only || self.help_back.is_some(),
            "cursor": self.cursor,
            "revision": self.editor.revision(),
            "line_count": self.editor.index().len_lines(),
            "undo_depth": self.editor.undo_depth(),
            "undo_bytes": self.editor.undo_bytes(),
            "selection": self.editor.selection_bytes(),
            "text": text,
            "text_truncated": truncated,
            "active": self.active,
            "buffers": buffers,
        })
    }

    fn remote_quit(&mut self, force: bool) -> ControlReply {
        if !force && self.any_modified() {
            return Err(RpcError::new(
                codes::POLICY_DENIED,
                "unsaved changes; pass --force to quit",
            ));
        }
        self.quit = true;
        Ok(json!({"accepted": true}))
    }

    fn remote_ready(&self) -> bool {
        // Read-only views still accept Ex commands such as :q and hunk actions.
        // File/save/edit operations guard help_back separately.
        matches!(self.ui.input_mode, InputMode::Normal)
            && self.command_palette.is_none()
            && self.fuzzy.is_none()
            && self.settings_overlay.is_none()
            && self.modal_layer.is_none()
            && self.ui.diagnostic_peek.is_none()
            && !self.ui.problems.visible
            && self.busy.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_editor::EditorState;

    #[test]
    fn opens_inside_workspace_and_reports_status() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("src/main.rs");
        std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        std::fs::write(&file, "one\ntwo\n").expect("write");
        let mut model = Model::new(EditorState::from_text("log"), None);
        model.workspace = dir.path().canonicalize().expect("canonical");
        let reply = model.handle_control(ControlRequest::Open {
            path: PathBuf::from("src/main.rs"),
            line: 99,
            column: 99,
        });
        assert!(reply.is_ok());
        assert_eq!(
            model.path.as_deref(),
            Some(file.canonicalize().expect("canonical").as_path())
        );
        assert_eq!(model.cursor, model.editor.text().len_bytes());

        let status = model
            .handle_control(ControlRequest::Status)
            .expect("status");
        assert_eq!(status["workspace"], serde_json::json!(model.workspace));
        assert_eq!(status["dirty"], false);
    }

    #[test]
    fn rejects_outside_workspace_and_unlisted_commands() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = dir.path().join("outside.rs");
        std::fs::write(&outside, "outside").expect("write");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("mkdir");
        let mut model = Model::new(EditorState::from_text("log"), None);
        model.workspace = workspace.canonicalize().expect("canonical");
        let reply = model.handle_control(ControlRequest::Open {
            path: PathBuf::from("../outside.rs"),
            line: 1,
            column: 1,
        });
        assert_eq!(reply.unwrap_err().code, codes::POLICY_DENIED);
        let reply = model.handle_control(ControlRequest::Exec {
            command: "!touch nope".to_owned(),
        });
        assert_eq!(reply.unwrap_err().code, codes::POLICY_DENIED);
    }

    #[test]
    fn rejects_requests_while_inserting() {
        let mut model = Model::new(EditorState::from_text("log"), None);
        model.ui.input_mode = InputMode::Insert;
        let reply = model.handle_control(ControlRequest::Status);
        assert!(reply.is_ok(), "status remains available");
        let reply = model.handle_control(ControlRequest::Exec {
            command: "q".to_owned(),
        });
        assert_eq!(reply.unwrap_err().code, codes::UNAVAILABLE);
    }
}

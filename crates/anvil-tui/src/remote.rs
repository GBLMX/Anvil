//! Remote control request handling for the TUI event loop.

use super::*;
use anvil_control::{
    BufferInfo, CAPABILITIES, CommandPolicyError, ControlReply, ControlRequest, EditMode,
    EditRange, EditResult, ExecResult, PROTOCOL_VERSION, QueryOptions, QueryResult, RpcError,
    Status, TextPoint, validate_remote_command,
};
use anvil_ipc::codes;
use serde::Serialize;
use serde_json::json;
use std::path::Path;

const MAX_QUERY_TEXT: usize = 256 * 1024;

fn reply<T: Serialize>(value: T) -> ControlReply {
    serde_json::to_value(value)
        .map_err(|error| RpcError::new(codes::INTERNAL_ERROR, error.to_string()))
}

impl Model {
    pub(crate) fn handle_control(&mut self, request: ControlRequest) -> ControlReply {
        match request {
            ControlRequest::Status => reply(Status {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                protocol_version: PROTOCOL_VERSION.to_owned(),
                workspace: self.workspace.path.clone(),
                file: self.session.path.clone(),
                dirty: self.file.modified,
                capabilities: CAPABILITIES
                    .iter()
                    .map(|capability| (*capability).to_owned())
                    .collect(),
            }),
            ControlRequest::Save { path } => self.remote_save(path.as_deref()),
            ControlRequest::Edit {
                mode,
                text,
                range,
                expected_revision,
            } => self.remote_edit(mode, &text, range, expected_revision),
            ControlRequest::Query(options) => reply(self.remote_query(options)),
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
        if self.buffers.help_back.is_some() {
            return Err(RpcError::new(
                codes::POLICY_DENIED,
                "close the read-only view first",
            ));
        }
        let absolute = self
            .absolute_path(path)
            .map_err(|error| RpcError::new(codes::INVALID_PARAMS, error))?;
        if !absolute.starts_with(&self.workspace.path) {
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
        if self.session.path.as_deref() != Some(absolute.as_path())
            && let Err(error) = self.open_path(&absolute)
        {
            return Err(RpcError::new(codes::INTERNAL_ERROR, error));
        }
        let line = line
            .max(1)
            .saturating_sub(1)
            .min(self.session.editor.index().len_lines().saturating_sub(1));
        self.session.cursor = self.offset_at_utf16(line, column.saturating_sub(1));
        self.status.text = format!(
            "remote open {}:{}:{}",
            self.session
                .path
                .as_deref()
                .unwrap_or(absolute.as_path())
                .display(),
            line + 1,
            column.max(1)
        );
        self.file.dirty = true;
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
        match validate_remote_command(command) {
            Ok(()) => {}
            Err(CommandPolicyError::Empty) => {
                return Err(RpcError::new(codes::INVALID_PARAMS, "command is empty"));
            }
            Err(error) => {
                return Err(RpcError::new(codes::POLICY_DENIED, error.to_string()));
            }
        }
        self.execute_command(command);
        if self.status.text.starts_with("E:") {
            return Err(RpcError::new(
                codes::POLICY_DENIED,
                self.status.text.clone(),
            ));
        }
        reply(ExecResult {
            accepted: true,
            status: self.status.text.clone(),
        })
    }

    fn remote_save(&mut self, path: Option<&Path>) -> ControlReply {
        if !self.remote_ready() {
            return Err(RpcError::new(
                codes::UNAVAILABLE,
                "editor is busy; finish the current prompt first",
            ));
        }
        if self.buffers.help_back.is_some() {
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
                if !absolute.starts_with(&self.workspace.path) {
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
            return Err(RpcError::new(
                codes::INTERNAL_ERROR,
                self.status.text.clone(),
            ));
        }
        Ok(json!({
            "accepted": true,
            "path": self.session.path,
            "status": self.status.text,
        }))
    }

    fn remote_edit(
        &mut self,
        mode: EditMode,
        text: &str,
        range: Option<EditRange>,
        expected_revision: Option<u64>,
    ) -> ControlReply {
        if !self.remote_ready() {
            return Err(RpcError::new(
                codes::UNAVAILABLE,
                "editor is busy; finish the current prompt first",
            ));
        }
        if self.read_only || self.buffers.help_back.is_some() {
            return Err(RpcError::new(codes::POLICY_DENIED, "buffer is read-only"));
        }
        match mode {
            EditMode::Set => {
                if range.is_some() || expected_revision.is_some() {
                    return Err(RpcError::new(
                        codes::INVALID_PARAMS,
                        "set does not accept a range or revision",
                    ));
                }
                let end = self.session.editor.text().len_bytes();
                let changed = self
                    .session
                    .editor
                    .apply_transaction(vec![(0..end, text.to_owned())]);
                self.session.editor.set_selection_bytes(0, 0);
                self.session.cursor = 0;
                self.view.scroll = 0;
                self.view.hscroll = 0;
                if changed {
                    self.file.modified = true;
                }
                self.file.missing = false;
                self.syntax_cache
                    .highlights
                    .borrow_mut()
                    .switch_path(self.session.path.as_deref(), false);
                self.git.diff_checked = None;
                self.git.diff_due = Some(Instant::now());
                self.sync_lsp_active();
                self.status.text = "remote edit: set".to_owned();
            }
            EditMode::Insert => {
                if range.is_some() || expected_revision.is_some() {
                    return Err(RpcError::new(
                        codes::INVALID_PARAMS,
                        "insert does not accept a range or revision",
                    ));
                }
                if text.is_empty() {
                    return Err(RpcError::new(codes::INVALID_PARAMS, "text is empty"));
                }
                self.insert_text(text);
                self.status.text = "remote edit: insert".to_owned();
            }
            EditMode::Replace => {
                let Some(range) = range else {
                    return Err(RpcError::new(
                        codes::INVALID_PARAMS,
                        "replace requires a range",
                    ));
                };
                let Some(expected_revision) = expected_revision else {
                    return Err(RpcError::new(
                        codes::INVALID_PARAMS,
                        "replace requires expected_revision",
                    ));
                };
                if expected_revision != self.session.editor.revision() {
                    return Err(RpcError::new(
                        codes::CONFLICT,
                        format!(
                            "revision changed: expected {expected_revision}, current {}",
                            self.session.editor.revision()
                        ),
                    ));
                }
                let start = self.offset_for_point(range.start)?;
                let end = self.offset_for_point(range.end)?;
                if start > end {
                    return Err(RpcError::new(
                        codes::INVALID_PARAMS,
                        "range start is after end",
                    ));
                }
                let changed = self
                    .session
                    .editor
                    .apply_transaction(vec![(start..end, text.to_owned())]);
                self.session.editor.set_selection_bytes(start, start);
                self.session.cursor = start;
                if changed {
                    self.file.modified = true;
                }
                self.file.missing = false;
                self.syntax_cache
                    .highlights
                    .borrow_mut()
                    .switch_path(self.session.path.as_deref(), false);
                self.git.diff_checked = None;
                self.git.diff_due = Some(Instant::now());
                self.sync_lsp_active();
                self.status.text = "remote edit: replace".to_owned();
            }
        }
        self.file.dirty = true;
        reply(EditResult {
            accepted: true,
            revision: self.session.editor.revision(),
            modified: self.file.modified,
            cursor: self.session.cursor,
        })
    }

    fn offset_for_point(&self, point: TextPoint) -> Result<usize, RpcError> {
        if point.line == 0 || point.byte_column == 0 {
            return Err(RpcError::new(
                codes::INVALID_PARAMS,
                "line and byte_column must be one-based",
            ));
        }
        let index = self.session.editor.index();
        let Some(line_start) = index.line_to_offset(point.line - 1) else {
            return Err(RpcError::new(
                codes::INVALID_PARAMS,
                format!("line {} is out of range", point.line),
            ));
        };
        let line_end = index
            .line_to_offset(point.line)
            .unwrap_or_else(|| self.session.editor.text().len_bytes());
        let max_column = line_end.saturating_sub(line_start) + 1;
        if point.byte_column > max_column {
            return Err(RpcError::new(
                codes::INVALID_PARAMS,
                format!("byte_column {} is past the line end", point.byte_column),
            ));
        }
        let offset = line_start + point.byte_column - 1;
        if offset > self.session.editor.text().len_bytes()
            || !self
                .session
                .editor
                .text()
                .slice(0..offset)
                .is_char_boundary(offset)
        {
            return Err(RpcError::new(
                codes::INVALID_PARAMS,
                "byte_column is not on a UTF-8 boundary",
            ));
        }
        Ok(offset)
    }

    fn remote_query(&mut self, options: QueryOptions) -> QueryResult {
        if self.buffers.help_back.is_none() {
            self.sync_active();
        }
        let full = self
            .session
            .editor
            .text()
            .slice(0..self.session.editor.text().len_bytes());
        let line_count = self.session.editor.index().len_lines();
        let start_line = options
            .start_line
            .unwrap_or(1)
            .max(1)
            .min(line_count.max(1));
        let end_line = options
            .end_line
            .unwrap_or(line_count.max(1))
            .max(start_line)
            .min(line_count.max(1));
        let start_offset = self
            .session
            .editor
            .index()
            .line_to_offset(start_line.saturating_sub(1))
            .unwrap_or(0);
        let end_offset = self
            .session
            .editor
            .index()
            .line_to_offset(end_line)
            .unwrap_or(full.len());
        let mut text = if options.include_text {
            full[start_offset.min(full.len())..end_offset.min(full.len())].to_owned()
        } else {
            String::new()
        };
        let truncated = text.len() > MAX_QUERY_TEXT;
        if truncated {
            let mut end = MAX_QUERY_TEXT;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        let mut buffers = self
            .buffers
            .entries
            .iter()
            .enumerate()
            .map(|(index, buffer)| BufferInfo {
                path: buffer.path.clone(),
                modified: buffer.modified,
                active: index == self.buffers.active,
            })
            .collect::<Vec<_>>();
        if buffers.is_empty() {
            buffers.push(BufferInfo {
                path: self.session.path.clone(),
                modified: self.file.modified,
                active: true,
            });
        }
        QueryResult {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            workspace: self.workspace.path.clone(),
            path: self.session.path.clone(),
            modified: self.file.modified,
            read_only: self.read_only || self.buffers.help_back.is_some(),
            cursor: self.session.cursor,
            revision: self.session.editor.revision(),
            line_count,
            undo_depth: self.session.editor.undo_depth(),
            undo_bytes: self.session.editor.undo_bytes(),
            selection: self.session.editor.selection_bytes(),
            text,
            text_truncated: truncated,
            start_line,
            end_line,
            active: self.buffers.active,
            buffers,
        }
    }

    fn remote_quit(&mut self, force: bool) -> ControlReply {
        if !force && self.any_modified() {
            return Err(RpcError::new(
                codes::POLICY_DENIED,
                "unsaved changes; pass --force to quit",
            ));
        }
        self.file.quit = true;
        Ok(json!({"accepted": true}))
    }

    fn remote_ready(&self) -> bool {
        // Read-only views still accept Ex commands such as :q and hunk actions.
        // File/save/edit operations guard help_back separately.
        matches!(self.ui.input_mode, InputMode::Normal)
            && self.overlays.command_palette.is_none()
            && self.overlays.fuzzy.is_none()
            && self.overlays.settings_overlay.is_none()
            && self.edit.modal_layer.is_none()
            && self.ui.diagnostic_peek.is_none()
            && !self.ui.problems.visible
            && self.status.busy.is_none()
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
        model.workspace.path = dir.path().canonicalize().expect("canonical");
        let reply = model.handle_control(ControlRequest::Open {
            path: PathBuf::from("src/main.rs"),
            line: 99,
            column: 99,
        });
        assert!(reply.is_ok());
        assert_eq!(
            model.session.path.as_deref(),
            Some(file.canonicalize().expect("canonical").as_path())
        );
        assert_eq!(
            model.session.cursor,
            model.session.editor.text().len_bytes()
        );

        let status = model
            .handle_control(ControlRequest::Status)
            .expect("status");
        assert_eq!(status["workspace"], serde_json::json!(model.workspace.path));
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
        model.workspace.path = workspace.canonicalize().expect("canonical");
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

    #[test]
    fn reports_protocol_capabilities() {
        let mut model = Model::new(EditorState::from_text("log"), None);
        let status = model
            .handle_control(ControlRequest::Status)
            .expect("status");
        assert_eq!(status["protocol_version"], "3");
        assert!(
            status["capabilities"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item == "query"))
        );
    }

    #[test]
    fn query_is_bounded_by_line_range() {
        let mut model = Model::new(EditorState::from_text("one\ntwo\nthree\n"), None);
        let value = model
            .handle_control(ControlRequest::Query(QueryOptions {
                start_line: Some(2),
                end_line: Some(2),
                include_text: true,
            }))
            .expect("query");
        let result: QueryResult = serde_json::from_value(value).expect("typed query");
        assert_eq!(result.start_line, 2);
        assert_eq!(result.end_line, 2);
        assert_eq!(result.text, "two\n");
        assert!(!result.text_truncated);
    }

    #[test]
    fn replace_is_revision_guarded() {
        let mut model = Model::new(EditorState::from_text("alpha beta"), None);
        let revision = model.session.editor.revision();
        let range = EditRange {
            start: TextPoint {
                line: 1,
                byte_column: 7,
            },
            end: TextPoint {
                line: 1,
                byte_column: 11,
            },
        };
        let value = model
            .handle_control(ControlRequest::Edit {
                mode: EditMode::Replace,
                text: "gamma".to_owned(),
                range: Some(range),
                expected_revision: Some(revision),
            })
            .expect("replace");
        let result: EditResult = serde_json::from_value(value).expect("typed edit");
        assert_eq!(
            model
                .session
                .editor
                .text()
                .slice(0..model.session.editor.text().len_bytes()),
            "alpha gamma"
        );
        let stale = model.handle_control(ControlRequest::Edit {
            mode: EditMode::Replace,
            text: "x".to_owned(),
            range: Some(range),
            expected_revision: Some(revision),
        });
        assert_eq!(stale.unwrap_err().code, codes::CONFLICT);
        assert_eq!(result.revision, model.session.editor.revision());
    }

    #[test]
    fn denies_admin_and_path_commands() {
        let mut model = Model::new(EditorState::from_text("log"), None);
        let admin = model.handle_control(ControlRequest::Exec {
            command: "pkg install https://example.invalid/x".to_owned(),
        });
        assert_eq!(admin.unwrap_err().code, codes::POLICY_DENIED);
        let path = model.handle_control(ControlRequest::Exec {
            command: "w ../outside".to_owned(),
        });
        assert_eq!(path.unwrap_err().code, codes::POLICY_DENIED);
    }
}

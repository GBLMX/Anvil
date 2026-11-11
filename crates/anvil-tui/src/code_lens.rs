//! LSP code lens state and response parsing.

use super::Model;
use crate::LspRequestKind;
use anvil_session::ui::NotificationSeverity;
use std::collections::HashMap;

pub(crate) const MAX_CODE_LENS: usize = 2048;
pub(crate) const MAX_CODE_LENS_PER_LINE: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeLensItem {
    pub(crate) line: usize,
    pub(crate) title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeLensState {
    pub(crate) revision: u64,
    pub(crate) items: Vec<CodeLensItem>,
}

impl Model {
    pub(crate) fn request_code_lens(&mut self) {
        self.pending_lsp
            .retain(|_, kind| *kind != LspRequestKind::CodeLens);
        self.code_lens = None;
        let Some(uri) = self.path.as_ref().map(|path| lsp_uri(path)) else {
            return;
        };
        if self.send_lsp_request(
            LspRequestKind::CodeLens,
            "textDocument/codeLens",
            serde_json::json!({"textDocument": {"uri": uri}}),
        ) {
            self.dirty = true;
        }
    }

    pub(crate) fn apply_code_lens_response(
        &mut self,
        result: &serde_json::Value,
        revision: Option<u64>,
    ) {
        let Some(items) = result.as_array() else {
            self.code_lens = None;
            self.status = "lsp: invalid code lens response".to_owned();
            self.dirty = true;
            return;
        };
        let mut per_line = HashMap::new();
        let mut lenses = Vec::new();
        for item in items {
            if lenses.len() >= MAX_CODE_LENS {
                break;
            }
            let Some(line) = item
                .get("range")
                .and_then(|range| range.get("start"))
                .and_then(|start| start.get("line"))
                .and_then(serde_json::Value::as_u64)
                .map(|line| line as usize)
            else {
                continue;
            };
            let Some(title) = item
                .get("command")
                .and_then(|command| command.get("title"))
                .and_then(serde_json::Value::as_str)
                .filter(|title| !title.is_empty())
            else {
                continue;
            };
            let count = per_line.entry(line).or_insert(0usize);
            if *count >= MAX_CODE_LENS_PER_LINE {
                continue;
            }
            *count += 1;
            lenses.push(CodeLensItem {
                line,
                title: title.to_owned(),
            });
        }
        lenses.sort_by_key(|lens| lens.line);
        let revision = revision.unwrap_or_else(|| self.editor.revision());
        self.code_lens = Some(CodeLensState {
            revision,
            items: lenses.clone(),
        });
        self.status = format!("code lens: {}", lenses.len());
        self.notify(
            NotificationSeverity::Info,
            "lsp",
            format!("code lens: {}", lenses.len()),
        );
        self.dirty = true;
    }
}

fn lsp_uri(path: &std::path::Path) -> String {
    crate::lsp_worker::file_uri(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp_worker;
    use anvil_editor::EditorState;
    use std::path::PathBuf;

    #[test]
    fn parses_bounded_code_lenses_and_ignores_missing_commands() {
        let mut model = Model::new(EditorState::from_text("fn main() {}"), None);
        model.path = Some(PathBuf::from("main.rs"));
        model.pending_lsp.insert(31, LspRequestKind::CodeLens);
        model.handle_lsp_response(
            31,
            serde_json::json!([
                {"range": {"start": {"line": 0, "character": 0}}, "command": {"title": "Run"}},
                {"range": {"start": {"line": 0, "character": 0}}, "command": {"title": "Test"}},
                {"range": {"start": {"line": 0, "character": 0}}, "command": {"title": "Debug"}},
                {"range": {"start": {"line": 0, "character": 0}}, "command": {"title": "Ignored"}},
                {"range": {"start": {"line": 1, "character": 0}}}
            ]),
            None,
        );
        let state = model.code_lens.as_ref().expect("code lens");
        assert_eq!(state.items.len(), 3);
        assert_eq!(state.items[0].title, "Run");
        assert_eq!(state.items[2].title, "Debug");
        assert_eq!(
            lsp_worker::file_uri(std::path::Path::new("main.rs")),
            lsp_uri(std::path::Path::new("main.rs"))
        );
    }

    #[test]
    fn stale_response_uses_explicit_revision() {
        let mut model = Model::new(EditorState::from_text("fn main() {}"), None);
        model.path = Some(PathBuf::from("main.rs"));
        model.pending_lsp.insert(32, LspRequestKind::CodeLens);
        model.handle_lsp_response(
            32,
            serde_json::json!([
                {"range": {"start": {"line": 0, "character": 0}}, "command": {"title": "Run"}}
            ]),
            None,
        );
        assert_eq!(model.code_lens.as_ref().expect("lens").revision, 0);
    }
}

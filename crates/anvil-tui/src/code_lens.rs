//! LSP code lens state and response parsing.

use super::Model;
use crate::LspRequestKind;
use anvil_session::ui::NotificationSeverity;

pub(crate) const MAX_CODE_LENS: usize = 2048;
pub(crate) const MAX_CODE_LENS_PER_LINE: usize = 3;
pub(crate) const MAX_CODE_LENS_ROWS_PER_VIEWPORT: usize = 32;

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
    pub(crate) fn code_lens_for_line(&self, line: usize) -> Vec<&CodeLensItem> {
        let Some(state) = &self.code_lens else {
            return Vec::new();
        };
        if state.revision != self.editor.revision() {
            return Vec::new();
        }
        state
            .items
            .iter()
            .filter(|item| item.line == line)
            .collect()
    }
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
        assert_eq!(state.items.len(), 4);
        assert_eq!(state.items[0].title, "Run");
        assert_eq!(state.items[3].title, "Ignored");
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

    #[test]
    fn virtual_rows_are_bounded_and_keep_source_mapping() {
        use crate::layout::DisplayRow;

        let mut model = Model::new(EditorState::from_text("a\nb\nc\n"), None);
        model.code_lens = Some(CodeLensState {
            revision: model.editor.revision(),
            items: (0..5)
                .map(|index| CodeLensItem {
                    line: 0,
                    title: format!("Lens {index}"),
                })
                .collect(),
        });
        let rows = model.visible_rows(0, 10);
        assert!(matches!(
            &rows[0],
            DisplayRow::CodeLens { title, .. } if title == "Lens 0"
        ));
        assert!(matches!(
            &rows[1],
            DisplayRow::CodeLens { title, .. } if title == "Lens 1"
        ));
        assert!(matches!(
            &rows[2],
            DisplayRow::CodeLens { title, .. } if title.contains("+3 lenses")
        ));
        assert_eq!(rows[3], DisplayRow::Source(0));
        assert_eq!(model.row_of_source_line(0, 10), Some(3));
    }

    #[test]
    fn code_lens_titles_render_above_source() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut model = Model::new(EditorState::from_text("fn main() {}\n"), None);
        model.code_lens = Some(CodeLensState {
            revision: model.editor.revision(),
            items: vec![CodeLensItem {
                line: 0,
                title: "Run test".to_owned(),
            }],
        });
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| crate::view(&model, frame))
            .expect("draw");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Run test"), "{rendered}");
        assert!(rendered.contains("fn main"), "{rendered}");
    }
}

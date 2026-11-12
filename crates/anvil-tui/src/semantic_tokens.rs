//! LSP semantic tokens and theme mapping.

use super::*;
use crate::{lsp_worker, theme};

impl Model {
    pub(crate) fn request_semantic_tokens(&mut self) {
        if self.semantic_legend.is_empty() {
            return;
        }
        self.language
            .pending_lsp
            .retain(|_, kind| *kind != LspRequestKind::SemanticTokens);
        self.prune_edit_requests();
        let Some(path) = self.path.clone() else {
            return;
        };
        let params = serde_json::json!({
            "textDocument": {"uri": lsp_worker::file_uri(&path)}
        });
        if self.send_lsp_request(
            LspRequestKind::SemanticTokens,
            "textDocument/semanticTokens/full",
            params,
        ) {
            self.status = "semantic tokens".to_owned();
            self.dirty = true;
        }
    }

    pub(crate) fn apply_semantic_tokens_response(&mut self, result: &serde_json::Value) {
        let Some(data) = result.get("data").and_then(serde_json::Value::as_array) else {
            self.semantic_tokens.clear();
            self.semantic_revision = None;
            self.status = "lsp: no semantic tokens".to_owned();
            return;
        };
        let mut tokens = Vec::new();
        let mut line = 0usize;
        let mut start = 0usize;
        for chunk in data.chunks(5) {
            if tokens.len() >= 4096 {
                break;
            }
            let [delta_line, delta_start, length, type_index, _modifiers] = chunk else {
                break;
            };
            let Some(delta_line) = delta_line.as_u64() else {
                continue;
            };
            let Some(delta_start) = delta_start.as_u64() else {
                continue;
            };
            let Some(length) = length.as_u64() else {
                continue;
            };
            let Some(type_index) = type_index.as_u64() else {
                continue;
            };
            if delta_line > 0 {
                line += delta_line as usize;
                start = delta_start as usize;
            } else {
                start += delta_start as usize;
            }
            let start_offset = self.offset_at_utf16(line, start);
            let end_offset = self.offset_at_utf16(line, start + length as usize);
            if end_offset > start_offset {
                tokens.push(SemanticToken {
                    start: start_offset,
                    end: end_offset,
                    type_index: type_index as usize,
                });
            }
        }
        self.semantic_tokens = tokens;
        self.semantic_revision = if self.semantic_tokens.is_empty() {
            None
        } else {
            Some(self.editor.revision())
        };
        self.status = format!("semantic tokens: {}", self.semantic_tokens.len());
        self.dirty = true;
    }

    pub(crate) fn clear_semantic_tokens(&mut self) {
        self.language
            .pending_lsp
            .retain(|_, kind| *kind != LspRequestKind::SemanticTokens);
        self.semantic_tokens.clear();
        self.semantic_revision = None;
    }

    pub(crate) fn semantic_ranges(
        &self,
        line_start: usize,
        line_end: usize,
    ) -> Vec<(usize, usize, Style)> {
        if self.semantic_revision != Some(self.editor.revision()) {
            return Vec::new();
        }
        self.semantic_tokens
            .iter()
            .filter_map(|token| {
                if token.end <= line_start || token.start >= line_end {
                    return None;
                }
                let style = self.semantic_style(token.type_index)?;
                Some((token.start.max(line_start), token.end.min(line_end), style))
            })
            .collect()
    }

    fn semantic_style(&self, type_index: usize) -> Option<Style> {
        let name = self.semantic_legend.get(type_index)?.to_ascii_lowercase();
        let color = match name.as_str() {
            "keyword" => theme::syntax_keyword(),
            "string" => theme::syntax_string(),
            "number" => theme::syntax_number(),
            "comment" => theme::syntax_comment(),
            "function" | "method" | "macro" => theme::syntax_function(),
            "type" | "class" | "struct" | "enum" | "interface" | "trait" | "namespace"
            | "typeparameter" => theme::syntax_type(),
            "property" | "field" | "parameter" | "variable" => theme::syntax_property(),
            "operator" | "punctuation" => theme::muted(),
            "decorator" | "attribute" => theme::syntax_emphasis(),
            _ => return None,
        };
        Some(Style::default().fg(color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_and_maps_semantic_tokens() {
        let mut model = crate::Model::new(crate::EditorState::from_text("fn main() {}"), None);
        model.semantic_legend = vec!["keyword".to_owned(), "function".to_owned()];
        model.apply_semantic_tokens_response(&serde_json::json!({
            "data": [
                0, 0, 2, 0, 0,
                0, 3, 4, 1, 0
            ]
        }));
        let ranges = model.semantic_ranges(0, 12);
        assert_eq!(ranges.len(), 2);
        assert_eq!((ranges[0].0, ranges[0].1), (0, 2));
        assert_eq!((ranges[1].0, ranges[1].1), (3, 7));
        assert_eq!(ranges[0].2.fg, Some(theme::syntax_keyword()));
    }

    #[test]
    fn empty_or_stale_tokens_are_not_rendered() {
        let mut model = crate::Model::new(crate::EditorState::from_text("x"), None);
        model.semantic_legend = vec!["keyword".to_owned()];
        model.apply_semantic_tokens_response(&serde_json::json!({"data": []}));
        assert!(model.semantic_revision.is_none());
        assert!(model.semantic_ranges(0, 1).is_empty());
        model.insert_text("y");
        assert!(model.semantic_ranges(0, 2).is_empty());
    }
}

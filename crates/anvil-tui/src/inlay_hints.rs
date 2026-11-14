//! LSP inlay hints cached for the visible viewport.

use super::*;
use crate::theme;
use anvil_lsp_client::file_uri;

impl Model {
    pub(crate) fn poll_inlay_hints(&mut self, height: u16) {
        if !self.config.settings.lsp.inlay_hints {
            self.overlays.lsp_results.inlay_hints.clear();
            self.overlays.lsp_results.inlay_pending = None;
            self.overlays.lsp_results.inlay_checked = None;
            return;
        }
        let Some(path) = self.session.path.clone() else {
            self.overlays.lsp_results.inlay_hints.clear();
            return;
        };
        let supported = self
            .language
            .lsp
            .config
            .as_ref()
            .and_then(|key| self.overlays.lsp_results.inlay_support.get(key))
            .copied()
            .unwrap_or(false);
        if !supported {
            self.overlays.lsp_results.inlay_hints.clear();
            self.overlays.lsp_results.inlay_pending = None;
            return;
        }
        let revision = self.session.editor.revision();
        let start = self.view.scroll;
        let end = visible_end_line(
            start,
            usize::from(height),
            self.session.editor.index().len_lines(),
        );
        let signature = (path.clone(), revision, start, end);
        if let Some(pending) = &self.overlays.lsp_results.inlay_pending {
            if (pending.1.clone(), pending.2, pending.3, pending.4) != signature {
                self.overlays.lsp_results.inlay_pending = None;
                self.overlays.lsp_results.inlay_hints.clear();
            } else {
                return;
            }
        }
        if self.overlays.lsp_results.inlay_checked.as_ref() == Some(&signature) {
            return;
        }
        if self.overlays.lsp_results.inlay_checked.as_ref() != Some(&signature) {
            self.overlays.lsp_results.inlay_hints.clear();
        }
        let params = serde_json::json!({
            "textDocument": {"uri": file_uri(&path)},
            "range": {
                "start": {"line": start, "character": 0},
                "end": {"line": end, "character": 0}
            }
        });
        let id = self.language.lsp_requests.next_request;
        if self.send_lsp_request(LspRequestKind::InlayHint, "textDocument/inlayHint", params) {
            self.overlays.lsp_results.inlay_pending = Some((id, path, revision, start, end));
            self.overlays.lsp_results.inlay_checked = Some(signature);
        }
    }

    pub(crate) fn apply_inlay_hints_response(&mut self, id: u64, result: &serde_json::Value) {
        if self
            .overlays
            .lsp_results
            .inlay_pending
            .as_ref()
            .map(|pending| pending.0)
            != Some(id)
        {
            return;
        }
        let Some((_, path, revision, _, _)) = self.overlays.lsp_results.inlay_pending.take() else {
            return;
        };
        if self.session.path.as_ref() != Some(&path) || self.session.editor.revision() != revision {
            self.overlays.lsp_results.inlay_hints.clear();
            self.file.dirty = true;
            return;
        }
        let Some(items) = result.as_array() else {
            self.overlays.lsp_results.inlay_hints.clear();
            return;
        };
        let mut hints = Vec::new();
        for item in items {
            let Some(position) = item.get("position") else {
                continue;
            };
            let Some(line) = position
                .get("line")
                .and_then(serde_json::Value::as_u64)
                .map(|line| line as usize)
            else {
                continue;
            };
            let Some(character) = position
                .get("character")
                .and_then(serde_json::Value::as_u64)
                .map(|character| character as usize)
            else {
                continue;
            };
            let label = item.get("label").map(inlay_label).unwrap_or_default();
            if label.is_empty() {
                continue;
            }
            hints.push(InlayHint {
                line,
                character,
                label,
            });
        }
        hints.sort_by_key(|hint| (hint.line, hint.character));
        self.overlays.lsp_results.inlay_hints = hints;
        self.file.dirty = true;
    }

    pub(crate) fn insert_inlay_hints(
        &self,
        line: usize,
        visible_start: usize,
        visible_end: usize,
        spans: &mut Vec<Span<'_>>,
    ) {
        let mut hints = self
            .overlays
            .lsp_results
            .inlay_hints
            .iter()
            .filter(|hint| hint.line == line)
            .collect::<Vec<_>>();
        hints.sort_by_key(|hint| std::cmp::Reverse(hint.character));
        for hint in hints {
            let offset = self.offset_at_utf16(line, hint.character);
            if offset < visible_start || offset > visible_end {
                continue;
            }
            insert_virtual_span(spans, offset - visible_start, hint.label.clone());
        }
    }
}

fn visible_end_line(start: usize, height: usize, line_count: usize) -> usize {
    (start + height)
        .min(line_count.saturating_sub(1))
        .max(start)
}

fn inlay_label(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(label) => label.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("value").and_then(serde_json::Value::as_str))
            .collect(),
        _ => String::new(),
    }
}

fn insert_virtual_span(spans: &mut Vec<Span<'_>>, at: usize, label: String) {
    let style = Style::default().fg(theme::muted());
    let mut offset = 0;
    for index in 0..spans.len() {
        let length = spans[index].content.len();
        if at <= offset + length {
            let local = at - offset;
            if local == 0 {
                spans.insert(index, Span::styled(label, style));
                return;
            }
            if local == length {
                spans.insert(index + 1, Span::styled(label, style));
                return;
            }
            let content = spans[index].content.to_string();
            let original_style = spans[index].style;
            spans[index] = Span::styled(content[..local].to_owned(), original_style);
            spans.insert(index + 1, Span::styled(label, style));
            spans.insert(
                index + 2,
                Span::styled(content[local..].to_owned(), original_style),
            );
            return;
        }
        offset += length;
    }
    spans.push(Span::styled(label, style));
}

#[cfg(test)]
mod tests {
    use super::{inlay_label, insert_virtual_span, visible_end_line};
    use ratatui::text::Span;

    #[test]
    fn parses_string_and_label_parts() {
        assert_eq!(inlay_label(&serde_json::json!(": i32")), ": i32");
        assert_eq!(
            inlay_label(&serde_json::json!([{"value": ": "}, {"value": "i32"}])),
            ": i32"
        );
    }

    #[test]
    fn inserts_virtual_text_without_mutating_offsets() {
        let mut spans = vec![Span::raw("let x = 1;")];
        insert_virtual_span(&mut spans, 5, ": i32".to_owned());
        let text = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(text, "let x: i32 = 1;");
    }

    #[test]
    fn viewport_range_never_exceeds_the_last_line() {
        assert_eq!(visible_end_line(0, 24, 4), 3);
        assert_eq!(visible_end_line(0, 24, 0), 0);
        assert_eq!(visible_end_line(3, 24, 4), 3);
    }
}

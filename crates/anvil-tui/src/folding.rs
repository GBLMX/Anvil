//! LSP folding ranges, folded-line visibility, and fold commands.

use super::*;
use crate::lsp_worker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FoldRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FoldAction {
    Toggle(usize),
    CloseAll,
}

impl Model {
    pub(crate) fn request_folding_ranges(&mut self, action: Option<FoldAction>) {
        self.language
            .lsp_requests
            .pending
            .retain(|_, kind| *kind != LspRequestKind::FoldingRange);
        self.prune_edit_requests();
        let Some(path) = self.session.path.clone() else {
            return;
        };
        let params = serde_json::json!({
            "textDocument": {"uri": lsp_worker::file_uri(&path)}
        });
        if self.send_lsp_request(
            LspRequestKind::FoldingRange,
            "textDocument/foldingRange",
            params,
        ) {
            self.overlays.lsp_results.pending_fold_action = action;
            self.status.text = "folds".to_owned();
        } else {
            self.overlays.lsp_results.pending_fold_action = None;
        }
        self.dirty = true;
    }

    pub(crate) fn clear_folds(&mut self) {
        self.language
            .lsp_requests
            .pending
            .retain(|_, kind| *kind != LspRequestKind::FoldingRange);
        self.overlays.lsp_results.folds.clear();
        self.overlays.lsp_results.folded.clear();
        self.overlays.lsp_results.fold_revision = None;
        self.overlays.lsp_results.pending_fold_action = None;
    }

    pub(crate) fn apply_folding_response(&mut self, result: &serde_json::Value) {
        let action = self.overlays.lsp_results.pending_fold_action.take();
        let previous_folded = std::mem::take(&mut self.overlays.lsp_results.folded);
        let Some(ranges) = result.as_array() else {
            self.overlays.lsp_results.folds.clear();
            self.overlays.lsp_results.folded.clear();
            self.status.text = "lsp: no folds".to_owned();
            return;
        };
        let mut folds = Vec::new();
        for range in ranges {
            if folds.len() >= 4096 {
                break;
            }
            let Some(start) = range
                .get("startLine")
                .and_then(serde_json::Value::as_u64)
                .map(|line| line as usize)
            else {
                continue;
            };
            let Some(end) = range
                .get("endLine")
                .and_then(serde_json::Value::as_u64)
                .map(|line| line as usize)
            else {
                continue;
            };
            if end > start {
                folds.push(FoldRange { start, end });
            }
        }
        folds.sort_by_key(|fold| (fold.start, fold.end));
        folds.dedup_by_key(|fold| (fold.start, fold.end));
        self.overlays.lsp_results.folds = folds;
        self.overlays.lsp_results.folded = previous_folded
            .into_iter()
            .filter(|start| {
                self.overlays
                    .lsp_results
                    .folds
                    .iter()
                    .any(|fold| fold.start == *start)
            })
            .collect();
        self.overlays.lsp_results.fold_revision = if self.overlays.lsp_results.folds.is_empty() {
            None
        } else {
            Some(self.session.editor.revision())
        };
        match action {
            Some(FoldAction::Toggle(line)) => self.toggle_fold_line(line),
            Some(FoldAction::CloseAll) => {
                self.overlays.lsp_results.folded = self
                    .overlays
                    .lsp_results
                    .folds
                    .iter()
                    .map(|fold| fold.start)
                    .collect()
            }
            None => {}
        }
        self.normalize_scroll();
        let count = self.overlays.lsp_results.folds.len();
        self.status.text = if count == 0 {
            "lsp: no folds".to_owned()
        } else {
            format!("folds: {count}")
        };
        self.dirty = true;
    }

    pub(crate) fn toggle_fold_at_cursor(&mut self) {
        let Some(line) = self.current_line() else {
            return;
        };
        if !self.folds_fresh() {
            self.request_folding_ranges(Some(FoldAction::Toggle(line)));
            return;
        }
        self.toggle_fold_line(line);
    }

    pub(crate) fn close_all_folds(&mut self) {
        if !self.folds_fresh() {
            self.request_folding_ranges(Some(FoldAction::CloseAll));
            return;
        }
        self.overlays.lsp_results.folded = self
            .overlays
            .lsp_results
            .folds
            .iter()
            .map(|fold| fold.start)
            .collect();
        self.move_cursor_out_of_hidden();
        self.normalize_scroll();
        self.status.text = format!("folds closed: {}", self.overlays.lsp_results.folded.len());
        self.dirty = true;
    }

    pub(crate) fn open_all_folds(&mut self) {
        self.overlays.lsp_results.folded.clear();
        self.status.text = "folds opened".to_owned();
        self.dirty = true;
    }

    fn toggle_fold_line(&mut self, line: usize) {
        let Some(start) = self.fold_start_containing(line) else {
            self.status.text = "no fold".to_owned();
            self.dirty = true;
            return;
        };
        if !self.overlays.lsp_results.folded.insert(start) {
            self.overlays.lsp_results.folded.remove(&start);
            self.status.text = "fold opened".to_owned();
        } else {
            self.status.text = "fold closed".to_owned();
        }
        self.move_cursor_out_of_hidden();
        self.normalize_scroll();
        self.dirty = true;
    }

    pub(crate) fn fold_start_containing(&self, line: usize) -> Option<usize> {
        if !self.folds_fresh() {
            return None;
        }
        self.overlays
            .lsp_results
            .folds
            .iter()
            .filter(|fold| fold.start <= line && line <= fold.end)
            .max_by_key(|fold| fold.start)
            .map(|fold| fold.start)
    }

    pub(crate) fn folds_fresh(&self) -> bool {
        self.overlays.lsp_results.fold_revision == Some(self.session.editor.revision())
    }

    pub(crate) fn is_line_hidden(&self, line: usize) -> bool {
        self.folds_fresh()
            && self.overlays.lsp_results.folds.iter().any(|fold| {
                self.overlays.lsp_results.folded.contains(&fold.start)
                    && fold.start < line
                    && line <= fold.end
            })
    }

    pub(crate) fn has_closed_folds(&self) -> bool {
        self.folds_fresh() && !self.overlays.lsp_results.folded.is_empty()
    }

    pub(crate) fn visible_line_numbers(&self, start: usize, height: usize) -> Vec<usize> {
        if height == 0 {
            return Vec::new();
        }
        let total = self.session.editor.index().len_lines();
        let mut line = self.first_visible_line(start);
        let mut lines = Vec::with_capacity(height.min(total));
        while lines.len() < height && line < total {
            lines.push(line);
            line = self.next_visible_line(line);
        }
        lines
    }

    pub(crate) fn normalize_scroll(&mut self) {
        let total = self.session.editor.index().len_lines();
        self.view.scroll = self
            .first_visible_line(self.view.scroll)
            .min(total.saturating_sub(1));
        self.view.scroll_subrow = 0;
    }

    pub(crate) fn open_fold_containing(&mut self, line: usize) {
        if let Some(start) = self.fold_start_containing(line) {
            self.overlays.lsp_results.folded.remove(&start);
            self.dirty = true;
        }
    }

    fn move_cursor_out_of_hidden(&mut self) {
        if let Some(line) = self.current_line()
            && self.is_line_hidden(line)
            && let Some(start) = self.fold_start_containing(line)
        {
            self.session.cursor = self.offset_at_utf16(start, 0);
        }
    }

    pub(crate) fn first_visible_line(&self, mut line: usize) -> usize {
        let total = self.session.editor.index().len_lines();
        line = line.min(total.saturating_sub(1));
        while self.is_line_hidden(line) {
            let Some(fold) = self
                .overlays
                .lsp_results
                .folds
                .iter()
                .filter(|fold| {
                    self.overlays.lsp_results.folded.contains(&fold.start)
                        && fold.start < line
                        && line <= fold.end
                })
                .max_by_key(|fold| fold.start)
            else {
                break;
            };
            if fold.end + 1 >= total {
                return total;
            }
            line = fold.end + 1;
        }
        line
    }

    pub(crate) fn fold_marker(&self, line: usize) -> Option<char> {
        if !self.folds_fresh() {
            return None;
        }
        let fold = self
            .overlays
            .lsp_results
            .folds
            .iter()
            .find(|fold| fold.start == line)?;
        Some(if self.overlays.lsp_results.folded.contains(&fold.start) {
            '▸'
        } else {
            '▾'
        })
    }

    pub(crate) fn next_visible_line(&self, line: usize) -> usize {
        if self.folds_fresh()
            && let Some(fold) = self.overlays.lsp_results.folds.iter().find(|fold| {
                fold.start == line && self.overlays.lsp_results.folded.contains(&fold.start)
            })
        {
            return fold.end + 1;
        }
        line + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folded_model() -> Model {
        let text = (0..12)
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let mut model = Model::new(crate::EditorState::from_text(&text), None);
        model.overlays.lsp_results.folds = vec![
            FoldRange { start: 1, end: 4 },
            FoldRange { start: 6, end: 9 },
        ];
        model.overlays.lsp_results.fold_revision = Some(model.session.editor.revision());
        model
    }

    #[test]
    fn closed_folds_hide_inner_lines() {
        let mut model = folded_model();
        model.overlays.lsp_results.folded.insert(1);
        assert!(model.is_line_hidden(2));
        assert!(!model.is_line_hidden(1));
        assert_eq!(model.visible_line_numbers(0, 6), vec![0, 1, 5, 6, 7, 8]);
        assert_eq!(model.fold_marker(1), Some('▸'));
        let rendered = model.visible_lines(10, 80);
        assert_eq!(rendered.len(), 9);
        let texts: Vec<String> = rendered
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();
        assert_eq!(texts[2], "5");
        model.view.scroll = 2;
        model.normalize_scroll();
        assert_eq!(model.view.scroll, 5);
        model.toggle_fold_line(2);
        assert!(!model.is_line_hidden(2));
    }

    #[test]
    fn folding_response_closes_through_action() {
        let mut model = folded_model();
        model.overlays.lsp_results.folds.clear();
        model.overlays.lsp_results.fold_revision = None;
        model.overlays.lsp_results.pending_fold_action = Some(FoldAction::CloseAll);
        model.apply_folding_response(&serde_json::json!([
            {"startLine": 1, "endLine": 4},
            {"startLine": 6, "endLine": 9}
        ]));
        assert_eq!(model.overlays.lsp_results.folds.len(), 2);
        assert_eq!(model.overlays.lsp_results.folded.len(), 2);
        assert!(model.is_line_hidden(7));
    }

    #[test]
    fn unloaded_folds_request_on_toggle() {
        let mut model = Model::new(
            crate::EditorState::from_text("fn main() {}\n"),
            Some(std::path::PathBuf::from("main.rs")),
        );
        model.toggle_fold_at_cursor();
        assert!(!model.folds_fresh());
        assert_eq!(model.status.text, "E: lsp is not running");
        assert!(!model.status.text.contains("no fold"));
    }

    #[test]
    fn empty_folding_response_is_not_fresh() {
        let mut model = folded_model();
        model.overlays.lsp_results.fold_revision = None;
        model.apply_folding_response(&serde_json::json!([]));
        assert!(model.overlays.lsp_results.folds.is_empty());
        assert!(!model.folds_fresh());
    }
}

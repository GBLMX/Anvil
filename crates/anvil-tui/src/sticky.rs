//! Sticky scroll header derived from LSP document symbols.

use super::*;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

const STICKY_DEBOUNCE: Duration = Duration::from_millis(400);
const STICKY_MAX_DEPTH: usize = 8;

impl Model {
    pub(crate) fn poll_sticky_symbols(&mut self) {
        let Some(path) = self.session.path.clone() else {
            return;
        };
        if self.overlays.lsp_results.sticky_pending {
            return;
        }
        if self.overlays.lsp_results.sticky_symbols_path.as_ref() == Some(&path)
            && self.overlays.lsp_results.sticky_symbols_revision
                == Some(self.session.editor.revision())
        {
            return;
        }
        if self
            .overlays
            .lsp_results
            .sticky_checked_at
            .is_some_and(|checked| checked.elapsed() < STICKY_DEBOUNCE)
        {
            return;
        }
        if self.language.lsp.config.is_none() {
            return;
        }
        self.overlays.lsp_results.sticky_checked_at = Some(Instant::now());
        let params = serde_json::json!({
            "textDocument": {"uri": anvil_lsp_client::file_uri(&path)},
        });
        self.overlays.lsp_results.sticky_pending = self.send_lsp_request(
            LspRequestKind::StickySymbols,
            "textDocument/documentSymbol",
            params,
        );
    }

    pub(crate) fn sticky_context(&self, top_line: usize) -> Option<&SymbolItem> {
        if self.overlays.lsp_results.sticky_symbols_path.as_ref() != self.session.path.as_ref() {
            return None;
        }
        let mut active = None;
        for (index, symbol) in self.overlays.lsp_results.sticky_symbols.iter().enumerate() {
            if symbol.line >= top_line {
                break;
            }
            let end = self
                .overlays
                .lsp_results
                .sticky_symbols
                .iter()
                .skip(index + 1)
                .find(|next| next.depth <= symbol.depth)
                .map_or_else(|| self.session.editor.index().len_lines(), |next| next.line);
            if top_line < end
                && active.is_none_or(|current: &SymbolItem| symbol.depth >= current.depth)
            {
                active = Some(symbol);
            }
        }
        active
    }
}

pub(crate) fn render(model: &Model, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let Some(symbol) = model.sticky_context(model.view.scroll) else {
        return;
    };
    let indent = "  ".repeat(symbol.depth.min(STICKY_MAX_DEPTH));
    let detail = symbol
        .detail
        .as_ref()
        .map(|detail| format!(" · {detail}"))
        .unwrap_or_default();
    let width = usize::from(area.width);
    let text = crate::status_line::middle_ellipsis(
        &format!("{indent}{}{detail}", symbol.name),
        width.saturating_sub(2),
    );
    debug_assert!(UnicodeWidthStr::width(text.as_str()) <= width);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(theme::accent())),
            Span::styled(
                text,
                theme::text()
                    .bg(theme::surface())
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .style(theme::surface()),
        Rect { height: 1, ..area },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(name: &str, line: usize, depth: usize) -> SymbolItem {
        SymbolItem {
            name: name.to_owned(),
            detail: None,
            line,
            character: 0,
            depth,
        }
    }

    #[test]
    fn picks_the_deepest_enclosing_symbol() {
        let path = PathBuf::from("main.rs");
        let mut model = Model::new(EditorState::from_text("a\nb\nc\nd\ne"), Some(path.clone()));
        model.overlays.lsp_results.sticky_symbols_path = Some(path);
        model.overlays.lsp_results.sticky_symbols = vec![
            symbol("impl Foo", 0, 0),
            symbol("fn run", 1, 1),
            symbol("if ready", 2, 2),
            symbol("fn next", 3, 0),
        ];
        assert_eq!(
            model.sticky_context(2).map(|symbol| symbol.name.as_str()),
            Some("fn run")
        );
        assert_eq!(
            model.sticky_context(4).map(|symbol| symbol.name.as_str()),
            Some("fn next")
        );
        assert!(model.sticky_context(0).is_none());
    }

    #[test]
    fn renders_the_context_header_over_the_top_row() {
        let path = PathBuf::from("main.rs");
        let mut model = Model::new(EditorState::from_text("a\nb\nc\nd\ne"), Some(path.clone()));
        model.overlays.lsp_results.sticky_symbols_path = Some(path);
        model.overlays.lsp_results.sticky_symbols = vec![symbol("fn run", 1, 1)];
        model.view.scroll = 2;
        let backend = ratatui::backend::TestBackend::new(40, 6);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| crate::render::view(&model, frame))
            .expect("draw");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("fn run"), "{rendered}");
    }

    #[test]
    fn skips_symbols_from_another_file() {
        let mut model = Model::new(
            EditorState::from_text("a\nb"),
            Some(PathBuf::from("one.rs")),
        );
        model.overlays.lsp_results.sticky_symbols_path = Some(PathBuf::from("two.rs"));
        model.overlays.lsp_results.sticky_symbols = vec![symbol("main", 0, 0)];
        assert!(model.sticky_context(1).is_none());
    }
}

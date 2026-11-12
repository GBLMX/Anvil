//! Clickable path and symbol breadcrumbs.

use super::*;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

pub(crate) fn visible(model: &Model, inner: Rect) -> bool {
    model.path.is_some() && model.buffers.help_back.is_none() && !model.welcome && inner.height > 2
}

pub(crate) fn render(model: &Model, frame: &mut Frame, area: Rect) {
    let text = model.breadcrumb_text(usize::from(area.width));
    let Some((path, symbol)) = text.split_once(" › ") else {
        frame.render_widget(
            Paragraph::new(Line::styled(text, theme::panel().fg(theme::muted()))),
            area,
        );
        return;
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(path.to_owned(), theme::panel().fg(theme::muted())),
            Span::styled(" › ", Style::default().fg(theme::accent())),
            Span::styled(
                symbol.to_owned(),
                theme::panel().fg(theme::text().fg.unwrap_or_default()),
            ),
        ])),
        area,
    );
}

pub(crate) fn target_at(model: &Model, column: u16, area: Rect) -> BreadcrumbTarget {
    let text = model.breadcrumb_text(usize::from(area.width));
    let relative = column.saturating_sub(area.x) as usize;
    let symbol_start = text
        .find(" › ")
        .map(|index| UnicodeWidthStr::width(&text[..index]))
        .map_or(usize::MAX, |width| width + 3);
    if relative >= symbol_start {
        BreadcrumbTarget::Symbol
    } else {
        BreadcrumbTarget::Path
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BreadcrumbTarget {
    Path,
    Symbol,
}

impl Model {
    pub(crate) fn reveal_breadcrumb_path(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if !self.tree.visible {
            self.open_tree();
        } else {
            self.ui.focus.tree = true;
            self.ui.focus.split = false;
        }
        if let Some(index) = self
            .tree
            .entries
            .iter()
            .position(|entry| entry.path == path)
        {
            self.tree.selected = Some(index);
        } else {
            self.tree.pending_select = Some(path);
        }
        self.status = "breadcrumb path".to_owned();
        self.dirty = true;
    }

    pub(crate) fn open_breadcrumb_symbols(&mut self) {
        self.request_document_symbols();
    }
}

use super::*;

impl Model {
    pub(crate) fn move_lines(&mut self, delta: isize) {
        let count = self.take_count();
        for _ in 0..count {
            self.move_line(delta);
        }
    }

    pub(crate) fn move_line(&mut self, delta: isize) {
        let Some(position) = self
            .session
            .editor
            .index()
            .position(self.session.editor.text(), self.session.cursor)
        else {
            return;
        };
        let target = position.line as isize + delta;
        if target < 0 {
            return;
        }
        let Some(offset) = self.session.editor.index().offset_at_display_column(
            self.session.editor.text(),
            target as usize,
            position.display_column,
        ) else {
            return;
        };
        self.set_caret(offset);
        self.file.dirty = true;
    }

    pub(crate) fn move_file_end(&mut self) {
        let count = self.input.count;
        let _ = self.take_count();
        let mut last_line = self.session.editor.index().len_lines().saturating_sub(1);
        let text_len = self.session.editor.text().len_bytes();
        if last_line > 0
            && text_len > 0
            && self.session.editor.text().slice(text_len - 1..text_len) == "\n"
        {
            last_line -= 1;
        }
        let line = if count == 0 {
            last_line
        } else {
            (count - 1).min(last_line)
        };
        let Some(offset) = self.session.editor.index().offset_at_display_column(
            self.session.editor.text(),
            line,
            0,
        ) else {
            return;
        };
        self.set_caret(offset);
        self.file.dirty = true;
    }

    pub(crate) fn move_chars(&mut self, delta: isize) {
        let count = self.take_count();
        for _ in 0..count {
            self.move_char(delta);
        }
    }

    pub(crate) fn move_char(&mut self, delta: isize) {
        let next = if delta < 0 {
            self.session
                .editor
                .previous_grapheme_boundary(self.session.cursor)
        } else {
            self.session
                .editor
                .next_grapheme_boundary(self.session.cursor)
        };
        if let Some(offset) = next {
            self.set_caret(offset);
            self.file.dirty = true;
        }
    }

    pub(crate) fn move_file_start(&mut self) {
        let Some(offset) =
            self.session
                .editor
                .index()
                .offset_at_display_column(self.session.editor.text(), 0, 0)
        else {
            return;
        };
        self.set_caret(offset);
        self.view.scroll = 0;
        self.view.scroll_subrow = 0;
        self.file.dirty = true;
    }

    pub(crate) fn center_cursor(&mut self) {
        let Some(position) = self
            .session
            .editor
            .index()
            .position(self.session.editor.text(), self.session.cursor)
        else {
            return;
        };
        let height = usize::from(self.view.viewport_height.saturating_sub(2)).max(1);
        self.view.scroll = position.line.saturating_sub(height / 2);
        self.view.scroll_subrow = 0;
        self.normalize_scroll();
        self.file.dirty = true;
    }

    pub(crate) fn move_home(&mut self) {
        if let Some(line) = self
            .session
            .editor
            .index()
            .offset_to_line(self.session.cursor)
        {
            self.set_caret(line.start);
            self.file.dirty = true;
        }
    }

    pub(crate) fn move_end(&mut self) {
        if let Some(line) = self
            .session
            .editor
            .index()
            .offset_to_line(self.session.cursor)
        {
            self.set_caret(line.end);
            self.file.dirty = true;
        }
    }

    fn set_caret(&mut self, offset: usize) {
        if let Some(session) = self.edit.insert_session.as_mut() {
            session.valid = false;
        }
        self.session.cursor = offset;
        if self.session.editor.selection_bytes().len() <= 1 {
            self.session.editor.set_selection_bytes(offset, offset);
        }
    }

    pub(crate) fn ensure_cursor_visible(&mut self, height: u16, width: u16) {
        let Some(position) = self
            .session
            .editor
            .index()
            .position(self.session.editor.text(), self.session.cursor)
        else {
            return;
        };
        let line = position.line;
        let previous_scroll = self.view.scroll;
        let height = usize::from(height.max(1));
        let margin = self
            .config
            .settings
            .editor
            .scroll_margin
            .min(height.saturating_sub(1) / 2);
        if self.overlays.lsp_results.code_lens.is_some()
            && self.row_of_source_line(line, height).is_none()
        {
            self.view.scroll = line;
            self.view.scroll_subrow = 0;
        }
        if self.has_closed_folds() {
            if self.is_line_hidden(line) {
                self.open_fold_containing(line);
            }
            let visible = self.visible_line_numbers(self.view.scroll, height);
            if !visible.contains(&line) {
                self.view.scroll = line;
                self.view.scroll_subrow = 0;
                self.normalize_scroll();
            }
        } else if line < self.view.scroll.saturating_add(margin) {
            self.view.scroll = line.saturating_sub(margin);
        } else if line + margin >= self.view.scroll.saturating_add(height) {
            self.view.scroll = line + margin + 1 - height;
        }
        let column = position.display_column;
        if self.config.settings.editor.soft_wrap {
            self.view.wrap_width = self.wrap_text_width(width);
            if self.row_of_position(line, column, height).is_none() {
                self.view.scroll = line;
                self.view.scroll_subrow = 0;
            } else if previous_scroll != self.view.scroll {
                self.view.scroll_subrow = 0;
            }
            let segments = self.wrap_segments(line, self.view.wrap_width);
            let line_end = self
                .session
                .editor
                .index()
                .line_to_offset(line)
                .and_then(|offset| self.session.editor.index().offset_to_line(offset))
                .map_or(0, |range| range.end);
            let mut segment = 0usize;
            for (index, (start, end, _)) in segments.iter().enumerate() {
                let start_column = self
                    .session
                    .editor
                    .index()
                    .position(self.session.editor.text(), *start)
                    .map_or(0, |position| position.display_column);
                let end_column = self
                    .session
                    .editor
                    .index()
                    .position(self.session.editor.text(), *end)
                    .map_or(start_column, |position| position.display_column);
                if column >= start_column && (column < end_column || *end >= line_end) {
                    segment = index;
                }
            }
            if segment < self.view.scroll_subrow {
                self.view.scroll_subrow = segment;
            } else if segment >= self.view.scroll_subrow.saturating_add(height) {
                self.view.scroll_subrow = segment + 1 - height;
            }
            self.view.hscroll = 0;
            return;
        }
        let width = usize::from(width.max(1));
        if column < self.view.hscroll {
            self.view.hscroll = column;
        } else if column >= self.view.hscroll.saturating_add(width) {
            self.view.hscroll = column + 1 - width;
        }
    }

    #[cfg(feature = "e2e-bench")]
    #[doc(hidden)]
    pub fn ensure_cursor_visible_for_bench(&mut self, height: u16, width: u16) {
        self.ensure_cursor_visible(height, width);
    }
}

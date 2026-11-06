//! Pointer state, hit-testing, and mouse gestures.

use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointerState {
    Idle,
    Pressed(DragKind),
    Dragging(DragKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DragKind {
    Editor { anchor: usize },
    Split,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Outside,
    Tabs(usize),
    Tree(usize),
    SplitHandle,
    Editor {
        side: Side,
        line: usize,
        column: usize,
    },
    Status,
}

pub(crate) fn handle_mouse(model: &mut Model, mouse: MouseEvent, width: u16, height: u16) -> bool {
    let area = Rect {
        x: 0,
        y: 0,
        width,
        height,
    };
    if overlay_open(model) {
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && !inside_overlay(model, mouse.column, mouse.row, area)
        {
            if model.ui.diagnostic_peek.is_some() {
                model.close_diagnostic_peek();
            }
            if model.ui.problems.visible {
                model.ui.problems.visible = false;
                model.dirty = true;
            }
        }
        return true;
    }
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let target = hit_test(model, mouse.column, mouse.row, width, height);
            match target {
                Target::Editor { side, line, column } => {
                    focus_editor_side(model, side);
                    let offset = offset_for_position(model, line, column);
                    model.cursor = offset;
                    model.editor.set_selection_bytes(offset, offset);
                    model.pointer = PointerState::Pressed(DragKind::Editor { anchor: offset });
                    model.dirty = true;
                }
                Target::Tree(index) => {
                    model.ui.focus.tree = true;
                    model.ui.focus.split = false;
                    if index < model.tree.len() {
                        model.tree_selected = Some(index);
                    }
                    model.dirty = true;
                }
                Target::Tabs(index) => {
                    if index < model.buffers.len() && index != model.active {
                        model.load_buffer(index);
                    }
                    model.focus_editor();
                }
                Target::SplitHandle => {
                    model.pointer = PointerState::Pressed(DragKind::Split);
                    model.dirty = true;
                }
                Target::Status | Target::Outside => {
                    model.pointer = PointerState::Idle;
                }
            }
            true
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            match model.pointer {
                PointerState::Pressed(kind) | PointerState::Dragging(kind) => {
                    model.pointer = PointerState::Dragging(kind);
                    match kind {
                        DragKind::Editor { anchor } => {
                            let line = editor_line_at(model, mouse.row);
                            let column = editor_column_at(model, mouse.column, width);
                            edge_scroll(model, mouse.row, height);
                            let head = offset_for_position(model, line, column);
                            model.editor.set_selection_bytes(anchor, head);
                            model.cursor = head;
                        }
                        DragKind::Split => {
                            let ratio = split_ratio_at(mouse.column, width);
                            model.split_ratio = ratio;
                        }
                    }
                    model.dirty = true;
                }
                PointerState::Idle => {}
            }
            true
        }
        MouseEventKind::Up(MouseButton::Left) => {
            model.pointer = PointerState::Idle;
            true
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let delta = if mouse.kind == MouseEventKind::ScrollUp {
                -1
            } else {
                1
            };
            if mouse
                .modifiers
                .contains(crossterm::event::KeyModifiers::SHIFT)
            {
                model.hscroll = model.hscroll.saturating_add_signed(delta as isize * 3);
            } else {
                model.scroll = model
                    .scroll
                    .saturating_add_signed(delta as isize)
                    .min(model.editor.index().len_lines().saturating_sub(1));
                model.ensure_cursor_visible(height.saturating_sub(2), width.saturating_sub(2));
            }
            model.dirty = true;
            true
        }
        _ => false,
    }
}

pub(crate) fn hit_test(model: &Model, col: u16, row: u16, width: u16, height: u16) -> Target {
    if col >= width || row >= height {
        return Target::Outside;
    }
    let mut inner = Rect {
        x: 1,
        y: 1,
        width: width.saturating_sub(2),
        height: height.saturating_sub(2),
    };
    if inner.width == 0 || inner.height == 0 {
        return Target::Outside;
    }
    if model.buffers.len() > 1 || model.help_back.is_some() {
        if row == inner.y {
            return Target::Tabs(tab_at(model, col.saturating_sub(inner.x), inner.width));
        }
        inner.y = inner.y.saturating_add(1);
        inner.height = inner.height.saturating_sub(1);
    }
    if model.tree_visible && inner.width > 50 {
        let sidebar = (inner.width / 5).clamp(18, 40).min(inner.width - 20);
        if col < inner.x.saturating_add(sidebar) {
            let visible = inner.height.saturating_sub(2) as usize;
            let start = model
                .tree_selected
                .unwrap_or(0)
                .saturating_sub(visible.saturating_sub(1));
            let index =
                start.saturating_add(row.saturating_sub(inner.y.saturating_add(1)) as usize);
            return Target::Tree(index);
        }
        inner.x = inner.x.saturating_add(sidebar);
        inner.width = inner.width.saturating_sub(sidebar);
    }
    if row == height.saturating_sub(1) {
        return Target::Status;
    }
    let split_x = inner.x + inner.width * model.split_ratio.min(100) / 100;
    if model.split.is_some() && col.abs_diff(split_x) <= 1 {
        return Target::SplitHandle;
    }
    let (pane_x, pane_width, side) = if model.split.is_some() {
        if col < split_x {
            (inner.x, split_x.saturating_sub(inner.x), Side::Left)
        } else {
            (
                split_x,
                inner.x.saturating_add(inner.width).saturating_sub(split_x),
                Side::Right,
            )
        }
    } else {
        (inner.x, inner.width, Side::Left)
    };
    let gutter = model.gutter_width().min(pane_width);
    let line = model
        .scroll
        .saturating_add(row.saturating_sub(inner.y) as usize);
    let column = col
        .saturating_sub(pane_x.saturating_add(gutter))
        .saturating_add(model.hscroll as u16) as usize;
    Target::Editor { side, line, column }
}

fn overlay_open(model: &Model) -> bool {
    model.ui.diagnostic_peek.is_some()
        || model.ui.problems.visible
        || model.command_palette.is_some()
        || model.fuzzy.is_some()
        || model.code_actions.is_some()
        || model.document_symbols.is_some()
        || model.completion.is_some()
}

fn inside_overlay(model: &Model, col: u16, row: u16, area: Rect) -> bool {
    if let Some(peek) = &model.ui.diagnostic_peek {
        let cursor_line = model
            .editor
            .index()
            .position(model.editor.text(), model.cursor)
            .map(|position| position.line as u16)
            .unwrap_or(0);
        let cursor_y = area
            .y
            .saturating_add(cursor_line.saturating_sub(model.scroll as u16))
            .min(area.y + area.height.saturating_sub(1));
        let width = (area.width * 2 / 3)
            .max(30)
            .min(area.width.saturating_sub(2));
        let height = (peek.line_count as u16 + 2)
            .min(12)
            .min(area.height.saturating_sub(2));
        if let Some(popup) = crate::render::diagnostic_peek_rect(area, cursor_y, width, height) {
            return col >= popup.x
                && col < popup.x + popup.width
                && row >= popup.y
                && row < popup.y + popup.height;
        }
    }
    if model.ui.problems.visible {
        let width = (area.width * 3 / 4)
            .max(30)
            .min(area.width.saturating_sub(2));
        let height = (model.ui.problems.items.len() as u16 + 2)
            .min(18)
            .min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        return col >= popup.x
            && col < popup.x + popup.width
            && row >= popup.y
            && row < popup.y + popup.height;
    }
    model.command_palette.is_some()
        || model.fuzzy.is_some()
        || model.code_actions.is_some()
        || model.document_symbols.is_some()
        || model.completion.is_some()
}

fn focus_editor_side(model: &mut Model, side: Side) {
    model.ui.focus.tree = false;
    if model.split.is_none() {
        model.ui.focus.split = false;
    } else {
        let split_side = Side::Right;
        let split_is_focused = model.ui.focus.split;
        let clicked_split = (side == split_side) != split_is_focused;
        if clicked_split {
            model.focus_split(true);
        } else {
            model.focus_editor();
        }
    }
}

fn editor_line_at(model: &Model, row: u16) -> usize {
    let top = if model.buffers.len() > 1 || model.help_back.is_some() {
        2
    } else {
        1
    };
    model
        .scroll
        .saturating_add(row.saturating_sub(top) as usize)
        .min(model.editor.index().len_lines().saturating_sub(1))
}

fn editor_column_at(model: &Model, col: u16, _width: u16) -> usize {
    let gutter = model.gutter_width();
    col.saturating_sub(1u16.saturating_add(gutter))
        .saturating_add(model.hscroll as u16) as usize
}

fn edge_scroll(model: &mut Model, row: u16, height: u16) {
    if row <= 1 {
        model.scroll = model.scroll.saturating_sub(1);
    } else if row + 2 >= height {
        model.scroll = model
            .scroll
            .saturating_add(1)
            .min(model.editor.index().len_lines().saturating_sub(1));
    }
}

fn offset_for_position(model: &Model, line: usize, column: usize) -> usize {
    let Some(line_range) = model
        .editor
        .index()
        .line_to_offset(line)
        .and_then(|offset| model.editor.line_range_at(offset))
    else {
        return model.cursor.min(model.editor.text().len_bytes());
    };
    let text = model.editor.text().slice(line_range.clone());
    let mut display = 0;
    for (offset, character) in text.char_indices() {
        let width = if character == '\t' {
            4 - display % 4
        } else {
            character.width().unwrap_or(0)
        };
        if display + width > column {
            return line_range.start
                + offset
                + if column < display + width.div_ceil(2) {
                    0
                } else {
                    character.len_utf8()
                };
        }
        display += width;
    }
    line_range.start + text.trim_end_matches('\n').len()
}

fn split_ratio_at(col: u16, width: u16) -> u16 {
    let usable = width.saturating_sub(2);
    if usable == 0 {
        return 50;
    }
    let relative = col.saturating_sub(1).min(usable);
    ((u32::from(relative) * 100) / u32::from(usable)).clamp(20, 80) as u16
}

fn tab_at(model: &Model, offset: u16, _width: u16) -> usize {
    let mut used = 0u16;
    for (index, buffer) in model.buffers.iter().enumerate() {
        let name = buffer
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "[no file]".to_owned());
        let label = format!(" {name} ");
        let label_width = label.chars().count() as u16;
        if offset < used + label_width {
            return index;
        }
        used = used.saturating_add(label_width + 1);
    }
    model.active
}

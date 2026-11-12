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
    Location(usize),
    Tree(usize),
    CodeLens(usize),
    SplitHandle,
    Editor {
        side: Side,
        line: usize,
        column: usize,
    },
    Status,
    Breadcrumb(crate::breadcrumbs::BreadcrumbTarget),
    WelcomeRecent(usize),
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
            close_outside_overlay(model);
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
                Target::Location(index) => {
                    model.select_stdin_location(index);
                }
                Target::CodeLens(index) => {
                    model.execute_code_lens(index);
                }
                Target::Tabs(index) => {
                    if index < model.buffers.len() && index != model.active {
                        model.switch_to_buffer(index);
                    }
                    model.focus_editor();
                }
                Target::SplitHandle => {
                    model.pointer = PointerState::Pressed(DragKind::Split);
                    model.dirty = true;
                }
                Target::Breadcrumb(crate::breadcrumbs::BreadcrumbTarget::Path) => {
                    model.reveal_breadcrumb_path();
                }
                Target::Breadcrumb(crate::breadcrumbs::BreadcrumbTarget::Symbol) => {
                    model.open_breadcrumb_symbols();
                }
                Target::WelcomeRecent(index) => {
                    if let Some(path) = model.openable_recent_files().get(index).cloned() {
                        model.welcome = false;
                        if let Err(error) = model.open_path(&path) {
                            model.status = format!("E: {error}");
                        }
                        model.dirty = true;
                    }
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
                            edge_scroll(model, mouse.row, height);
                            if let Some((_, line, column)) =
                                editor_position_at(model, mouse.column, mouse.row, width, height)
                            {
                                let head = offset_for_position(model, line, column);
                                model.editor.set_selection_bytes(anchor, head);
                                model.cursor = head;
                            }
                        }
                        DragKind::Split => {
                            if let Some(inner) = editor_area(model, width, height) {
                                model.split_ratio = split_ratio_at(inner, mouse.column);
                            }
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
            let (side, pane_width) = editor_area(model, width, height)
                .map(|inner| pane_at(model, inner, mouse.column))
                .map(|(side, _, pane_width)| (side, pane_width))
                .unwrap_or((Side::Left, width.saturating_sub(2)));
            let active_side = if model.ui.focus.split {
                Side::Right
            } else {
                Side::Left
            };
            let split_target = model.split.is_some() && side != active_side;
            if mouse
                .modifiers
                .contains(crossterm::event::KeyModifiers::SHIFT)
            {
                if split_target {
                    if let Some(pane) = model.split.as_mut() {
                        pane.buffer.hscroll = pane
                            .buffer
                            .hscroll
                            .saturating_add_signed(delta as isize * 3);
                    }
                } else {
                    model.hscroll = model.hscroll.saturating_add_signed(delta as isize * 3);
                }
            } else if split_target {
                model.scroll_split_rows(delta as isize);
            } else if model.settings.editor.soft_wrap {
                let text_width = pane_width.saturating_sub(model.gutter_width());
                model.scroll_display_rows_in_pane(delta as isize, text_width);
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
    if model.welcome {
        return crate::render::welcome_recent_at(model, col, row, width, height)
            .map_or(Target::Outside, Target::WelcomeRecent);
    }
    let Some((inner, tab_row, breadcrumb_row)) = content_rect(model, width, height) else {
        return Target::Outside;
    };
    if let Some(area) = breadcrumb_row
        && row == area.y
    {
        return Target::Breadcrumb(crate::breadcrumbs::target_at(model, col, area));
    }
    if tab_row == Some(row) {
        return Target::Tabs(tab_at(model, col.saturating_sub(inner.x), inner.width));
    }
    let mut editor_inner = inner;
    if model.tree_visible && editor_inner.width > 50 {
        let sidebar = sidebar_width(editor_inner);
        if col < editor_inner.x.saturating_add(sidebar) {
            let visible = editor_inner.height.saturating_sub(2) as usize;
            let start = model
                .tree_selected
                .unwrap_or(0)
                .saturating_sub(visible.saturating_sub(1));
            let index =
                start.saturating_add(row.saturating_sub(editor_inner.y.saturating_add(1)) as usize);
            return Target::Tree(index);
        }
        editor_inner.x = editor_inner.x.saturating_add(sidebar);
        editor_inner.width = editor_inner.width.saturating_sub(sidebar);
    }
    if let Some(panel) = crate::pipe_locations::panel_rect(model, editor_inner)
        && row > panel.y
        && row + 1 < panel.bottom()
        && col >= panel.x
        && col < panel.right()
    {
        let index = model.stdin_location_scroll
            + usize::from(row.saturating_sub(panel.y).saturating_sub(1));
        if index < model.stdin_locations.len() {
            return Target::Location(index);
        }
    }
    if row >= editor_inner.y
        && let Some(index) = model.code_lens_at_row(
            usize::from(row - editor_inner.y),
            usize::from(editor_inner.height),
        )
    {
        return Target::CodeLens(index);
    }
    if row == height.saturating_sub(1) {
        return Target::Status;
    }
    let split_x = split_x(editor_inner, model.split_ratio);
    if model.split.is_some() && col.abs_diff(split_x) <= 1 {
        return Target::SplitHandle;
    }
    let (side, line, column) = editor_position_at(model, col, row, width, height).unwrap_or((
        Side::Left,
        model.scroll,
        model.hscroll,
    ));
    Target::Editor { side, line, column }
}

fn content_rect(
    model: &Model,
    width: u16,
    height: u16,
) -> Option<(Rect, Option<u16>, Option<Rect>)> {
    if model.welcome {
        return None;
    }
    let mut inner = Rect {
        x: 1,
        y: 1,
        width: width.saturating_sub(2),
        height: height.saturating_sub(2),
    };
    if inner.width == 0 || inner.height == 0 {
        return None;
    }
    let mut tab_row = None;
    if model.buffers.len() > 1 || model.help_back.is_some() {
        tab_row = Some(inner.y);
        inner.y = inner.y.saturating_add(1);
        inner.height = inner.height.saturating_sub(1);
    }
    let breadcrumb_row = if crate::breadcrumbs::visible(model, inner) {
        let area = Rect { height: 1, ..inner };
        inner.y = inner.y.saturating_add(1);
        inner.height = inner.height.saturating_sub(1);
        Some(area)
    } else {
        None
    };
    Some((inner, tab_row, breadcrumb_row))
}

fn sidebar_width(inner: Rect) -> u16 {
    (inner.width / 5).clamp(18, 40).min(inner.width - 20)
}

fn editor_area(model: &Model, width: u16, height: u16) -> Option<Rect> {
    let (mut inner, _, _) = content_rect(model, width, height)?;
    if model.tree_visible && inner.width > 50 {
        let sidebar = sidebar_width(inner);
        inner.x = inner.x.saturating_add(sidebar);
        inner.width = inner.width.saturating_sub(sidebar);
    }
    Some(inner)
}

fn split_x(inner: Rect, ratio: u16) -> u16 {
    inner
        .x
        .saturating_add((u32::from(inner.width) * u32::from(ratio.min(100)) / 100) as u16)
}

fn pane_at(model: &Model, inner: Rect, col: u16) -> (Side, u16, u16) {
    let split_x = split_x(inner, model.split_ratio);
    if model.split.is_some() {
        if col < split_x {
            (Side::Left, inner.x, split_x.saturating_sub(inner.x))
        } else {
            (
                Side::Right,
                split_x,
                inner.x.saturating_add(inner.width).saturating_sub(split_x),
            )
        }
    } else {
        (Side::Left, inner.x, inner.width)
    }
}

fn editor_position_at(
    model: &Model,
    col: u16,
    row: u16,
    width: u16,
    height: u16,
) -> Option<(Side, usize, usize)> {
    let inner = editor_area(model, width, height)?;
    let (side, pane_x, pane_width) = pane_at(model, inner, col);
    let gutter = model.gutter_width().min(pane_width);
    let (line, base_column) = model.position_at_visible_row(row.saturating_sub(inner.y) as usize)?;
    let column = col
        .saturating_sub(pane_x.saturating_add(gutter))
        .saturating_add(base_column as u16) as usize;
    Some((side, line, column))
}

fn overlay_open(model: &Model) -> bool {
    model.ui.diagnostic_peek.is_some()
        || model.ui.problems.visible
        || model.command_palette.is_some()
        || model.fuzzy.is_some()
        || model.code_actions.is_some()
        || model.document_symbols.is_some()
        || model.references.is_some()
        || model.call_hierarchy.is_some()
        || model.settings_overlay.is_some()
        || model.completion.is_some()
}

fn close_outside_overlay(model: &mut Model) {
    if model.ui.diagnostic_peek.is_some() {
        model.close_diagnostic_peek();
    }
    if model.ui.problems.visible {
        model.ui.problems.visible = false;
        model.dirty = true;
    }
    if model.command_palette.is_some() {
        model.close_command_palette();
    }
    if model.fuzzy.is_some() {
        model.fuzzy = None;
        model.clear_preview();
        model.dirty = true;
    }
    if model.code_actions.is_some() {
        model.cancel_code_actions();
    }
    if model.document_symbols.is_some() {
        model.cancel_document_symbols();
    }
    if model.references.is_some() {
        model.cancel_references();
    }
    if model.call_hierarchy.is_some() {
        model.cancel_call_hierarchy();
    }
    if model.settings_overlay.is_some() {
        model.cancel_settings();
    }
    if model.completion.is_some() {
        model.cancel_completion();
    }
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
        return crate::render::rect_contains(popup, col, row);
    }
    if model.command_palette.is_some() {
        return crate::render::rect_contains(crate::render::command_palette_rect(area), col, row);
    }
    if model.fuzzy.is_some() {
        return crate::render::rect_contains(crate::render::fuzzy_rect(area), col, row);
    }
    if let Some(actions) = &model.code_actions {
        let popup = crate::render::code_actions_rect(area, actions.items.len());
        return crate::render::rect_contains(popup, col, row);
    }
    if let Some(symbols) = &model.document_symbols {
        let popup = crate::render::document_symbols_rect(area, symbols.items.len());
        return crate::render::rect_contains(popup, col, row);
    }
    if let Some(references) = &model.references {
        let popup = crate::render::document_symbols_rect(area, references.items.len());
        return crate::render::rect_contains(popup, col, row);
    }
    if let Some(calls) = &model.call_hierarchy {
        let popup = crate::render::document_symbols_rect(area, calls.items.len());
        return crate::render::rect_contains(popup, col, row);
    }
    if model.settings_overlay.is_some() {
        let popup =
            crate::render::document_symbols_rect(area, crate::settings_ui::SETTINGS_ROWS + 1);
        return crate::render::rect_contains(popup, col, row);
    }
    if let Some(completion) = &model.completion {
        let popup = crate::render::completion_rect(area, completion.filtered.len());
        return crate::render::rect_contains(popup, col, row);
    }
    false
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

fn split_ratio_at(inner: Rect, col: u16) -> u16 {
    if inner.width == 0 {
        return 50;
    }
    let relative = col.saturating_sub(inner.x).min(inner.width);
    ((u32::from(relative) * 100 + u32::from(inner.width) / 2) / u32::from(inner.width))
        .clamp(20, 80) as u16
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

//! Pointer state, hit-testing, and mouse gestures.

use super::*;
use crate::layout::{tree_fits, tree_viewport_start};
use crate::surface::{DockId, SurfaceId};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

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
    WelcomeAction(usize),
    WelcomeContinue,
    WelcomeFile(usize),
    WelcomeWorkspace(usize),
}

pub(crate) fn handle_mouse(model: &mut Model, mouse: MouseEvent, width: u16, height: u16) -> bool {
    let area = Rect {
        x: 0,
        y: 0,
        width,
        height,
    };
    if overlay_open(model) {
        if model.overlays.theme_browser.is_some() {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    model.move_theme_browser(-1);
                    return true;
                }
                MouseEventKind::ScrollDown => {
                    model.move_theme_browser(1);
                    return true;
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(browser) = model.overlays.theme_browser.as_ref()
                        && let Some(index) =
                            crate::theme_browser::hit(browser, area, mouse.column, mouse.row)
                    {
                        let browser = model
                            .overlays
                            .theme_browser
                            .as_mut()
                            .expect("theme browser");
                        browser.selected = index;
                        if let Some(name) = browser.selected_name() {
                            let _ = crate::theme::apply_choice(
                                crate::theme::Choice::Fixed(name),
                                false,
                            );
                        }
                        model.file.dirty = true;
                        return true;
                    }
                }
                _ => {}
            }
        }
        if model.overlays.buffer_picker.is_some() {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    model.move_buffer_picker(-1);
                    return true;
                }
                MouseEventKind::ScrollDown => {
                    model.move_buffer_picker(1);
                    return true;
                }
                _ => {}
            }
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && model.overlays.command_palette.is_some()
            && let Some(row) =
                crate::render::command_palette_row_at(model, area, mouse.column, mouse.row)
        {
            model
                .overlays
                .command_palette
                .as_mut()
                .expect("command palette")
                .select(row);
            model.file.dirty = true;
            return true;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(row) =
                crate::render::code_action_row_at(model, area, mouse.column, mouse.row)
            {
                if let Some(actions) = model.overlays.lsp_results.code_actions.as_mut() {
                    actions.selected = row;
                    model.file.dirty = true;
                    return true;
                }
            }
            if let Some(row) =
                crate::render::document_symbol_row_at(model, area, mouse.column, mouse.row)
            {
                if let Some(symbols) = model.overlays.lsp_results.document_symbols.as_mut() {
                    symbols.selected = row;
                    model.file.dirty = true;
                    return true;
                }
            }
            if let Some(row) = crate::render::reference_row_at(model, area, mouse.column, mouse.row)
            {
                if let Some(references) = model.overlays.lsp_results.references.as_mut() {
                    references.selected = row;
                    model.file.dirty = true;
                    return true;
                }
            }
            if let Some(row) =
                crate::render::call_hierarchy_row_at(model, area, mouse.column, mouse.row)
            {
                if let Some(calls) = model.overlays.lsp_results.call_hierarchy.as_mut() {
                    calls.selected = row;
                    model.file.dirty = true;
                    return true;
                }
            }
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && model.overlays.buffer_picker.is_some()
            && let Some(row) =
                crate::render::buffer_picker_row_at(model, area, mouse.column, mouse.row)
        {
            model
                .overlays
                .buffer_picker
                .as_mut()
                .expect("buffer picker")
                .selected = row;
            model.file.dirty = true;
            return true;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && model.overlays.fuzzy.is_some()
            && let Some(row) = crate::render::fuzzy_row_at(model, area, mouse.column, mouse.row)
        {
            model.overlays.fuzzy.as_mut().expect("fuzzy").selected = row;
            model.request_preview();
            model.file.dirty = true;
            return true;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(overlay) = model.overlays.settings_overlay.as_ref()
                && let Some(row) = crate::settings_ui::hit(overlay, area, mouse.column, mouse.row)
            {
                model.select_settings_row(row);
                return true;
            }
            if !inside_overlay(model, mouse.column, mouse.row, area) {
                close_outside_overlay(model);
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
                    model.session.cursor = offset;
                    let _ = model.session.editor.try_set_selection_bytes(offset, offset);
                    model.view.pointer = PointerState::Pressed(DragKind::Editor { anchor: offset });
                    model.file.dirty = true;
                }
                Target::Tree(index) => {
                    model.ui.focus.tree = true;
                    model.ui.focus.split = false;
                    if index < model.workspace.tree.entries.len() {
                        model.workspace.tree.selected = Some(index);
                    }
                    model.file.dirty = true;
                }
                Target::Location(index) => {
                    model.select_stdin_location(index);
                }
                Target::CodeLens(index) => {
                    model.execute_code_lens(index);
                }
                Target::Tabs(index) => {
                    if index < model.buffers.entries.len() && index != model.buffers.active {
                        model.switch_to_buffer(index);
                    }
                    model.focus_editor();
                }
                Target::SplitHandle => {
                    model.view.pointer = PointerState::Pressed(DragKind::Split);
                    model.file.dirty = true;
                }
                Target::Breadcrumb(crate::breadcrumbs::BreadcrumbTarget::Path) => {
                    model.reveal_breadcrumb_path();
                }
                Target::Breadcrumb(crate::breadcrumbs::BreadcrumbTarget::Symbol) => {
                    model.open_breadcrumb_symbols();
                }
                Target::WelcomeAction(index) => {
                    model.start_focus_actions();
                    model.start_page.action_selected = index;
                    model.activate_start_action(index);
                }
                Target::WelcomeContinue => {
                    model.start_focus_files();
                    model.start_page.file_selected = 0;
                    model.activate_start_file(0);
                }
                Target::WelcomeFile(index) => {
                    model.start_focus_files();
                    model.start_page.file_selected =
                        index + usize::from(model.start_page.session_available);
                    model.activate_start_file(
                        index + usize::from(model.start_page.session_available),
                    );
                }
                Target::WelcomeWorkspace(index) => {
                    model.start_focus_workspaces();
                    model.start_page.workspace_selected = index;
                    model.activate_start_workspace(index);
                }
                Target::Status | Target::Outside => {
                    model.view.pointer = PointerState::Idle;
                }
            }
            true
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            match model.view.pointer {
                PointerState::Pressed(kind) | PointerState::Dragging(kind) => {
                    model.view.pointer = PointerState::Dragging(kind);
                    match kind {
                        DragKind::Editor { anchor } => {
                            edge_scroll(model, mouse.row, height);
                            if let Some((_, line, column)) =
                                editor_position_at(model, mouse.column, mouse.row, width, height)
                            {
                                let head = offset_for_position(model, line, column);
                                let _ = model.session.editor.try_set_selection_bytes(anchor, head);
                                model.session.cursor = head;
                            }
                        }
                        DragKind::Split => {
                            if let Some(inner) = editor_area(model, width, height) {
                                model.view.split_ratio = if model.view.split_orientation
                                    == crate::state::SplitOrientation::Vertical
                                {
                                    split_ratio_at(inner.x, inner.width, mouse.column)
                                } else {
                                    split_ratio_at(inner.y, inner.height, mouse.row)
                                };
                                model.refresh_wrap_width();
                            }
                        }
                    }
                    model.file.dirty = true;
                }
                PointerState::Idle => {}
            }
            true
        }
        MouseEventKind::Up(MouseButton::Left) => {
            model.view.pointer = PointerState::Idle;
            true
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let delta = if mouse.kind == MouseEventKind::ScrollUp {
                -1
            } else {
                1
            };
            if let Some(inner) = editor_area(model, width, height)
                && model.workspace.tree.visible
                && tree_fits(inner.width)
                && mouse.column < inner.x.saturating_add(model.sidebar_width(inner.width))
                && mouse.row > inner.y
            {
                model.tree_move(delta * 3);
                model.file.dirty = true;
                return true;
            }
            let (side, pane_width) = editor_area(model, width, height)
                .map(|inner| pane_at(model, inner, mouse.column, mouse.row))
                .map(|(side, _, pane_width, _)| (side, pane_width))
                .unwrap_or((Side::Left, width.saturating_sub(2)));
            let active_side = if model.ui.focus.split {
                Side::Right
            } else {
                Side::Left
            };
            let split_target = model.buffers.split.is_some() && side != active_side;
            if mouse
                .modifiers
                .contains(crossterm::event::KeyModifiers::SHIFT)
            {
                if split_target {
                    if let Some(pane) = model.buffers.split.as_mut() {
                        pane.buffer.hscroll = pane.buffer.hscroll.saturating_add_signed(delta * 3);
                    }
                } else {
                    model.view.hscroll = model.view.hscroll.saturating_add_signed(delta * 3);
                }
            } else if split_target {
                model.scroll_split_rows(delta);
            } else if model.buffer_settings().soft_wrap {
                let text_width = pane_width.saturating_sub(model.gutter_width());
                model.scroll_display_rows_in_pane(delta, text_width);
            } else {
                model.view.scroll = model
                    .view
                    .scroll
                    .saturating_add_signed(delta)
                    .min(model.session.editor.index().len_lines().saturating_sub(1));
                model.ensure_cursor_visible(height.saturating_sub(2), width.saturating_sub(2));
            }
            model.file.dirty = true;
            true
        }
        _ => false,
    }
}

pub(crate) fn hit_test(model: &Model, col: u16, row: u16, width: u16, height: u16) -> Target {
    if col >= width || row >= height {
        return Target::Outside;
    }
    if model.start_page.welcome {
        let area = model.welcome_area(width, height);
        return match crate::render::welcome_hit_at(model, col, row, area) {
            Some(crate::render::WelcomeHit::Action(index)) => Target::WelcomeAction(index),
            Some(crate::render::WelcomeHit::Continue) => Target::WelcomeContinue,
            Some(crate::render::WelcomeHit::File(index)) => Target::WelcomeFile(index),
            Some(crate::render::WelcomeHit::Workspace(index)) => Target::WelcomeWorkspace(index),
            None => Target::Outside,
        };
    }
    let Some(rects) = crate::layout::workbench_rects(model, width, height) else {
        return Target::Outside;
    };
    if rects
        .message
        .is_some_and(|area| row >= area.y && row < area.bottom())
    {
        return Target::Outside;
    }
    if let Some(area) = rects.breadcrumbs
        && row == area.y
    {
        return Target::Breadcrumb(crate::breadcrumbs::target_at(model, col, area));
    }
    if let Some(area) = rects.tab_bar
        && row == area.y
    {
        return crate::render::buffer_tab_at(model, area.width, col.saturating_sub(area.x))
            .map_or(Target::Outside, Target::Tabs);
    }
    if let Some(area) = rects.rect_for(SurfaceId::Dock(DockId::Tree))
        && col < area.right()
    {
        if row == area.y {
            return Target::Outside;
        }
        let visible = area.height.saturating_sub(1) as usize;
        let start = tree_viewport_start(model.workspace.tree.selected, visible);
        let index = start.saturating_add(row.saturating_sub(area.y.saturating_add(1)) as usize);
        return Target::Tree(index);
    }
    if let Some(panel) = rects.pipe_locations
        && row > panel.y
        && row + 1 < panel.bottom()
        && col >= panel.x
        && col < panel.right()
    {
        let index = model.overlays.location.stdin_location_scroll
            + usize::from(row.saturating_sub(panel.y).saturating_sub(1));
        if index < model.overlays.location.stdin_locations.len() {
            return Target::Location(index);
        }
    }
    if row >= rects.editor_content.y
        && let Some(index) = model.code_lens_at_row(
            usize::from(row - rects.editor_content.y),
            usize::from(rects.editor_content.height),
        )
    {
        return Target::CodeLens(index);
    }
    if row == rects.status.y {
        return Target::Status;
    }
    if let Some(split) = rects.split
        && col >= rects.editor_content.x
        && col < rects.editor_content.right()
        && row >= rects.editor_content.y
        && row < rects.editor_content.bottom()
        && ((split.vertical && col.abs_diff(split.handle_x) <= 1)
            || (!split.vertical && row.abs_diff(split.handle_y) <= 1))
    {
        return Target::SplitHandle;
    }
    let (side, line, column) = editor_position_at(model, col, row, width, height).unwrap_or((
        Side::Left,
        model.view.scroll,
        model.view.hscroll,
    ));
    Target::Editor { side, line, column }
}

fn editor_area(model: &Model, width: u16, height: u16) -> Option<Rect> {
    crate::layout::workbench_rects(model, width, height).map(|rects| rects.editor_content)
}

fn split_x(inner: Rect, ratio: u16) -> u16 {
    inner
        .x
        .saturating_add((u32::from(inner.width) * u32::from(ratio.min(100)) / 100) as u16)
}

fn pane_at(model: &Model, inner: Rect, col: u16, row: u16) -> (Side, u16, u16, u16) {
    if model.buffers.split.is_none() {
        return (Side::Left, inner.x, inner.width, inner.y);
    }
    if model.view.split_orientation == crate::state::SplitOrientation::Horizontal {
        let split_y = split_y(inner, model.view.split_ratio);
        if row < split_y {
            (Side::Left, inner.x, inner.width, inner.y)
        } else {
            (Side::Right, inner.x, inner.width, split_y)
        }
    } else {
        let split_x = split_x(inner, model.view.split_ratio);
        if col < split_x {
            (
                Side::Left,
                inner.x,
                split_x.saturating_sub(inner.x),
                inner.y,
            )
        } else {
            (
                Side::Right,
                split_x,
                inner.x.saturating_add(inner.width).saturating_sub(split_x),
                inner.y,
            )
        }
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
    let (side, pane_x, pane_width, pane_y) = pane_at(model, inner, col, row);
    let gutter = model.gutter_width().min(pane_width);
    let (line, base_column) = model.position_at_visible_row(row.saturating_sub(pane_y) as usize)?;
    let column = col
        .saturating_sub(pane_x.saturating_add(gutter))
        .saturating_add(base_column as u16) as usize;
    Some((side, line, column))
}

fn overlay_open(model: &Model) -> bool {
    model.ui.diagnostic_peek.is_some()
        || model.ui.problems.visible
        || model.overlays.command_palette.is_some()
        || model.overlays.buffer_picker.is_some()
        || model.overlays.fuzzy.is_some()
        || model.overlays.lsp_results.code_actions.is_some()
        || model.overlays.lsp_results.document_symbols.is_some()
        || model.overlays.lsp_results.references.is_some()
        || model.overlays.lsp_results.call_hierarchy.is_some()
        || model.overlays.settings_overlay.is_some()
        || model.overlays.theme_browser.is_some()
        || model.language.completion.completion.is_some()
}

fn close_outside_overlay(model: &mut Model) {
    if model.ui.diagnostic_peek.is_some() {
        model.close_diagnostic_peek();
    }
    if model.ui.problems.visible {
        model.close_problems();
    }
    if model.overlays.command_palette.is_some() {
        model.close_command_palette();
    }
    if model.overlays.buffer_picker.is_some() {
        model.close_buffer_picker();
    }
    if model.overlays.fuzzy.is_some() {
        model.close_fuzzy();
    }
    if model.overlays.lsp_results.code_actions.is_some() {
        model.cancel_code_actions();
    }
    if model.overlays.lsp_results.document_symbols.is_some() {
        model.cancel_document_symbols();
    }
    if model.overlays.lsp_results.references.is_some() {
        model.cancel_references();
    }
    if model.overlays.lsp_results.call_hierarchy.is_some() {
        model.cancel_call_hierarchy();
    }
    if model.overlays.settings_overlay.is_some() {
        model.cancel_settings();
    }
    if model.overlays.theme_browser.is_some() {
        model.cancel_theme_browser();
    }
    if model.language.completion.completion.is_some() {
        model.cancel_completion();
    }
}

fn inside_overlay(model: &Model, col: u16, row: u16, area: Rect) -> bool {
    if let Some(peek) = &model.ui.diagnostic_peek {
        let cursor_line = model
            .session
            .editor
            .index()
            .position(model.session.editor.text(), model.session.cursor)
            .map(|position| position.line as u16)
            .unwrap_or(0);
        let cursor_y = area
            .y
            .saturating_add(cursor_line.saturating_sub(model.view.scroll as u16))
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
        return crate::render::rect_contains(
            crate::render::problems_popup_rect(model, area),
            col,
            row,
        );
    }
    if model.overlays.command_palette.is_some() {
        return crate::render::rect_contains(crate::render::command_palette_rect(area), col, row);
    }
    if model.overlays.buffer_picker.is_some() {
        return crate::render::rect_contains(crate::render::fuzzy_rect(area), col, row);
    }
    if model.overlays.fuzzy.is_some() {
        return crate::render::rect_contains(crate::render::fuzzy_rect(area), col, row);
    }
    if let Some(actions) = &model.overlays.lsp_results.code_actions {
        let popup = crate::render::code_actions_rect(area, actions.items.len());
        return crate::render::rect_contains(popup, col, row);
    }
    if let Some(symbols) = &model.overlays.lsp_results.document_symbols {
        let popup = crate::render::document_symbols_rect(area, symbols.items.len());
        return crate::render::rect_contains(popup, col, row);
    }
    if let Some(references) = &model.overlays.lsp_results.references {
        let popup = crate::render::document_symbols_rect(area, references.items.len());
        return crate::render::rect_contains(popup, col, row);
    }
    if let Some(calls) = &model.overlays.lsp_results.call_hierarchy {
        let popup = crate::render::document_symbols_rect(area, calls.items.len());
        return crate::render::rect_contains(popup, col, row);
    }
    if model.overlays.settings_overlay.is_some() {
        return crate::render::rect_contains(crate::settings_ui::rect(area), col, row);
    }
    if model.overlays.theme_browser.is_some() {
        return crate::render::rect_contains(crate::theme_browser::rect(area), col, row);
    }
    if let Some(completion) = &model.language.completion.completion {
        let popup = crate::render::completion_rect(area, completion.filtered.len());
        return crate::render::rect_contains(popup, col, row);
    }
    false
}

fn focus_editor_side(model: &mut Model, side: Side) {
    model.ui.focus.tree = false;
    if model.buffers.split.is_none() {
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
        model.view.scroll = model.view.scroll.saturating_sub(1);
    } else if row + 2 >= height {
        model.view.scroll = model
            .view
            .scroll
            .saturating_add(1)
            .min(model.session.editor.index().len_lines().saturating_sub(1));
    }
}

fn offset_for_position(model: &Model, line: usize, column: usize) -> usize {
    let editor = &model.session.editor;
    editor
        .index()
        .offset_at_display_column_rounded(editor.text(), line, column)
        .unwrap_or_else(|| model.session.cursor.min(editor.text().len_bytes()))
}

fn split_ratio_at(start: u16, size: u16, position: u16) -> u16 {
    if size == 0 {
        return 50;
    }
    let relative = position.saturating_sub(start).min(size);
    ((u32::from(relative) * 100 + u32::from(size) / 2) / u32::from(size)).clamp(20, 80) as u16
}

fn split_y(inner: Rect, ratio: u16) -> u16 {
    inner
        .y
        .saturating_add((u32::from(inner.height) * u32::from(ratio.min(100)) / 100) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_editor::EditorState;

    #[test]
    fn pointer_snaps_to_grapheme_boundaries() {
        let model = Model::new(EditorState::from_text("a👨‍👩‍👧‍👦b"), None);
        let emoji_end = model
            .session
            .editor
            .next_grapheme_boundary(1)
            .expect("emoji boundary");
        assert_eq!(offset_for_position(&model, 0, 2), 1);
        assert_eq!(offset_for_position(&model, 0, 5), emoji_end);
        assert_eq!(offset_for_position(&model, 0, 9), emoji_end);
        for column in 0..=12 {
            let offset = offset_for_position(&model, 0, column);
            assert!(model.session.editor.text().is_char_boundary(offset));
        }
    }
}

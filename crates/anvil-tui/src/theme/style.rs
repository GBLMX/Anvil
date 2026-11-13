//! Theme color and style accessors.

use super::*;

pub(crate) fn bg() -> Color {
    active().bg
}
pub(crate) fn surface() -> Color {
    active().surface
}
pub(crate) fn surface_alt() -> Color {
    active().surface_alt
}
pub(crate) fn code_block_bg() -> Color {
    active().code_block_bg
}
pub(crate) fn border_color() -> Color {
    active().border
}
pub(crate) fn text_color() -> Color {
    active().text
}
pub(crate) fn muted() -> Color {
    active().muted
}
pub(crate) fn dim() -> Color {
    active().dim
}
pub(crate) fn accent() -> Color {
    active().accent
}
pub(crate) fn selection() -> Color {
    active().selection
}
pub(crate) fn success() -> Color {
    active().success
}
pub(crate) fn warning() -> Color {
    active().warning
}
pub(crate) fn error() -> Color {
    active().error
}
pub(crate) fn magenta() -> Color {
    active().magenta
}
pub(crate) fn white() -> Color {
    active().white
}
pub(crate) fn syntax_keyword() -> Color {
    active().syntax_keyword
}
pub(crate) fn syntax_string() -> Color {
    active().syntax_string
}
pub(crate) fn syntax_comment() -> Color {
    active().syntax_comment
}
pub(crate) fn syntax_number() -> Color {
    active().syntax_number
}
pub(crate) fn syntax_function() -> Color {
    active().syntax_function
}
pub(crate) fn syntax_type() -> Color {
    active().syntax_type
}
pub(crate) fn syntax_property() -> Color {
    active().syntax_property
}
pub(crate) fn syntax_constant() -> Color {
    active().syntax_constant
}
pub(crate) fn syntax_heading() -> Color {
    active().syntax_heading
}
pub(crate) fn syntax_link() -> Color {
    active().syntax_link
}
pub(crate) fn syntax_code() -> Color {
    active().syntax_code
}
pub(crate) fn syntax_emphasis() -> Color {
    active().syntax_emphasis
}
pub(crate) fn syntax_list() -> Color {
    active().syntax_list
}
pub(crate) fn syntax_quote() -> Color {
    active().syntax_quote
}
pub(crate) fn syntax_table() -> Color {
    active().syntax_table
}

pub(crate) fn syntax_modifier(index: usize) -> Modifier {
    active()
        .syntax_modifiers
        .get(index)
        .copied()
        .unwrap_or_else(Modifier::empty)
}

fn status_badge(slot: usize, fallback: Color) -> Style {
    active()
        .status_modes
        .get(slot)
        .copied()
        .flatten()
        .unwrap_or_else(|| mode_badge(fallback))
}

pub(crate) fn mode_normal_badge() -> Style {
    status_badge(0, accent())
}

pub(crate) fn mode_insert_badge() -> Style {
    status_badge(1, success())
}

pub(crate) fn mode_command_badge() -> Style {
    status_badge(2, warning())
}

pub(crate) fn mode_search_badge() -> Style {
    status_badge(3, warning())
}

pub(crate) fn mode_rename_badge() -> Style {
    status_badge(4, magenta())
}

pub(crate) fn mode_create_badge() -> Style {
    status_badge(5, accent())
}

pub(crate) fn mode_confirm_badge() -> Style {
    status_badge(6, warning())
}

pub(crate) fn text() -> Style {
    Style::default().fg(text_color()).bg(bg())
}

pub(crate) fn panel() -> Style {
    Style::default().fg(text_color()).bg(surface())
}

pub(crate) fn title() -> Style {
    Style::default()
        .fg(text_color())
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn status() -> Style {
    Style::default().fg(text_color()).bg(surface_alt())
}

pub(crate) fn selected() -> Style {
    Style::default()
        .fg(white())
        .bg(selection())
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn search_match() -> Style {
    Style::default().bg(selection())
}

pub(crate) fn search_active() -> Style {
    Style::default().bg(warning()).add_modifier(Modifier::BOLD)
}

pub(crate) fn current_line() -> Style {
    Style::default().fg(text_color()).bg(surface_alt())
}

pub(crate) fn current_line_number() -> Style {
    current_line().fg(accent()).add_modifier(Modifier::BOLD)
}
pub(crate) fn code_block() -> Style {
    Style::default().bg(code_block_bg())
}

pub(crate) fn mode_badge(color: Color) -> Style {
    Style::default()
        .fg(bg())
        .bg(color)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn border(active: bool) -> Style {
    Style::default().fg(if active { accent() } else { border_color() })
}

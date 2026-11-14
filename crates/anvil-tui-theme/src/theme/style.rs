//! Theme color and style accessors.

use super::*;

pub fn bg() -> Color {
    active().bg
}
pub fn surface() -> Color {
    active().surface
}
pub fn surface_alt() -> Color {
    active().surface_alt
}
pub fn code_block_bg() -> Color {
    active().code_block_bg
}
pub fn border_color() -> Color {
    active().border
}
pub fn text_color() -> Color {
    active().text
}
pub fn muted() -> Color {
    active().muted
}
pub fn dim() -> Color {
    active().dim
}
pub fn accent() -> Color {
    active().accent
}
pub fn selection() -> Color {
    active().selection
}
pub fn success() -> Color {
    active().success
}
pub fn warning() -> Color {
    active().warning
}
pub fn error() -> Color {
    active().error
}
pub fn magenta() -> Color {
    active().magenta
}
pub fn white() -> Color {
    active().white
}
pub fn syntax_keyword() -> Color {
    active().syntax_keyword
}
pub fn syntax_string() -> Color {
    active().syntax_string
}
pub fn syntax_comment() -> Color {
    active().syntax_comment
}
pub fn syntax_number() -> Color {
    active().syntax_number
}
pub fn syntax_function() -> Color {
    active().syntax_function
}
pub fn syntax_type() -> Color {
    active().syntax_type
}
pub fn syntax_property() -> Color {
    active().syntax_property
}
pub fn syntax_constant() -> Color {
    active().syntax_constant
}
pub fn syntax_heading() -> Color {
    active().syntax_heading
}
pub fn syntax_link() -> Color {
    active().syntax_link
}
pub fn syntax_code() -> Color {
    active().syntax_code
}
pub fn syntax_emphasis() -> Color {
    active().syntax_emphasis
}
pub fn syntax_list() -> Color {
    active().syntax_list
}
pub fn syntax_quote() -> Color {
    active().syntax_quote
}
pub fn syntax_table() -> Color {
    active().syntax_table
}

pub fn syntax_modifier(index: usize) -> Modifier {
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

pub fn mode_normal_badge() -> Style {
    status_badge(0, accent())
}

pub fn mode_insert_badge() -> Style {
    status_badge(1, success())
}

pub fn mode_command_badge() -> Style {
    status_badge(2, warning())
}

pub fn mode_search_badge() -> Style {
    status_badge(3, warning())
}

pub fn mode_rename_badge() -> Style {
    status_badge(4, magenta())
}

pub fn mode_create_badge() -> Style {
    status_badge(5, accent())
}

pub fn mode_confirm_badge() -> Style {
    status_badge(6, warning())
}

pub fn text() -> Style {
    Style::default().fg(text_color()).bg(bg())
}

pub fn panel() -> Style {
    Style::default().fg(text_color()).bg(surface())
}

pub fn title() -> Style {
    Style::default()
        .fg(text_color())
        .add_modifier(Modifier::BOLD)
}

pub fn status() -> Style {
    Style::default().fg(text_color()).bg(surface_alt())
}

pub fn selected() -> Style {
    Style::default()
        .fg(white())
        .bg(selection())
        .add_modifier(Modifier::BOLD)
}

pub fn search_match() -> Style {
    Style::default().bg(selection())
}

pub fn search_active() -> Style {
    Style::default().bg(warning()).add_modifier(Modifier::BOLD)
}

pub fn current_line() -> Style {
    Style::default().fg(text_color()).bg(surface_alt())
}

pub fn current_line_number() -> Style {
    current_line().fg(accent()).add_modifier(Modifier::BOLD)
}
pub fn code_block() -> Style {
    Style::default().bg(code_block_bg())
}

pub fn mode_badge(color: Color) -> Style {
    Style::default()
        .fg(bg())
        .bg(color)
        .add_modifier(Modifier::BOLD)
}

pub fn border(active: bool) -> Style {
    Style::default().fg(if active { accent() } else { border_color() })
}

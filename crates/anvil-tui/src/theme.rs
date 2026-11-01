//! Dark terminal palette with WCAG-AA text contrast.
//!
//! The TUI owns its background instead of inheriting the terminal theme so
//! syntax and chrome colors stay readable across terminal profiles.

use ratatui::style::{Color, Modifier, Style};

pub(crate) const BG: Color = Color::Rgb(13, 17, 23);
pub(crate) const SURFACE: Color = Color::Rgb(22, 27, 34);
pub(crate) const SURFACE_ALT: Color = Color::Rgb(30, 35, 46);
pub(crate) const BORDER: Color = Color::Rgb(48, 54, 61);
pub(crate) const TEXT: Color = Color::Rgb(230, 237, 243);
pub(crate) const MUTED: Color = Color::Rgb(139, 148, 158);
pub(crate) const DIM: Color = Color::Rgb(125, 133, 144);
pub(crate) const ACCENT: Color = Color::Rgb(88, 166, 255);
pub(crate) const SELECTION: Color = Color::Rgb(26, 95, 180);
pub(crate) const SUCCESS: Color = Color::Rgb(63, 185, 80);
pub(crate) const WARNING: Color = Color::Rgb(210, 153, 34);
pub(crate) const ERROR: Color = Color::Rgb(248, 81, 73);
pub(crate) const MAGENTA: Color = Color::Rgb(247, 119, 186);
pub(crate) const WHITE: Color = Color::Rgb(255, 255, 255);

pub(crate) const SYNTAX_KEYWORD: Color = Color::Rgb(255, 123, 114);
pub(crate) const SYNTAX_STRING: Color = Color::Rgb(165, 214, 255);
pub(crate) const SYNTAX_COMMENT: Color = Color::Rgb(139, 148, 158);
pub(crate) const SYNTAX_NUMBER: Color = Color::Rgb(121, 192, 255);
pub(crate) const SYNTAX_FUNCTION: Color = Color::Rgb(210, 168, 255);
pub(crate) const SYNTAX_TYPE: Color = Color::Rgb(126, 231, 135);
pub(crate) const SYNTAX_PROPERTY: Color = Color::Rgb(255, 166, 87);
pub(crate) const SYNTAX_CONSTANT: Color = Color::Rgb(242, 204, 96);
pub(crate) const SYNTAX_HEADING: Color = Color::Rgb(255, 123, 114);
pub(crate) const SYNTAX_LINK: Color = Color::Rgb(88, 166, 255);
pub(crate) const SYNTAX_CODE: Color = Color::Rgb(126, 231, 135);
pub(crate) const SYNTAX_EMPHASIS: Color = Color::Rgb(247, 119, 186);
pub(crate) const SYNTAX_LIST: Color = Color::Rgb(242, 204, 96);
pub(crate) const SYNTAX_QUOTE: Color = Color::Rgb(139, 148, 158);
pub(crate) const SYNTAX_TABLE: Color = Color::Rgb(255, 166, 87);

pub(crate) fn text() -> Style {
    Style::default().fg(TEXT).bg(BG)
}

pub(crate) fn panel() -> Style {
    Style::default().fg(TEXT).bg(SURFACE)
}

pub(crate) fn title() -> Style {
    Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
}

pub(crate) fn status() -> Style {
    Style::default().fg(TEXT).bg(SURFACE_ALT)
}

pub(crate) fn selected() -> Style {
    Style::default()
        .fg(WHITE)
        .bg(SELECTION)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn search_match() -> Style {
    Style::default().bg(SELECTION)
}

pub(crate) fn search_active() -> Style {
    Style::default().bg(WARNING).add_modifier(Modifier::BOLD)
}

pub(crate) fn current_line() -> Style {
    Style::default().fg(TEXT).bg(SURFACE_ALT)
}

pub(crate) fn mode_badge(color: Color) -> Style {
    Style::default()
        .fg(BG)
        .bg(color)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn border(active: bool) -> Style {
    Style::default().fg(if active { ACCENT } else { BORDER })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color) -> f64 {
        let Color::Rgb(red, green, blue) = color else {
            panic!("theme colors must be RGB");
        };
        let channel = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
    }

    fn contrast(left: Color, right: Color) -> f64 {
        let left = luminance(left);
        let right = luminance(right);
        (left.max(right) + 0.05) / (left.min(right) + 0.05)
    }

    #[test]
    fn palette_meets_wcag_aa_for_normal_text() {
        for color in [
            TEXT,
            MUTED,
            DIM,
            ACCENT,
            SUCCESS,
            WARNING,
            ERROR,
            MAGENTA,
            SYNTAX_KEYWORD,
            SYNTAX_STRING,
            SYNTAX_COMMENT,
            SYNTAX_NUMBER,
            SYNTAX_FUNCTION,
            SYNTAX_TYPE,
            SYNTAX_PROPERTY,
            SYNTAX_CONSTANT,
        ] {
            assert!(
                contrast(color, BG) >= 4.5,
                "{color:?} has insufficient contrast on the background"
            );
            assert!(
                contrast(color, SURFACE) >= 4.5,
                "{color:?} has insufficient contrast on panels"
            );
        }
        assert!(contrast(WHITE, SELECTION) >= 4.5);
        assert!(contrast(BG, ACCENT) >= 4.5);
        assert!(contrast(BG, SUCCESS) >= 4.5);
        assert!(contrast(BG, WARNING) >= 4.5);
        assert!(contrast(BG, ERROR) >= 4.5);
        assert!(contrast(BG, MAGENTA) >= 4.5);
    }
}

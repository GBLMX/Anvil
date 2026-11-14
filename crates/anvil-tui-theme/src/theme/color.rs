//! Color inspection and ANSI-256 quantization.

use super::*;

pub fn color_is_light(color: Color) -> bool {
    let Some((red, green, blue)) = color_rgb(color) else {
        return false;
    };
    (299 * u32::from(red) + 587 * u32::from(green) + 114 * u32::from(blue)) / 1000 > 128
}

pub fn color_rgb(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(red, green, blue) => Some((red, green, blue)),
        Color::Indexed(index) => Some(indexed_rgb(index)),
        Color::Black => Some((0, 0, 0)),
        Color::Red => Some((128, 0, 0)),
        Color::Green => Some((0, 128, 0)),
        Color::Yellow => Some((128, 128, 0)),
        Color::Blue => Some((0, 0, 128)),
        Color::Magenta => Some((128, 0, 128)),
        Color::Cyan => Some((0, 128, 128)),
        Color::Gray => Some((192, 192, 192)),
        Color::DarkGray => Some((128, 128, 128)),
        Color::LightRed => Some((255, 0, 0)),
        Color::LightGreen => Some((0, 255, 0)),
        Color::LightYellow => Some((255, 255, 0)),
        Color::LightBlue => Some((0, 0, 255)),
        Color::LightMagenta => Some((255, 0, 255)),
        Color::LightCyan => Some((0, 255, 255)),
        Color::White => Some((255, 255, 255)),
        Color::Reset => None,
    }
}

pub fn indexed_rgb(index: u8) -> (u8, u8, u8) {
    const ANSI: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    if index < 16 {
        return ANSI[usize::from(index)];
    }
    if index >= 232 {
        let value = 8 + (index - 232) * 10;
        return (value, value, value);
    }
    let cube = index - 16;
    let levels = [0u8, 95, 135, 175, 215, 255];
    (
        levels[usize::from(cube / 36)],
        levels[usize::from((cube / 6) % 6)],
        levels[usize::from(cube % 6)],
    )
}

pub fn quantize_theme(theme: &Theme) -> Theme {
    let mut theme = *theme;
    for color in [
        &mut theme.bg,
        &mut theme.surface,
        &mut theme.surface_alt,
        &mut theme.code_block_bg,
        &mut theme.border,
        &mut theme.text,
        &mut theme.muted,
        &mut theme.dim,
        &mut theme.accent,
        &mut theme.selection,
        &mut theme.success,
        &mut theme.warning,
        &mut theme.error,
        &mut theme.magenta,
        &mut theme.white,
        &mut theme.syntax_keyword,
        &mut theme.syntax_string,
        &mut theme.syntax_comment,
        &mut theme.syntax_number,
        &mut theme.syntax_function,
        &mut theme.syntax_type,
        &mut theme.syntax_property,
        &mut theme.syntax_constant,
        &mut theme.syntax_heading,
        &mut theme.syntax_link,
        &mut theme.syntax_code,
        &mut theme.syntax_emphasis,
        &mut theme.syntax_list,
        &mut theme.syntax_quote,
        &mut theme.syntax_table,
    ] {
        *color = quantize_color(*color);
    }
    for style in theme.status_modes.iter_mut().flatten() {
        if let Some(color) = style.fg {
            style.fg = Some(quantize_color(color));
        }
        if let Some(color) = style.bg {
            style.bg = Some(quantize_color(color));
        }
    }
    theme
}

pub fn quantize_color(color: Color) -> Color {
    match color {
        Color::Rgb(red, green, blue) => Color::Indexed(nearest_ansi_256(red, green, blue)),
        other => other,
    }
}

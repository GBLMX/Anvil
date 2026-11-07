//! Semantic terminal palettes with WCAG-AA text contrast.
//!
//! The TUI owns its background instead of inheriting the terminal theme so
//! syntax and chrome colors stay readable across terminal profiles.
use std::cell::{Cell, RefCell};

use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Theme {
    pub(crate) name: &'static str,
    pub(crate) bg: Color,
    pub(crate) surface: Color,
    pub(crate) surface_alt: Color,
    pub(crate) code_block_bg: Color,
    pub(crate) border: Color,
    pub(crate) text: Color,
    pub(crate) muted: Color,
    pub(crate) dim: Color,
    pub(crate) accent: Color,
    pub(crate) selection: Color,
    pub(crate) success: Color,
    pub(crate) warning: Color,
    pub(crate) error: Color,
    pub(crate) magenta: Color,
    pub(crate) white: Color,
    pub(crate) syntax_keyword: Color,
    pub(crate) syntax_string: Color,
    pub(crate) syntax_comment: Color,
    pub(crate) syntax_number: Color,
    pub(crate) syntax_function: Color,
    pub(crate) syntax_type: Color,
    pub(crate) syntax_property: Color,
    pub(crate) syntax_constant: Color,
    pub(crate) syntax_heading: Color,
    pub(crate) syntax_link: Color,
    pub(crate) syntax_code: Color,
    pub(crate) syntax_emphasis: Color,
    pub(crate) syntax_list: Color,
    pub(crate) syntax_quote: Color,
    pub(crate) syntax_table: Color,
}

impl Theme {
    pub(crate) const ANVIL_DARK: Self = Self {
        name: "anvil-dark",
        bg: Color::Rgb(13, 17, 23),
        surface: Color::Rgb(22, 27, 34),
        surface_alt: Color::Rgb(30, 35, 46),
        code_block_bg: Color::Rgb(36, 40, 59),
        border: Color::Rgb(48, 54, 61),
        text: Color::Rgb(230, 237, 243),
        muted: Color::Rgb(139, 148, 158),
        dim: Color::Rgb(125, 133, 144),
        accent: Color::Rgb(88, 166, 255),
        selection: Color::Rgb(26, 95, 180),
        success: Color::Rgb(63, 185, 80),
        warning: Color::Rgb(210, 153, 34),
        error: Color::Rgb(248, 81, 73),
        magenta: Color::Rgb(247, 119, 186),
        white: Color::Rgb(255, 255, 255),
        syntax_keyword: Color::Rgb(255, 123, 114),
        syntax_string: Color::Rgb(165, 214, 255),
        syntax_comment: Color::Rgb(139, 148, 158),
        syntax_number: Color::Rgb(121, 192, 255),
        syntax_function: Color::Rgb(210, 168, 255),
        syntax_type: Color::Rgb(126, 231, 135),
        syntax_property: Color::Rgb(255, 166, 87),
        syntax_constant: Color::Rgb(242, 204, 96),
        syntax_heading: Color::Rgb(255, 123, 114),
        syntax_link: Color::Rgb(88, 166, 255),
        syntax_code: Color::Rgb(126, 231, 135),
        syntax_emphasis: Color::Rgb(247, 119, 186),
        syntax_list: Color::Rgb(242, 204, 96),
        syntax_quote: Color::Rgb(139, 148, 158),
        syntax_table: Color::Rgb(255, 166, 87),
    };

    pub(crate) const ONE_DARK: Self = Self {
        name: "one-dark",
        bg: Color::Rgb(40, 44, 52),
        surface: Color::Rgb(33, 37, 43),
        surface_alt: Color::Rgb(44, 49, 58),
        code_block_bg: Color::Rgb(51, 56, 66),
        border: Color::Rgb(75, 82, 99),
        text: Color::Rgb(171, 178, 191),
        muted: Color::Rgb(154, 162, 177),
        dim: Color::Rgb(139, 147, 162),
        accent: Color::Rgb(97, 175, 239),
        selection: Color::Rgb(55, 100, 145),
        success: Color::Rgb(152, 195, 121),
        warning: Color::Rgb(229, 192, 123),
        error: Color::Rgb(232, 132, 140),
        magenta: Color::Rgb(198, 120, 221),
        white: Color::Rgb(255, 255, 255),
        syntax_keyword: Color::Rgb(198, 120, 221),
        syntax_string: Color::Rgb(152, 195, 121),
        syntax_comment: Color::Rgb(143, 150, 163),
        syntax_number: Color::Rgb(209, 154, 102),
        syntax_function: Color::Rgb(97, 175, 239),
        syntax_type: Color::Rgb(229, 192, 123),
        syntax_property: Color::Rgb(232, 132, 140),
        syntax_constant: Color::Rgb(209, 154, 102),
        syntax_heading: Color::Rgb(232, 132, 140),
        syntax_link: Color::Rgb(97, 175, 239),
        syntax_code: Color::Rgb(152, 195, 121),
        syntax_emphasis: Color::Rgb(198, 120, 221),
        syntax_list: Color::Rgb(229, 192, 123),
        syntax_quote: Color::Rgb(143, 150, 163),
        syntax_table: Color::Rgb(209, 154, 102),
    };

    pub(crate) const SOLARIZED_LIGHT: Self = Self {
        name: "solarized-light",
        bg: Color::Rgb(253, 246, 227),
        surface: Color::Rgb(238, 232, 213),
        surface_alt: Color::Rgb(228, 221, 200),
        code_block_bg: Color::Rgb(238, 232, 213),
        border: Color::Rgb(213, 205, 182),
        text: Color::Rgb(75, 95, 102),
        muted: Color::Rgb(82, 102, 109),
        dim: Color::Rgb(82, 102, 109),
        accent: Color::Rgb(26, 93, 143),
        selection: Color::Rgb(47, 111, 143),
        success: Color::Rgb(91, 103, 0),
        warning: Color::Rgb(116, 89, 0),
        error: Color::Rgb(168, 58, 18),
        magenta: Color::Rgb(165, 31, 99),
        white: Color::Rgb(255, 255, 255),
        syntax_keyword: Color::Rgb(91, 103, 0),
        syntax_string: Color::Rgb(20, 115, 109),
        syntax_comment: Color::Rgb(82, 102, 109),
        syntax_number: Color::Rgb(168, 58, 18),
        syntax_function: Color::Rgb(26, 93, 143),
        syntax_type: Color::Rgb(116, 89, 0),
        syntax_property: Color::Rgb(168, 58, 18),
        syntax_constant: Color::Rgb(116, 89, 0),
        syntax_heading: Color::Rgb(168, 58, 18),
        syntax_link: Color::Rgb(26, 93, 143),
        syntax_code: Color::Rgb(20, 115, 109),
        syntax_emphasis: Color::Rgb(165, 31, 99),
        syntax_list: Color::Rgb(116, 89, 0),
        syntax_quote: Color::Rgb(82, 102, 109),
        syntax_table: Color::Rgb(168, 58, 18),
    };

    pub(crate) const GRUVBOX_DARK: Self = Self {
        name: "gruvbox-dark",
        bg: Color::Rgb(40, 40, 40),
        surface: Color::Rgb(50, 48, 47),
        surface_alt: Color::Rgb(60, 56, 54),
        code_block_bg: Color::Rgb(69, 64, 61),
        border: Color::Rgb(80, 73, 69),
        text: Color::Rgb(235, 219, 178),
        muted: Color::Rgb(213, 196, 161),
        dim: Color::Rgb(189, 174, 147),
        accent: Color::Rgb(131, 165, 152),
        selection: Color::Rgb(63, 115, 117),
        success: Color::Rgb(184, 187, 38),
        warning: Color::Rgb(250, 189, 47),
        error: Color::Rgb(255, 123, 105),
        magenta: Color::Rgb(211, 134, 155),
        white: Color::Rgb(251, 241, 199),
        syntax_keyword: Color::Rgb(255, 123, 105),
        syntax_string: Color::Rgb(184, 187, 38),
        syntax_comment: Color::Rgb(189, 174, 147),
        syntax_number: Color::Rgb(211, 134, 155),
        syntax_function: Color::Rgb(142, 192, 124),
        syntax_type: Color::Rgb(250, 189, 47),
        syntax_property: Color::Rgb(254, 128, 25),
        syntax_constant: Color::Rgb(250, 189, 47),
        syntax_heading: Color::Rgb(255, 123, 105),
        syntax_link: Color::Rgb(131, 165, 152),
        syntax_code: Color::Rgb(184, 187, 38),
        syntax_emphasis: Color::Rgb(211, 134, 155),
        syntax_list: Color::Rgb(250, 189, 47),
        syntax_quote: Color::Rgb(189, 174, 147),
        syntax_table: Color::Rgb(254, 128, 25),
    };

    pub(crate) const NORD: Self = Self {
        name: "nord",
        bg: Color::Rgb(46, 52, 64),
        surface: Color::Rgb(59, 66, 82),
        surface_alt: Color::Rgb(67, 76, 94),
        code_block_bg: Color::Rgb(76, 86, 106),
        border: Color::Rgb(76, 86, 106),
        text: Color::Rgb(236, 239, 244),
        muted: Color::Rgb(216, 222, 233),
        dim: Color::Rgb(184, 192, 204),
        accent: Color::Rgb(136, 192, 208),
        selection: Color::Rgb(63, 95, 138),
        success: Color::Rgb(163, 190, 140),
        warning: Color::Rgb(235, 203, 139),
        error: Color::Rgb(255, 154, 168),
        magenta: Color::Rgb(217, 179, 212),
        white: Color::Rgb(236, 239, 244),
        syntax_keyword: Color::Rgb(154, 180, 214),
        syntax_string: Color::Rgb(163, 190, 140),
        syntax_comment: Color::Rgb(184, 196, 214),
        syntax_number: Color::Rgb(217, 179, 212),
        syntax_function: Color::Rgb(136, 192, 208),
        syntax_type: Color::Rgb(143, 188, 187),
        syntax_property: Color::Rgb(255, 154, 168),
        syntax_constant: Color::Rgb(235, 203, 139),
        syntax_heading: Color::Rgb(255, 154, 168),
        syntax_link: Color::Rgb(136, 192, 208),
        syntax_code: Color::Rgb(163, 190, 140),
        syntax_emphasis: Color::Rgb(217, 179, 212),
        syntax_list: Color::Rgb(235, 203, 139),
        syntax_quote: Color::Rgb(184, 196, 214),
        syntax_table: Color::Rgb(255, 154, 168),
    };

    pub(crate) const CATPPUCCIN_MOCHA: Self = Self {
        name: "catppuccin-mocha",
        bg: Color::Rgb(30, 30, 46),
        surface: Color::Rgb(24, 24, 37),
        surface_alt: Color::Rgb(49, 50, 68),
        code_block_bg: Color::Rgb(49, 50, 68),
        border: Color::Rgb(69, 71, 90),
        text: Color::Rgb(205, 214, 244),
        muted: Color::Rgb(186, 194, 222),
        dim: Color::Rgb(166, 173, 200),
        accent: Color::Rgb(137, 180, 250),
        selection: Color::Rgb(69, 71, 90),
        success: Color::Rgb(166, 227, 161),
        warning: Color::Rgb(249, 226, 175),
        error: Color::Rgb(243, 139, 168),
        magenta: Color::Rgb(203, 166, 247),
        white: Color::Rgb(255, 255, 255),
        syntax_keyword: Color::Rgb(203, 166, 247),
        syntax_string: Color::Rgb(166, 227, 161),
        syntax_comment: Color::Rgb(147, 153, 178),
        syntax_number: Color::Rgb(250, 179, 135),
        syntax_function: Color::Rgb(137, 180, 250),
        syntax_type: Color::Rgb(249, 226, 175),
        syntax_property: Color::Rgb(243, 139, 168),
        syntax_constant: Color::Rgb(250, 179, 135),
        syntax_heading: Color::Rgb(243, 139, 168),
        syntax_link: Color::Rgb(137, 180, 250),
        syntax_code: Color::Rgb(166, 227, 161),
        syntax_emphasis: Color::Rgb(245, 194, 231),
        syntax_list: Color::Rgb(249, 226, 175),
        syntax_quote: Color::Rgb(147, 153, 178),
        syntax_table: Color::Rgb(250, 179, 135),
    };
}

pub(crate) const BUILTINS: &[Theme] = &[
    Theme::ANVIL_DARK,
    Theme::ONE_DARK,
    Theme::SOLARIZED_LIGHT,
    Theme::GRUVBOX_DARK,
    Theme::NORD,
    Theme::CATPPUCCIN_MOCHA,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Fixed(&'static str),
    Auto {
        dark: &'static str,
        light: &'static str,
    },
}

thread_local! {
    static ACTIVE: Cell<&'static Theme> = const { Cell::new(&Theme::ANVIL_DARK) };
    static CHOICE: RefCell<Choice> = const { RefCell::new(Choice::Fixed("anvil-dark")) };
    static AUTO: Cell<(&'static str, &'static str)> =
        const { Cell::new(("anvil-dark", "solarized-light")) };
    static RUNTIME: RefCell<Option<Choice>> = const { RefCell::new(None) };
    static BACKGROUND_IS_LIGHT: Cell<Option<bool>> = const { Cell::new(None) };
}

fn active() -> &'static Theme {
    ACTIVE.with(Cell::get)
}

fn resolve(name: &str) -> Result<&'static Theme, String> {
    BUILTINS
        .iter()
        .find(|theme| theme.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            format!(
                "unknown theme {name:?}; available: {}",
                BUILTINS
                    .iter()
                    .map(|theme| theme.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

pub(crate) fn canonical(name: &str) -> Result<&'static str, String> {
    resolve(name).map(|theme| theme.name)
}

pub(crate) fn set_background_is_light(value: Option<bool>) {
    BACKGROUND_IS_LIGHT.with(|background| background.set(value));
}

fn terminal_is_light() -> bool {
    if let Some(value) = BACKGROUND_IS_LIGHT.with(Cell::get) {
        return value;
    }
    std::env::var("COLORFGBG")
        .ok()
        .and_then(|value| {
            value
                .rsplit(';')
                .next()
                .and_then(|background| background.parse::<u8>().ok())
        })
        .is_some_and(|background| background >= 8)
}

pub(crate) fn apply_choice(choice: Choice, runtime: bool) -> Result<(), String> {
    let (dark, light, selected) = match choice {
        Choice::Fixed(name) => {
            let theme = resolve(name)?;
            (theme, theme, theme)
        }
        Choice::Auto { dark, light } => {
            let dark_theme = resolve(dark)?;
            let light_theme = resolve(light)?;
            let selected = if terminal_is_light() {
                light_theme
            } else {
                dark_theme
            };
            (dark_theme, light_theme, selected)
        }
    };
    if matches!(choice, Choice::Auto { .. }) {
        AUTO.with(|auto| auto.set((dark.name, light.name)));
    }
    ACTIVE.with(|active| active.set(selected));
    CHOICE.with(|current| *current.borrow_mut() = choice);
    if runtime {
        RUNTIME.with(|current| *current.borrow_mut() = Some(choice));
    }
    Ok(())
}

pub(crate) fn names() -> Vec<&'static str> {
    BUILTINS.iter().map(|theme| theme.name).collect()
}

pub(crate) fn name() -> &'static str {
    active().name
}

pub(crate) fn choice() -> Choice {
    CHOICE.with(|choice| *choice.borrow())
}

pub(crate) fn runtime_choice() -> Option<Choice> {
    RUNTIME.with(|choice| *choice.borrow())
}

pub(crate) fn auto_mapping() -> (&'static str, &'static str) {
    AUTO.with(Cell::get)
}

pub(crate) fn set_theme(name: &str) -> Result<(), String> {
    let theme = resolve(name)?;
    apply_choice(Choice::Fixed(theme.name), true)
}

pub(crate) fn set_startup_theme(name: &str) -> Result<(), String> {
    let theme = resolve(name)?;
    apply_choice(Choice::Fixed(theme.name), false)
}

pub(crate) fn set_auto(dark: &str, light: &str) -> Result<(), String> {
    let dark = resolve(dark)?;
    let light = resolve(light)?;
    apply_choice(
        Choice::Auto {
            dark: dark.name,
            light: light.name,
        },
        true,
    )
}

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
    fn builtin_palettes_meet_wcag_aa_for_normal_text() {
        for theme in BUILTINS {
            for color in [
                theme.text,
                theme.muted,
                theme.dim,
                theme.accent,
                theme.success,
                theme.warning,
                theme.error,
                theme.magenta,
                theme.syntax_keyword,
                theme.syntax_string,
                theme.syntax_comment,
                theme.syntax_number,
                theme.syntax_function,
                theme.syntax_type,
                theme.syntax_property,
                theme.syntax_constant,
            ] {
                assert!(
                    contrast(color, theme.bg) >= 4.5,
                    "{}: {color:?} has insufficient contrast on the background",
                    theme.name
                );
                assert!(
                    contrast(color, theme.surface) >= 4.5,
                    "{}: {color:?} has insufficient contrast on panels",
                    theme.name
                );
            }
            assert!(
                contrast(theme.white, theme.selection) >= 4.5,
                "{}: selection foreground",
                theme.name
            );
            assert!(
                contrast(theme.bg, theme.accent) >= 4.5,
                "{}: accent badge",
                theme.name
            );
            for color in [theme.success, theme.warning, theme.error, theme.magenta] {
                assert!(
                    contrast(theme.bg, color) >= 4.5,
                    "{}: {color:?} badge",
                    theme.name
                );
            }
        }
    }

    #[test]
    fn theme_switch_rejects_unknown_names() {
        let original = name();
        let error = set_theme("missing").expect_err("unknown theme");
        assert!(error.contains("available:"));
        assert_eq!(name(), original);
    }

    #[test]
    fn auto_mode_uses_detected_background() {
        set_background_is_light(Some(true));
        apply_choice(
            Choice::Auto {
                dark: "nord",
                light: "solarized-light",
            },
            true,
        )
        .expect("light auto");
        assert_eq!(name(), "solarized-light");
        set_background_is_light(Some(false));
        apply_choice(
            Choice::Auto {
                dark: "nord",
                light: "solarized-light",
            },
            true,
        )
        .expect("dark auto");
        assert_eq!(name(), "nord");
        set_background_is_light(None);
        set_startup_theme("anvil-dark").expect("reset");
    }
}

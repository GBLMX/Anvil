//! Semantic terminal palettes with WCAG-AA text contrast.
//!
//! The TUI owns its background instead of inheriting the terminal theme so
//! syntax and chrome colors stay readable across terminal profiles.
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

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
    pub(crate) syntax_modifiers: [Modifier; 15],
    pub(crate) status_modes: [Option<Style>; 7],
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
        syntax_modifiers: [Modifier::empty(); 15],
        status_modes: [None; 7],
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
        syntax_modifiers: [Modifier::empty(); 15],
        status_modes: [None; 7],
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
        syntax_modifiers: [Modifier::empty(); 15],
        status_modes: [None; 7],
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
        syntax_modifiers: [Modifier::empty(); 15],
        status_modes: [None; 7],
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
        syntax_modifiers: [Modifier::empty(); 15],
        status_modes: [None; 7],
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
        syntax_modifiers: [Modifier::empty(); 15],
        status_modes: [None; 7],
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

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ThemeFile {
    ui: UiSpec,
    syntax: SyntaxSpec,
    status: StatusSpec,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UiSpec {
    background: Option<String>,
    foreground: Option<String>,
    selection: Option<String>,
    cursorline: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SyntaxSpec {
    keyword: Option<StyleSpec>,
    string: Option<StyleSpec>,
    comment: Option<StyleSpec>,
    function: Option<StyleSpec>,
    number: Option<StyleSpec>,
    r#type: Option<StyleSpec>,
    property: Option<StyleSpec>,
    constant: Option<StyleSpec>,
    heading: Option<StyleSpec>,
    link: Option<StyleSpec>,
    code: Option<StyleSpec>,
    emphasis: Option<StyleSpec>,
    list: Option<StyleSpec>,
    quote: Option<StyleSpec>,
    table: Option<StyleSpec>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct StatusSpec {
    mode_normal: Option<StyleSpec>,
    mode_insert: Option<StyleSpec>,
    mode_command: Option<StyleSpec>,
    mode_search: Option<StyleSpec>,
    mode_rename: Option<StyleSpec>,
    mode_create: Option<StyleSpec>,
    mode_confirm: Option<StyleSpec>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct StyleSpec {
    fg: Option<String>,
    bg: Option<String>,
    bold: Option<bool>,
    italic: Option<bool>,
}

#[derive(Clone)]
struct CustomTheme {
    path: PathBuf,
    source: String,
    theme: &'static Theme,
}

thread_local! {
    static ACTIVE: Cell<&'static Theme> = const { Cell::new(&Theme::ANVIL_DARK) };
    static CHOICE: RefCell<Choice> = const { RefCell::new(Choice::Fixed("anvil-dark")) };
    static AUTO: Cell<(&'static str, &'static str)> =
        const { Cell::new(("anvil-dark", "solarized-light")) };
    static RUNTIME: RefCell<Option<Choice>> = const { RefCell::new(None) };
    static BACKGROUND_IS_LIGHT: Cell<Option<bool>> = const { Cell::new(None) };
    static CUSTOM: RefCell<Vec<CustomTheme>> = const { RefCell::new(Vec::new()) };
    static THEME_DIR_MTIME: Cell<Option<SystemTime>> = const { Cell::new(None) };
    static LAST_RELOAD_CHECK: Cell<Option<Instant>> = const { Cell::new(None) };
}

fn theme_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ANVIL_THEMES") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(config).join("anvil/themes");
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/anvil/themes"))
}

pub(crate) fn load_custom_themes() -> Result<(), String> {
    let Some(directory) = theme_dir() else {
        return Ok(());
    };
    load_custom_themes_from(&directory)
}

pub(crate) fn load_custom_themes_from(directory: &Path) -> Result<(), String> {
    let old = CUSTOM.with(|custom| custom.borrow().clone());
    let mut paths = Vec::new();
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    let mut custom = Vec::new();
    for path in paths {
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("theme {}: {error}", path.display()))?;
        if let Some(previous) = old
            .iter()
            .find(|theme| theme.path == path && theme.source == source)
        {
            custom.push(previous.clone());
            continue;
        }
        let theme = parse_theme_file(&path, &source)?;
        custom.push(CustomTheme {
            path,
            source,
            theme: Box::leak(Box::new(theme)),
        });
    }
    CUSTOM.with(|current| *current.borrow_mut() = custom);
    Ok(())
}

fn style_modifiers(spec: &StyleSpec) -> Modifier {
    let mut modifiers = Modifier::empty();
    if spec.bold == Some(true) {
        modifiers |= Modifier::BOLD;
    }
    if spec.italic == Some(true) {
        modifiers |= Modifier::ITALIC;
    }
    modifiers
}

fn parse_theme_file(path: &Path, source: &str) -> Result<Theme, String> {
    let file: ThemeFile = toml_edit::de::from_str(source)
        .map_err(|error| format!("invalid theme TOML {}: {error}", path.display()))?;
    let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
        return Err(format!("invalid theme filename: {}", path.display()));
    };
    if name.is_empty() {
        return Err(format!("invalid theme filename: {}", path.display()));
    }
    let mut theme = Theme::ANVIL_DARK;
    theme.name = Box::leak(name.to_owned().into_boxed_str());
    apply_color(&mut theme.bg, file.ui.background.as_deref())?;
    apply_color(&mut theme.text, file.ui.foreground.as_deref())?;
    apply_color(&mut theme.selection, file.ui.selection.as_deref())?;
    apply_color(&mut theme.surface_alt, file.ui.cursorline.as_deref())?;
    if let Some(style) = &file.syntax.keyword {
        apply_color(&mut theme.syntax_keyword, style.fg.as_deref())?;
        theme.syntax_modifiers[0] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.string {
        apply_color(&mut theme.syntax_string, style.fg.as_deref())?;
        theme.syntax_modifiers[1] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.comment {
        apply_color(&mut theme.syntax_comment, style.fg.as_deref())?;
        theme.syntax_modifiers[2] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.function {
        apply_color(&mut theme.syntax_function, style.fg.as_deref())?;
        theme.syntax_modifiers[4] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.number {
        apply_color(&mut theme.syntax_number, style.fg.as_deref())?;
        theme.syntax_modifiers[3] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.r#type {
        apply_color(&mut theme.syntax_type, style.fg.as_deref())?;
        theme.syntax_modifiers[5] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.property {
        apply_color(&mut theme.syntax_property, style.fg.as_deref())?;
        theme.syntax_modifiers[6] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.constant {
        apply_color(&mut theme.syntax_constant, style.fg.as_deref())?;
        theme.syntax_modifiers[7] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.heading {
        apply_color(&mut theme.syntax_heading, style.fg.as_deref())?;
        theme.syntax_modifiers[8] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.link {
        apply_color(&mut theme.syntax_link, style.fg.as_deref())?;
        theme.syntax_modifiers[9] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.code {
        apply_color(&mut theme.syntax_code, style.fg.as_deref())?;
        theme.syntax_modifiers[10] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.emphasis {
        apply_color(&mut theme.syntax_emphasis, style.fg.as_deref())?;
        theme.syntax_modifiers[11] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.list {
        apply_color(&mut theme.syntax_list, style.fg.as_deref())?;
        theme.syntax_modifiers[12] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.quote {
        apply_color(&mut theme.syntax_quote, style.fg.as_deref())?;
        theme.syntax_modifiers[13] = style_modifiers(style);
    }
    if let Some(style) = &file.syntax.table {
        apply_color(&mut theme.syntax_table, style.fg.as_deref())?;
        theme.syntax_modifiers[14] = style_modifiers(style);
    }
    apply_status_style(
        &mut theme.status_modes[0],
        file.status.mode_normal.as_ref(),
        theme.text,
        theme.surface_alt,
    )?;
    apply_status_style(
        &mut theme.status_modes[1],
        file.status.mode_insert.as_ref(),
        theme.text,
        theme.surface_alt,
    )?;
    apply_status_style(
        &mut theme.status_modes[2],
        file.status.mode_command.as_ref(),
        theme.text,
        theme.surface_alt,
    )?;
    apply_status_style(
        &mut theme.status_modes[3],
        file.status.mode_search.as_ref(),
        theme.text,
        theme.surface_alt,
    )?;
    apply_status_style(
        &mut theme.status_modes[4],
        file.status.mode_rename.as_ref(),
        theme.text,
        theme.surface_alt,
    )?;
    apply_status_style(
        &mut theme.status_modes[5],
        file.status.mode_create.as_ref(),
        theme.text,
        theme.surface_alt,
    )?;
    apply_status_style(
        &mut theme.status_modes[6],
        file.status.mode_confirm.as_ref(),
        theme.text,
        theme.surface_alt,
    )?;
    Ok(theme)
}

fn apply_color(target: &mut Color, value: Option<&str>) -> Result<(), String> {
    if let Some(value) = value {
        *target = parse_color(value)?;
    }
    Ok(())
}

fn parse_color(value: &str) -> Result<Color, String> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() != 6 {
            return Err(format!("invalid hex color {value:?}"));
        }
        let red = u8::from_str_radix(&hex[0..2], 16)
            .map_err(|_| format!("invalid hex color {value:?}"))?;
        let green = u8::from_str_radix(&hex[2..4], 16)
            .map_err(|_| format!("invalid hex color {value:?}"))?;
        let blue = u8::from_str_radix(&hex[4..6], 16)
            .map_err(|_| format!("invalid hex color {value:?}"))?;
        return Ok(degrade_color(Color::Rgb(red, green, blue)));
    }
    if let Some(index) = value.strip_prefix("color") {
        let index = index
            .parse::<u8>()
            .map_err(|_| format!("invalid ANSI color {value:?}"))?;
        if index > 15 {
            return Err(format!("invalid ANSI color {value:?}"));
        }
        return Ok(Color::Indexed(index));
    }
    let color = match value {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "bright_black" | "gray" => Color::DarkGray,
        "bright_red" => Color::LightRed,
        "bright_green" => Color::LightGreen,
        "bright_yellow" => Color::LightYellow,
        "bright_blue" => Color::LightBlue,
        "bright_magenta" => Color::LightMagenta,
        "bright_cyan" => Color::LightCyan,
        "bright_white" => Color::White,
        "reset" | "default" => Color::Reset,
        _ => return Err(format!("unknown color {value:?}")),
    };
    Ok(color)
}

fn degrade_color(color: Color) -> Color {
    if !truecolor_supported() {
        return match color {
            Color::Rgb(red, green, blue) => Color::Indexed(nearest_ansi_256(red, green, blue)),
            other => other,
        };
    }
    color
}

fn truecolor_supported() -> bool {
    if let Some(value) = std::env::var_os("ANVIL_TRUECOLOR") {
        return value != "0";
    }
    std::env::var("COLORTERM").is_ok_and(|value| {
        value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
    }) || std::env::var("TERM")
        .is_ok_and(|value| value.contains("direct") || value.contains("truecolor"))
}

fn nearest_ansi_256(red: u8, green: u8, blue: u8) -> u8 {
    let to_cube = |value: u8| -> u8 {
        if value < 48 {
            0
        } else if value < 115 {
            1
        } else {
            ((value as u16 - 35) / 40).min(5) as u8
        }
    };
    let r = to_cube(red);
    let g = to_cube(green);
    let b = to_cube(blue);
    16 + 36 * r + 6 * g + b
}

fn apply_status_style(
    target: &mut Option<Style>,
    style: Option<&StyleSpec>,
    default_fg: Color,
    default_bg: Color,
) -> Result<(), String> {
    let Some(style) = style else {
        return Ok(());
    };
    let fg = match style.fg.as_deref() {
        Some(value) => parse_color(value)?,
        None => default_fg,
    };
    let bg = match style.bg.as_deref() {
        Some(value) => parse_color(value)?,
        None => default_bg,
    };
    *target = Some(
        Style::default()
            .fg(fg)
            .bg(bg)
            .add_modifier(style_modifiers(style)),
    );
    Ok(())
}

fn theme_signature(directory: &Path) -> Option<SystemTime> {
    let entries = fs::read_dir(directory).ok()?;
    let mut signature = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        signature = Some(signature.map_or(modified, |current: SystemTime| current.max(modified)));
    }
    signature
}

pub(crate) fn reload_custom_if_changed() -> Result<bool, String> {
    let now = Instant::now();
    let should_check = LAST_RELOAD_CHECK.with(|last| {
        let due = last
            .get()
            .is_none_or(|last| now.duration_since(last) >= Duration::from_secs(1));
        if due {
            last.set(Some(now));
        }
        due
    });
    if !should_check {
        return Ok(false);
    }
    let Some(directory) = theme_dir() else {
        return Ok(false);
    };
    let Some(modified) = theme_signature(&directory) else {
        return Ok(false);
    };
    if THEME_DIR_MTIME.with(Cell::get) == Some(modified) {
        return Ok(false);
    }
    THEME_DIR_MTIME.with(|current| current.set(Some(modified)));
    load_custom_themes_from(&directory)?;
    if let Choice::Fixed(name) = choice()
        && let Some(theme) = CUSTOM.with(|custom| {
            custom
                .borrow()
                .iter()
                .find(|theme| theme.theme.name == name)
                .map(|theme| theme.theme)
        })
    {
        ACTIVE.with(|active| active.set(theme));
        CHOICE.with(|current| *current.borrow_mut() = Choice::Fixed(theme.name));
    }
    Ok(true)
}

fn active() -> &'static Theme {
    ACTIVE.with(Cell::get)
}

fn resolve(name: &str) -> Result<&'static Theme, String> {
    if let Some(theme) = BUILTINS
        .iter()
        .find(|theme| theme.name.eq_ignore_ascii_case(name))
    {
        return Ok(theme);
    }
    if let Some(theme) = CUSTOM.with(|custom| {
        custom
            .borrow()
            .iter()
            .find(|theme| theme.theme.name.eq_ignore_ascii_case(name))
            .map(|theme| theme.theme)
    }) {
        return Ok(theme);
    }
    Err(format!(
        "unknown theme {name:?}; available: {}",
        names().join(", ")
    ))
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
    let mut names = BUILTINS.iter().map(|theme| theme.name).collect::<Vec<_>>();
    CUSTOM.with(|custom| {
        names.extend(custom.borrow().iter().map(|theme| theme.theme.name));
    });
    names
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
    fn parses_custom_theme_colors_and_ansi_names() {
        assert_eq!(parse_color("color5").expect("ansi"), Color::Indexed(5));
        assert_eq!(parse_color("bright_blue").expect("named"), Color::LightBlue);
        assert!(parse_color("#GGGGGG").is_err());
        assert!(parse_color("not-a-color").is_err());
        assert_eq!(nearest_ansi_256(255, 0, 0), 196);
    }

    #[test]
    fn loads_custom_theme_and_switches_instantly() {
        let directory = tempfile::tempdir().expect("tempdir");
        fs::write(
            directory.path().join("cyberpunk.toml"),
            "[ui]\nbackground = \"#101010\"\nforeground = \"#eeeeee\"\nselection = \"color5\"\ncursorline = \"#202020\"\n[syntax]\nkeyword = { fg = \"#ff00aa\", bold = true }\n[status]\nmode_normal = { bg = \"#ff00aa\", fg = \"color0\" }\nmode_insert = { bg = \"#00ff00\", fg = \"color0\" }\n",
        )
        .expect("write");
        load_custom_themes_from(directory.path()).expect("load");
        assert!(names().contains(&"cyberpunk"));
        set_theme("cyberpunk").expect("switch");
        assert_eq!(name(), "cyberpunk");
        assert_eq!(selection(), parse_color("color5").expect("ansi"));
        assert_eq!(syntax_keyword(), parse_color("#ff00aa").expect("hex"));
        assert!(syntax_modifier(0).contains(Modifier::BOLD));
        assert_eq!(
            mode_normal_badge().fg,
            Some(parse_color("color0").expect("ansi"))
        );
        assert_eq!(
            mode_normal_badge().bg,
            Some(parse_color("#ff00aa").expect("hex"))
        );
        assert_eq!(
            mode_insert_badge().bg,
            Some(parse_color("#00ff00").expect("hex"))
        );
        set_startup_theme("anvil-dark").expect("reset");
    }

    #[test]
    fn theme_signature_skips_unreadable_entries() {
        let directory = tempfile::tempdir().expect("tempdir");
        fs::write(
            directory.path().join("valid.toml"),
            "[ui]\nbackground = \"#101010\"\n",
        )
        .expect("write");
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            directory.path().join("missing-target"),
            directory.path().join("broken.toml"),
        )
        .expect("symlink");
        assert!(theme_signature(directory.path()).is_some());
    }

    #[test]
    fn invalid_custom_theme_keeps_previous_registry() {
        let directory = tempfile::tempdir().expect("tempdir");
        fs::write(
            directory.path().join("broken.toml"),
            "[ui]\nbackground = \"#GGGGGG\"\n",
        )
        .expect("write");
        assert!(load_custom_themes_from(directory.path()).is_err());
        assert!(!names().contains(&"broken"));
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

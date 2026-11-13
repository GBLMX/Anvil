//! Semantic terminal palettes with WCAG-AA text contrast.
//!
//! The TUI owns its background instead of inheriting the terminal theme so
//! syntax and chrome colors stay readable across terminal profiles.
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use ratatui::style::{Color, Modifier, Style};

mod color;
pub(crate) use color::*;
mod style;
pub(crate) use style::*;
mod custom;
use custom::{CustomTheme, parse_theme_file};
mod palette;
use palette::BUILTINS;

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
    static BASE_ACTIVE: Cell<&'static Theme> = const { Cell::new(&Theme::ANVIL_DARK) };
    static HIGH_CONTRAST: Cell<bool> = const { Cell::new(false) };
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
        activate(theme);
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

pub(crate) fn init_color_support() {
    refresh_active();
}

pub(crate) fn set_high_contrast(enabled: bool) {
    HIGH_CONTRAST.with(|value| value.set(enabled));
    refresh_active();
}

fn activate(theme: &'static Theme) {
    BASE_ACTIVE.with(|active| active.set(theme));
    refresh_active();
}

fn refresh_active() {
    let mut theme = BASE_ACTIVE.with(Cell::get);
    if HIGH_CONTRAST.with(Cell::get) {
        theme = Box::leak(Box::new(high_contrast_theme(theme)));
    }
    if !truecolor_supported() {
        theme = Box::leak(Box::new(quantize_theme(theme)));
    }
    ACTIVE.with(|active| active.set(theme));
}

fn high_contrast_theme(theme: &Theme) -> Theme {
    let mut theme = *theme;
    let light = color_is_light(theme.bg);
    if light {
        theme.text = Color::Rgb(0, 0, 0);
        theme.muted = Color::Rgb(30, 30, 30);
        theme.dim = Color::Rgb(45, 45, 45);
        theme.border = Color::Rgb(50, 50, 50);
    } else {
        theme.text = Color::Rgb(255, 255, 255);
        theme.muted = Color::Rgb(220, 220, 220);
        theme.dim = Color::Rgb(195, 195, 195);
        theme.border = Color::Rgb(150, 150, 150);
    }
    theme.selection = Color::Rgb(0, 70, 160);
    theme
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
    activate(selected);
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
    fn high_contrast_theme_uses_readable_text() {
        let theme = high_contrast_theme(&Theme::ANVIL_DARK);
        assert_eq!(theme.text, Color::Rgb(255, 255, 255));
        assert!(contrast(theme.text, theme.bg) >= 7.0);
        assert!(contrast(theme.muted, theme.bg) >= 4.5);
    }

    #[test]
    fn high_contrast_detects_degraded_light_backgrounds() {
        let light = high_contrast_theme(&Theme::SOLARIZED_LIGHT);
        assert_eq!(light.text, Color::Rgb(0, 0, 0));
        let degraded = quantize_theme(&Theme::SOLARIZED_LIGHT);
        assert!(matches!(degraded.bg, Color::Indexed(_)));
        let light = high_contrast_theme(&degraded);
        assert_eq!(light.text, Color::Rgb(0, 0, 0));
        assert!(color_is_light(light.bg));
    }

    #[test]
    fn lists_modern_builtins() {
        for name in ["tokyonight", "omp-kde", "helix-base16", "cyberpunk-volt"] {
            assert!(names().contains(&name), "{name}");
        }
    }

    #[test]
    fn lists_boxpigma_themes() {
        for name in [
            "boxpigma",
            "dracula",
            "gruvbox",
            "solarized",
            "tokyo-night",
            "catppuccin",
            "monokai",
            "rose-pine",
            "kanagawa",
            "github-light",
            "gruvbox-light",
            "one-light",
            "catppuccin-latte",
            "cyberpunk-volt",
            "cyberpunk-fury",
            "cyberpunk-hot",
        ] {
            assert!(names().contains(&name), "{name}");
        }
    }

    #[test]
    fn quantizes_builtin_rgb_colors_for_ansi_terminals() {
        let theme = quantize_theme(&Theme::ANVIL_DARK);
        assert!(matches!(theme.bg, Color::Indexed(_)));
        assert!(matches!(theme.text, Color::Indexed(_)));
        assert!(matches!(theme.accent, Color::Indexed(_)));
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

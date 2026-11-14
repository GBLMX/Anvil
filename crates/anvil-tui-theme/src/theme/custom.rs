//! Custom theme file parsing.

use super::*;
use serde::Deserialize;

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
pub struct CustomTheme {
    pub path: PathBuf,
    pub source: String,
    pub theme: &'static Theme,
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

pub fn parse_theme_file(path: &Path, source: &str) -> Result<Theme, String> {
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

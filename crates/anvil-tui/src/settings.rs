//! Typed settings registry with explicit defaults, file, and environment layers.

use crate::theme::{self, Choice};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const PROJECT_SETTINGS: &str = ".anvil.toml";
pub(crate) const MAX_BYTES: u64 = 64 * 1024;
pub(crate) const DEFAULT_SCROLL_MARGIN: usize = 3;
pub(crate) const MAX_SCROLL_MARGIN: usize = 16;
pub(crate) const DEFAULT_MAX_BUFFERS: usize = 9;
pub(crate) const MAX_MAX_BUFFERS: usize = 99;
pub(crate) const DEFAULT_SIDEBAR_PERCENT: u16 = 20;
pub(crate) const MIN_SIDEBAR_PERCENT: u16 = 10;
pub(crate) const MAX_SIDEBAR_PERCENT: u16 = 40;

const ENV_SCROLL_MARGIN: &str = "ANVIL_EDITOR_SCROLL_MARGIN";
const ENV_AUTO_SAVE: &str = "ANVIL_EDITOR_AUTO_SAVE";
const ENV_SOFT_WRAP: &str = "ANVIL_EDITOR_SOFT_WRAP";
const ENV_FORMAT_ON_SAVE: &str = "ANVIL_EDITOR_FORMAT_ON_SAVE";
const ENV_MAX_BUFFERS: &str = "ANVIL_EDITOR_MAX_BUFFERS";
const ENV_SIDEBAR_PERCENT: &str = "ANVIL_EDITOR_SIDEBAR_PERCENT";
const ENV_SEARCH_CASE_SENSITIVE: &str = "ANVIL_SEARCH_CASE_SENSITIVE";
const ENV_INLAY_HINTS: &str = "ANVIL_LSP_INLAY_HINTS";
const ENV_REDUCED_MOTION: &str = "ANVIL_REDUCED_MOTION";
const ENV_HIGH_CONTRAST: &str = "ANVIL_HIGH_CONTRAST";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EditorSettings {
    pub(crate) scroll_margin: usize,
    pub(crate) max_buffers: usize,
    pub(crate) auto_save: bool,
    pub(crate) soft_wrap: bool,
    pub(crate) format_on_save: bool,
    pub(crate) sidebar_percent: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SearchSettings {
    pub(crate) case_sensitive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LspSettings {
    pub(crate) inlay_hints: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UiSettings {
    pub(crate) reduced_motion: bool,
    pub(crate) high_contrast: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Settings {
    pub(crate) theme: Choice,
    pub(crate) editor: EditorSettings,
    pub(crate) search: SearchSettings,
    pub(crate) lsp: LspSettings,
    pub(crate) ui: UiSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Choice::Fixed("anvil-dark"),
            editor: EditorSettings {
                scroll_margin: DEFAULT_SCROLL_MARGIN,
                max_buffers: DEFAULT_MAX_BUFFERS,
                auto_save: false,
                soft_wrap: false,
                format_on_save: false,
                sidebar_percent: DEFAULT_SIDEBAR_PERCENT,
            },
            search: SearchSettings {
                case_sensitive: false,
            },
            lsp: LspSettings { inlay_hints: true },
            ui: UiSettings {
                reduced_motion: false,
                high_contrast: false,
            },
        }
    }
}

pub(crate) fn lsp_command_override(
    language_id: &str,
    default_args: &[String],
) -> Option<(String, Vec<String>)> {
    let path = path().ok()?;
    if !path.exists() {
        return None;
    }
    if fs::metadata(&path).ok()?.len() > MAX_BYTES {
        return None;
    }
    let text = fs::read_to_string(&path).ok()?;
    let file: SettingsFile = toml_edit::de::from_str(&text).ok()?;
    resolve_lsp_command(file.lsp.commands.get(language_id)?, default_args)
}

fn resolve_lsp_command(
    command: &LspCommandFile,
    default_args: &[String],
) -> Option<(String, Vec<String>)> {
    match command {
        LspCommandFile::Command(command) => {
            (!command.trim().is_empty()).then(|| (command.clone(), default_args.to_vec()))
        }
        LspCommandFile::Argv(argv) => {
            let (command, args) = argv.split_first()?;
            (!command.trim().is_empty()).then(|| (command.clone(), args.to_vec()))
        }
    }
}

/// Load settings for one workspace.
///
/// The project layer is read only when its workspace root is trusted. An
/// explicit `ANVIL_SETTINGS` file is a complete override, matching the
/// pre-project-layer behavior and keeping tests isolated.
pub(crate) fn load_for_workspace(
    workspace: &Path,
    project_trusted: bool,
) -> Result<Settings, String> {
    let user = path()?;
    let project = if explicit_settings_path().is_some() {
        None
    } else {
        project_layer(workspace, project_trusted)
    };
    load_from_layers(Some(&user), project.as_deref())
}

fn project_layer(workspace: &Path, trusted: bool) -> Option<PathBuf> {
    trusted.then(|| workspace.join(PROJECT_SETTINGS))
}

fn load_from_layers(user: Option<&Path>, project: Option<&Path>) -> Result<Settings, String> {
    let mut merged = toml_edit::DocumentMut::new();
    let mut layers = Vec::new();
    for (path, is_project) in [(user, false), (project, true)] {
        let Some(path) = path else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let text = read_settings_text(path)?;
        let document = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|error| format!("{}: invalid settings TOML: {error}", path.display()))?;
        if is_project {
            let file: SettingsFile = toml_edit::de::from_str(&text)
                .map_err(|error| format!("{}: invalid settings TOML: {error}", path.display()))?;
            if !file.lsp.commands.is_empty() {
                return Err(format!(
                    "{}: [lsp.commands] is not allowed in project settings",
                    path.display()
                ));
            }
        }
        merge_document(&mut merged, &document);
        layers.push(path);
    }
    let settings = if layers.is_empty() {
        Settings::default()
    } else {
        let names = layers
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        parse(&merged.to_string()).map_err(|error| format!("{names}: {error}"))?
    };
    apply_env(settings, |name| std::env::var(name).ok())
}

fn read_settings_text(path: &Path) -> Result<String, String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if metadata.len() > MAX_BYTES {
        return Err(format!("{} exceeds {} bytes", path.display(), MAX_BYTES));
    }
    fs::read_to_string(path).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn merge_document(base: &mut toml_edit::DocumentMut, overlay: &toml_edit::DocumentMut) {
    merge_table(base.as_table_mut(), overlay.as_table());
}

fn merge_table(base: &mut toml_edit::Table, overlay: &toml_edit::Table) {
    for (key, value) in overlay.iter() {
        match (base.get_mut(key), value) {
            (Some(toml_edit::Item::Table(base)), toml_edit::Item::Table(overlay)) => {
                merge_table(base, overlay);
            }
            _ => {
                base.insert(key, value.clone());
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn load_file_from_path(path: &Path) -> Result<Settings, String> {
    if !path.exists() {
        return Ok(Settings::default());
    }
    parse(&read_settings_text(path)?)
}

pub(crate) fn save(settings: Settings) -> Result<PathBuf, String> {
    let path = path()?;
    save_to_path(&path, settings)?;
    Ok(path)
}

pub(crate) fn save_to_path(path: &Path, settings: Settings) -> Result<(), String> {
    let mut document = if path.exists() {
        let metadata = fs::metadata(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if metadata.len() > MAX_BYTES {
            return Err(format!("{} exceeds {} bytes", path.display(), MAX_BYTES));
        }
        let text = fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let _ = parse(&text)?;
        text.parse::<toml_edit::DocumentMut>()
            .map_err(|error| format!("invalid settings TOML: {error}"))?
    } else {
        toml_edit::DocumentMut::new()
    };
    let theme = table_mut(&mut document, "theme")?;
    for key in ["mode", "name", "dark", "light"] {
        theme.remove(key);
    }
    match settings.theme {
        Choice::Fixed(name) => {
            theme["mode"] = toml_edit::value("fixed");
            theme["name"] = toml_edit::value(name);
        }
        Choice::Auto { dark, light } => {
            theme["mode"] = toml_edit::value("auto");
            theme["dark"] = toml_edit::value(dark);
            theme["light"] = toml_edit::value(light);
        }
    }
    let editor = table_mut(&mut document, "editor")?;
    editor["scroll_margin"] = toml_edit::value(settings.editor.scroll_margin as i64);
    editor["max_buffers"] = toml_edit::value(settings.editor.max_buffers as i64);
    editor["auto_save"] = toml_edit::value(settings.editor.auto_save);
    editor["soft_wrap"] = toml_edit::value(settings.editor.soft_wrap);
    editor["format_on_save"] = toml_edit::value(settings.editor.format_on_save);
    editor["sidebar_percent"] = toml_edit::value(settings.editor.sidebar_percent as i64);
    let search = table_mut(&mut document, "search")?;
    search["case_sensitive"] = toml_edit::value(settings.search.case_sensitive);
    let lsp = table_mut(&mut document, "lsp")?;
    lsp["inlay_hints"] = toml_edit::value(settings.lsp.inlay_hints);
    let ui = table_mut(&mut document, "ui")?;
    ui["reduced_motion"] = toml_edit::value(settings.ui.reduced_motion);
    ui["high_contrast"] = toml_edit::value(settings.ui.high_contrast);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let temporary = path.with_extension(format!("toml.tmp-{}", std::process::id()));
    fs::write(&temporary, document.to_string())
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("cannot replace {}: {error}", path.display()));
    }
    Ok(())
}

fn table_mut<'a>(
    document: &'a mut toml_edit::DocumentMut,
    key: &str,
) -> Result<&'a mut toml_edit::Table, String> {
    if !document.contains_key(key) {
        document.insert(key, toml_edit::Item::Table(toml_edit::Table::new()));
    }
    document[key]
        .as_table_mut()
        .ok_or_else(|| format!("[{key}] must be a table"))
}

pub(crate) fn parse(text: &str) -> Result<Settings, String> {
    let document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| format!("invalid settings TOML: {error}"))?;
    for (key, _) in document.iter() {
        if !matches!(key, "theme" | "editor" | "search" | "lsp" | "ui") {
            return Err(format!("unknown settings section [{key}]"));
        }
    }
    let file: SettingsFile =
        toml_edit::de::from_str(text).map_err(|error| format!("invalid settings TOML: {error}"))?;
    let mut settings = Settings {
        theme: parse_theme(file.theme)?,
        ..Settings::default()
    };
    if let Some(scroll_margin) = file.editor.scroll_margin {
        if scroll_margin > MAX_SCROLL_MARGIN {
            return Err(format!(
                "editor.scroll_margin must be 0..={MAX_SCROLL_MARGIN}, got {scroll_margin}"
            ));
        }
        settings.editor.scroll_margin = scroll_margin;
    }
    if let Some(max_buffers) = file.editor.max_buffers {
        if !(1..=MAX_MAX_BUFFERS).contains(&max_buffers) {
            return Err(format!(
                "editor.max_buffers must be 1..={MAX_MAX_BUFFERS}, got {max_buffers}"
            ));
        }
        settings.editor.max_buffers = max_buffers;
    }
    if let Some(auto_save) = file.editor.auto_save {
        settings.editor.auto_save = auto_save;
    }
    if let Some(soft_wrap) = file.editor.soft_wrap {
        settings.editor.soft_wrap = soft_wrap;
    }
    if let Some(format_on_save) = file.editor.format_on_save {
        settings.editor.format_on_save = format_on_save;
    }
    if let Some(sidebar_percent) = file.editor.sidebar_percent {
        if !(MIN_SIDEBAR_PERCENT..=MAX_SIDEBAR_PERCENT).contains(&sidebar_percent) {
            return Err(format!(
                "editor.sidebar_percent must be {MIN_SIDEBAR_PERCENT}..={MAX_SIDEBAR_PERCENT}, got {sidebar_percent}"
            ));
        }
        settings.editor.sidebar_percent = sidebar_percent;
    }
    if let Some(case_sensitive) = file.search.case_sensitive {
        settings.search.case_sensitive = case_sensitive;
    }
    if let Some(inlay_hints) = file.lsp.inlay_hints {
        settings.lsp.inlay_hints = inlay_hints;
    }
    if let Some(reduced_motion) = file.ui.reduced_motion {
        settings.ui.reduced_motion = reduced_motion;
    }
    if let Some(high_contrast) = file.ui.high_contrast {
        settings.ui.high_contrast = high_contrast;
    }
    Ok(settings)
}
fn parse_theme(theme: Option<ThemeFile>) -> Result<Choice, String> {
    let Some(theme) = theme else {
        return Ok(Choice::Fixed("anvil-dark"));
    };
    match theme.mode.as_deref().unwrap_or("fixed") {
        "fixed" => {
            if theme.dark.is_some() || theme.light.is_some() {
                return Err("fixed theme mode must not set dark or light".to_owned());
            }
            let name = theme::canonical(theme.name.as_deref().unwrap_or("anvil-dark"))?;
            Ok(Choice::Fixed(name))
        }
        "auto" => {
            if theme.name.is_some() {
                return Err("auto theme mode must not set name".to_owned());
            }
            let dark = theme::canonical(theme.dark.as_deref().unwrap_or("anvil-dark"))?;
            let light = theme::canonical(theme.light.as_deref().unwrap_or("solarized-light"))?;
            Ok(Choice::Auto { dark, light })
        }
        mode => Err(format!(
            "unknown theme mode {mode:?}; expected fixed or auto"
        )),
    }
}

fn apply_env(
    mut settings: Settings,
    get: impl Fn(&str) -> Option<String>,
) -> Result<Settings, String> {
    if let Some(value) = get(ENV_SCROLL_MARGIN) {
        let margin = value.parse::<usize>().map_err(|_| {
            format!("{ENV_SCROLL_MARGIN} must be an integer 0..={MAX_SCROLL_MARGIN}")
        })?;
        if margin > MAX_SCROLL_MARGIN {
            return Err(format!(
                "{ENV_SCROLL_MARGIN} must be 0..={MAX_SCROLL_MARGIN}, got {margin}"
            ));
        }
        settings.editor.scroll_margin = margin;
    }
    if let Some(value) = get(ENV_MAX_BUFFERS) {
        let max_buffers = value
            .parse::<usize>()
            .map_err(|_| format!("{ENV_MAX_BUFFERS} must be an integer 1..={MAX_MAX_BUFFERS}"))?;
        if !(1..=MAX_MAX_BUFFERS).contains(&max_buffers) {
            return Err(format!(
                "{ENV_MAX_BUFFERS} must be 1..={MAX_MAX_BUFFERS}, got {max_buffers}"
            ));
        }
        settings.editor.max_buffers = max_buffers;
    }
    if let Some(value) = get(ENV_AUTO_SAVE) {
        settings.editor.auto_save = parse_bool(ENV_AUTO_SAVE, &value)?;
    }
    if let Some(value) = get(ENV_SOFT_WRAP) {
        settings.editor.soft_wrap = parse_bool(ENV_SOFT_WRAP, &value)?;
    }
    if let Some(value) = get(ENV_FORMAT_ON_SAVE) {
        settings.editor.format_on_save = parse_bool(ENV_FORMAT_ON_SAVE, &value)?;
    }
    if let Some(value) = get(ENV_SIDEBAR_PERCENT) {
        let percent = value.parse::<u16>().map_err(|_| {
            format!(
                "{ENV_SIDEBAR_PERCENT} must be an integer {MIN_SIDEBAR_PERCENT}..={MAX_SIDEBAR_PERCENT}"
            )
        })?;
        if !(MIN_SIDEBAR_PERCENT..=MAX_SIDEBAR_PERCENT).contains(&percent) {
            return Err(format!(
                "{ENV_SIDEBAR_PERCENT} must be {MIN_SIDEBAR_PERCENT}..={MAX_SIDEBAR_PERCENT}, got {percent}"
            ));
        }
        settings.editor.sidebar_percent = percent;
    }
    if let Some(value) = get(ENV_SEARCH_CASE_SENSITIVE) {
        settings.search.case_sensitive = parse_bool(ENV_SEARCH_CASE_SENSITIVE, &value)?;
    }
    if let Some(value) = get(ENV_INLAY_HINTS) {
        settings.lsp.inlay_hints = parse_bool(ENV_INLAY_HINTS, &value)?;
    }
    if let Some(value) = get(ENV_REDUCED_MOTION) {
        settings.ui.reduced_motion = parse_bool(ENV_REDUCED_MOTION, &value)?;
    }
    if let Some(value) = get(ENV_HIGH_CONTRAST) {
        settings.ui.high_contrast = parse_bool(ENV_HIGH_CONTRAST, &value)?;
    }
    Ok(settings)
}

fn parse_bool(name: &str, value: &str) -> Result<bool, String> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(format!("{name} must be true or false, got {value:?}")),
    }
}

fn explicit_settings_path() -> Option<PathBuf> {
    let explicit = std::env::var_os("ANVIL_SETTINGS")?;
    let explicit = explicit.to_string_lossy();
    (!explicit.trim().is_empty()).then(|| PathBuf::from(explicit.into_owned()))
}

pub(crate) fn path() -> Result<PathBuf, String> {
    if let Some(explicit) = explicit_settings_path() {
        return Ok(explicit);
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        let config = config.to_string_lossy();
        if !config.trim().is_empty() {
            return Ok(PathBuf::from(config.into_owned()).join("anvil/settings.toml"));
        }
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| "cannot resolve settings path: HOME is unset".to_owned())?;
    Ok(PathBuf::from(home).join(".config/anvil/settings.toml"))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SettingsFile {
    theme: Option<ThemeFile>,
    editor: EditorFile,
    search: SearchFile,
    lsp: LspFile,
    ui: UiFile,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ThemeFile {
    mode: Option<String>,
    name: Option<String>,
    dark: Option<String>,
    light: Option<String>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct EditorFile {
    scroll_margin: Option<usize>,
    max_buffers: Option<usize>,
    auto_save: Option<bool>,
    soft_wrap: Option<bool>,
    format_on_save: Option<bool>,
    sidebar_percent: Option<u16>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SearchFile {
    case_sensitive: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LspFile {
    inlay_hints: Option<bool>,
    commands: BTreeMap<String, LspCommandFile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum LspCommandFile {
    Command(String),
    Argv(Vec<String>),
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UiFile {
    reduced_motion: Option<bool>,
    high_contrast: Option<bool>,
}

impl crate::Model {
    pub(crate) fn apply_settings(&mut self, settings: Settings) {
        let theme_changed = self.config.settings.theme != settings.theme;
        self.config.settings = settings;
        self.search.state.options.case_sensitive = settings.search.case_sensitive;
        self.overlays.lsp_results.inlay_hints.clear();
        self.overlays.lsp_results.inlay_pending = None;
        self.overlays.lsp_results.inlay_checked = None;
        if theme_changed {
            if let Err(error) = theme::apply_choice(settings.theme, false) {
                self.status.text = format!("E: theme: {error}");
            }
        }
        theme::set_high_contrast(settings.ui.high_contrast);
        self.file.dirty = true;
    }

    pub(crate) fn persist_theme_status(&mut self, status: String) -> String {
        if let Some(choice) = theme::runtime_choice() {
            self.config.settings.theme = choice;
        }
        if !self.appearance.persist_theme {
            return status;
        }
        match save(self.config.settings) {
            Ok(_) => format!("{status} (saved)"),
            Err(error) => format!("{status}; E: {error}"),
        }
    }
}

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_explicit() {
        assert_eq!(
            Settings::default(),
            Settings {
                theme: Choice::Fixed("anvil-dark"),
                editor: EditorSettings {
                    scroll_margin: 3,
                    max_buffers: DEFAULT_MAX_BUFFERS,
                    auto_save: false,
                    soft_wrap: false,
                    format_on_save: false,
                    sidebar_percent: DEFAULT_SIDEBAR_PERCENT,
                },
                search: SearchSettings {
                    case_sensitive: false,
                },
                lsp: LspSettings { inlay_hints: true },
                ui: UiSettings {
                    reduced_motion: false,
                    high_contrast: false,
                },
            }
        );
    }

    #[test]
    fn parses_and_validates_sidebar_percent() {
        let settings = parse("[editor]\nsidebar_percent = 30\n").expect("sidebar width");
        assert_eq!(settings.editor.sidebar_percent, 30);
        assert!(parse("[editor]\nsidebar_percent = 5\n").is_err());
        assert!(parse("[editor]\nsidebar_percent = 41\n").is_err());
    }

    #[test]
    fn project_layer_merges_over_user_settings() {
        let directory = tempfile::tempdir().expect("tempdir");
        let user = directory.path().join("user.toml");
        let project = directory.path().join(PROJECT_SETTINGS);
        fs::write(
            &user,
            "[theme]\nname = \"anvil-dark\"\n[editor]\nscroll_margin = 2\nsoft_wrap = false\n",
        )
        .expect("user settings");
        fs::write(
            &project,
            "[theme]\nname = \"nord\"\n[editor]\nsoft_wrap = true\n",
        )
        .expect("project settings");

        let settings = load_from_layers(Some(&user), Some(&project)).expect("merged settings");
        assert_eq!(settings.theme, Choice::Fixed("nord"));
        assert_eq!(settings.editor.scroll_margin, 2);
        assert!(settings.editor.soft_wrap);
    }

    #[test]
    fn project_layer_requires_trust() {
        let workspace = Path::new("/workspace");
        assert_eq!(project_layer(workspace, false), None);
        assert_eq!(
            project_layer(workspace, true),
            Some(workspace.join(PROJECT_SETTINGS))
        );
    }

    #[test]
    fn project_layer_rejects_lsp_commands() {
        let directory = tempfile::tempdir().expect("tempdir");
        let project = directory.path().join(PROJECT_SETTINGS);
        fs::write(&project, "[lsp.commands]\nrust = \"/tmp/evil\"\n").expect("project settings");

        let error = load_from_layers(None, Some(&project)).expect_err("lsp commands");
        assert!(error.contains("[lsp.commands]"), "{error}");
    }

    #[test]
    fn explicit_settings_replace_the_project_layer() {
        let _guard = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let directory = tempfile::tempdir().expect("tempdir");
        let workspace = directory.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        fs::write(
            workspace.join(PROJECT_SETTINGS),
            "[theme]\nname = \"nord\"\n",
        )
        .expect("project settings");
        let explicit = directory.path().join("explicit.toml");
        fs::write(&explicit, "[theme]\nname = \"omp-kde\"\n").expect("explicit settings");

        unsafe { std::env::set_var("ANVIL_SETTINGS", &explicit) };
        let result = load_for_workspace(&workspace, true);
        unsafe { std::env::remove_var("ANVIL_SETTINGS") };
        let settings = result.expect("explicit settings");
        assert_eq!(settings.theme, Choice::Fixed("omp-kde"));
    }

    #[test]
    fn parses_lsp_command_overrides() {
        let (tool, args) = resolve_lsp_command(
            &LspCommandFile::Command("/usr/bin/json-server".to_owned()),
            &["--stdio".to_owned()],
        )
        .expect("string command");
        assert_eq!(tool, "/usr/bin/json-server");
        assert_eq!(args, ["--stdio"]);

        let settings = parse(
            "[lsp]\ninlay_hints = true\n[lsp.commands]\njson = [\"node\", \"server.js\", \"--stdio\"]\nrust = \"/usr/bin/rust-analyzer\"\n",
        )
        .expect("settings with lsp commands");
        assert!(settings.lsp.inlay_hints);
        let file: SettingsFile = toml_edit::de::from_str(
            "[lsp.commands]\njson = [\"node\", \"server.js\", \"--stdio\"]\nrust = \"/usr/bin/rust-analyzer\"\n",
        )
        .expect("lsp commands");
        let (tool, args) =
            resolve_lsp_command(&file.lsp.commands["json"], &[]).expect("json command");
        assert_eq!(tool, "node");
        assert_eq!(args, ["server.js", "--stdio"]);
        let (tool, args) = resolve_lsp_command(&file.lsp.commands["rust"], &["--stdio".to_owned()])
            .expect("rust command");
        assert_eq!(tool, "/usr/bin/rust-analyzer");
        assert_eq!(args, ["--stdio"]);
    }

    #[test]
    fn string_json_override_keeps_builtin_stdio_args() {
        let file: SettingsFile = toml_edit::de::from_str(
            "[lsp.commands]\njson = \"/usr/bin/vscode-json-language-server\"\n",
        )
        .expect("json command");
        let (tool, args) = resolve_lsp_command(&file.lsp.commands["json"], &["--stdio".to_owned()])
            .expect("json command");
        assert_eq!(tool, "/usr/bin/vscode-json-language-server");
        assert_eq!(args, ["--stdio"]);
    }

    #[test]
    fn parses_all_domains_and_theme() {
        let settings = parse(
            "[theme]\nmode = \"fixed\"\n[editor]\nscroll_margin = 1\nmax_buffers = 4\nauto_save = true\nsoft_wrap = true\nformat_on_save = true\n[search]\ncase_sensitive = true\n[lsp]\ninlay_hints = false\n",
        )
        .expect("settings");
        assert_eq!(settings.theme, Choice::Fixed("anvil-dark"));
        assert_eq!(settings.editor.scroll_margin, 1);
        assert_eq!(settings.editor.max_buffers, 4);
        assert!(settings.editor.auto_save);
        assert!(settings.editor.soft_wrap);
        assert!(settings.editor.format_on_save);
        assert!(settings.search.case_sensitive);
        assert!(!settings.lsp.inlay_hints);
    }

    #[test]
    fn loads_file_values_and_missing_files_as_defaults() {
        let directory =
            std::env::temp_dir().join(format!("anvil-settings-registry-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("directory");
        let path = directory.join("settings.toml");
        assert_eq!(
            load_file_from_path(&path).expect("missing"),
            Settings::default()
        );
        fs::write(
            &path,
            "[editor]\nscroll_margin = 2\n[search]\ncase_sensitive = true\n[lsp]\ninlay_hints = false\n",
        )
        .expect("write");
        let settings = load_file_from_path(&path).expect("settings");
        assert_eq!(settings.editor.scroll_margin, 2);
        assert!(settings.search.case_sensitive);
        assert!(!settings.lsp.inlay_hints);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn save_writes_theme_and_domains() {
        let directory =
            std::env::temp_dir().join(format!("anvil-settings-save-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("directory");
        let path = directory.join("settings.toml");
        fs::write(
            &path,
            "[theme]\nmode = \"fixed\"\nname = \"nord\"\n[editor]\nscroll_margin = 1\n",
        )
        .expect("write");
        let mut settings = Settings {
            theme: Choice::Fixed("nord"),
            ..Settings::default()
        };
        settings.editor.scroll_margin = 5;
        settings.editor.max_buffers = 12;
        settings.search.case_sensitive = true;
        settings.lsp.inlay_hints = false;
        save_to_path(&path, settings).expect("save");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("[theme]"));
        assert!(text.contains("name = \"nord\""));
        assert!(text.contains("max_buffers = 12"));
        assert_eq!(load_file_from_path(&path).expect("load"), settings);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn theme_parser_rejects_ambiguous_and_unknown_values() {
        assert!(parse("[theme]\nmode = \"auto\"\nname = \"nord\"\n").is_err());
        assert!(parse("[theme]\ndark = \"nord\"\n").is_err());
        assert!(parse("[theme]\nname = \"missing\"\n").is_err());
        assert!(parse("[theme]\nmode = \"system\"\n").is_err());
    }
    #[test]
    fn parses_auto_theme_mode() {
        let settings =
            parse("[theme]\nmode = \"auto\"\ndark = \"nord\"\nlight = \"solarized-light\"\n")
                .expect("auto theme");
        assert_eq!(
            settings.theme,
            Choice::Auto {
                dark: "nord",
                light: "solarized-light",
            }
        );
    }

    #[test]
    fn rejects_unknown_sections_fields_and_values() {
        assert!(parse("[ui]\nmode = \"x\"\n").is_err());
        assert!(parse("[editor]\ntab_width = 2\n").is_err());
        assert!(parse("[editor]\nscroll_margin = 99\n").is_err());
        assert!(parse("[search]\ncase_sensitive = \"yes\"\n").is_err());
        assert!(parse("[editor]\nmax_buffers = 0\n").is_err());
        assert!(parse("[editor]\nmax_buffers = 100\n").is_err());
    }

    #[test]
    fn environment_overrides_file_values() {
        let settings = apply_env(
            parse("[editor]\nscroll_margin = 1\n[search]\ncase_sensitive = true\n[lsp]\ninlay_hints = false\n")
                .expect("file"),
            |name| match name {
                ENV_SCROLL_MARGIN => Some("4".to_owned()),
                ENV_MAX_BUFFERS => Some("12".to_owned()),
                ENV_SEARCH_CASE_SENSITIVE => Some("false".to_owned()),
                ENV_INLAY_HINTS => Some("true".to_owned()),
                _ => None,
            },
        )
        .expect("env");
        assert_eq!(settings.editor.max_buffers, 12);
        assert_eq!(settings.editor.scroll_margin, 4);
        assert!(!settings.search.case_sensitive);
        assert!(settings.lsp.inlay_hints);
    }

    #[test]
    fn environment_errors_are_visible() {
        assert!(
            apply_env(Settings::default(), |name| {
                (name == ENV_SCROLL_MARGIN).then(|| "99".to_owned())
            })
            .is_err()
        );
        assert!(
            apply_env(Settings::default(), |name| {
                (name == ENV_MAX_BUFFERS).then(|| "100".to_owned())
            })
            .is_err()
        );
        assert!(
            apply_env(Settings::default(), |name| {
                (name == ENV_INLAY_HINTS).then(|| "maybe".to_owned())
            })
            .is_err()
        );
    }

    #[test]
    fn applying_settings_updates_model_defaults() {
        let mut model = crate::Model::new(crate::EditorState::from_text("x"), None);
        let mut settings = Settings::default();
        settings.search.case_sensitive = true;
        settings.lsp.inlay_hints = false;
        model.apply_settings(settings);
        assert!(model.search.state.options.case_sensitive);
        assert!(!model.config.settings.lsp.inlay_hints);
    }
}

#[cfg(test)]
mod accessibility_tests {
    use super::*;

    #[test]
    fn parses_and_overrides_ui_accessibility_settings() {
        let settings = parse("[ui]\nreduced_motion = true\nhigh_contrast = true\n").expect("parse");
        assert!(settings.ui.reduced_motion);
        assert!(settings.ui.high_contrast);
        let settings = apply_env(settings, |name| {
            (name == ENV_REDUCED_MOTION).then(|| "false".to_owned())
        })
        .expect("env");
        assert!(!settings.ui.reduced_motion);
        assert!(settings.ui.high_contrast);
    }
}

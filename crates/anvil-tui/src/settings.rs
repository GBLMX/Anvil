//! Typed settings registry with explicit defaults, file, and environment layers.

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const MAX_BYTES: u64 = 64 * 1024;
pub(crate) const DEFAULT_SCROLL_MARGIN: usize = 3;
pub(crate) const MAX_SCROLL_MARGIN: usize = 16;

const ENV_SCROLL_MARGIN: &str = "ANVIL_EDITOR_SCROLL_MARGIN";
const ENV_AUTO_SAVE: &str = "ANVIL_EDITOR_AUTO_SAVE";
const ENV_SEARCH_CASE_SENSITIVE: &str = "ANVIL_SEARCH_CASE_SENSITIVE";
const ENV_INLAY_HINTS: &str = "ANVIL_LSP_INLAY_HINTS";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EditorSettings {
    pub(crate) scroll_margin: usize,
    pub(crate) auto_save: bool,
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
pub(crate) struct Settings {
    pub(crate) editor: EditorSettings,
    pub(crate) search: SearchSettings,
    pub(crate) lsp: LspSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            editor: EditorSettings {
                scroll_margin: DEFAULT_SCROLL_MARGIN,
                auto_save: false,
            },
            search: SearchSettings {
                case_sensitive: false,
            },
            lsp: LspSettings { inlay_hints: true },
        }
    }
}

pub(crate) fn load() -> Result<Settings, String> {
    let path = path()?;
    let settings = load_file_from_path(&path)?;
    apply_env(settings, |name| std::env::var(name).ok())
}

pub(crate) fn load_file_from_path(path: &Path) -> Result<Settings, String> {
    if !path.exists() {
        return Ok(Settings::default());
    }
    let metadata =
        fs::metadata(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if metadata.len() > MAX_BYTES {
        return Err(format!("{} exceeds {} bytes", path.display(), MAX_BYTES));
    }
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    parse(&text)
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
    let editor = table_mut(&mut document, "editor")?;
    editor["scroll_margin"] = toml_edit::value(settings.editor.scroll_margin as i64);
    editor["auto_save"] = toml_edit::value(settings.editor.auto_save);
    let search = table_mut(&mut document, "search")?;
    search["case_sensitive"] = toml_edit::value(settings.search.case_sensitive);
    let lsp = table_mut(&mut document, "lsp")?;
    lsp["inlay_hints"] = toml_edit::value(settings.lsp.inlay_hints);
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
        if !matches!(key, "theme" | "editor" | "search" | "lsp") {
            return Err(format!("unknown settings section [{key}]"));
        }
    }
    let file: SettingsFile =
        toml_edit::de::from_str(text).map_err(|error| format!("invalid settings TOML: {error}"))?;
    let mut settings = Settings::default();
    if let Some(scroll_margin) = file.editor.scroll_margin {
        if scroll_margin > MAX_SCROLL_MARGIN {
            return Err(format!(
                "editor.scroll_margin must be 0..={MAX_SCROLL_MARGIN}, got {scroll_margin}"
            ));
        }
        settings.editor.scroll_margin = scroll_margin;
    }
    if let Some(auto_save) = file.editor.auto_save {
        settings.editor.auto_save = auto_save;
    }
    if let Some(case_sensitive) = file.search.case_sensitive {
        settings.search.case_sensitive = case_sensitive;
    }
    if let Some(inlay_hints) = file.lsp.inlay_hints {
        settings.lsp.inlay_hints = inlay_hints;
    }
    Ok(settings)
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
    if let Some(value) = get(ENV_AUTO_SAVE) {
        settings.editor.auto_save = parse_bool(ENV_AUTO_SAVE, &value)?;
    }
    if let Some(value) = get(ENV_SEARCH_CASE_SENSITIVE) {
        settings.search.case_sensitive = parse_bool(ENV_SEARCH_CASE_SENSITIVE, &value)?;
    }
    if let Some(value) = get(ENV_INLAY_HINTS) {
        settings.lsp.inlay_hints = parse_bool(ENV_INLAY_HINTS, &value)?;
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

pub(crate) fn path() -> Result<PathBuf, String> {
    if let Some(explicit) = std::env::var_os("ANVIL_SETTINGS") {
        let explicit = explicit.to_string_lossy();
        if !explicit.trim().is_empty() {
            return Ok(PathBuf::from(explicit.into_owned()));
        }
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
    editor: EditorFile,
    search: SearchFile,
    lsp: LspFile,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct EditorFile {
    scroll_margin: Option<usize>,
    auto_save: Option<bool>,
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
}

impl crate::Model {
    pub(crate) fn apply_settings(&mut self, settings: Settings) {
        self.settings = settings;
        self.search.options.case_sensitive = settings.search.case_sensitive;
        self.inlay_hints.clear();
        self.inlay_pending = None;
        self.inlay_checked = None;
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_explicit() {
        assert_eq!(
            Settings::default(),
            Settings {
                editor: EditorSettings {
                    scroll_margin: 3,
                    auto_save: false,
                },
                search: SearchSettings {
                    case_sensitive: false,
                },
                lsp: LspSettings { inlay_hints: true },
            }
        );
    }

    #[test]
    fn parses_all_domains_and_ignores_theme() {
        let settings = parse(
            "[theme]\nmode = \"fixed\"\n[editor]\nscroll_margin = 1\nauto_save = true\n[search]\ncase_sensitive = true\n[lsp]\ninlay_hints = false\n",
        )
        .expect("settings");
        assert_eq!(settings.editor.scroll_margin, 1);
        assert!(settings.editor.auto_save);
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
    fn save_preserves_theme_and_writes_domains() {
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
        let mut settings = Settings::default();
        settings.editor.scroll_margin = 5;
        settings.search.case_sensitive = true;
        settings.lsp.inlay_hints = false;
        save_to_path(&path, settings).expect("save");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("[theme]"));
        assert!(text.contains("name = \"nord\""));
        assert_eq!(load_file_from_path(&path).expect("load"), settings);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn rejects_unknown_sections_fields_and_values() {
        assert!(parse("[ui]\nmode = \"x\"\n").is_err());
        assert!(parse("[editor]\ntab_width = 2\n").is_err());
        assert!(parse("[editor]\nscroll_margin = 99\n").is_err());
        assert!(parse("[search]\ncase_sensitive = \"yes\"\n").is_err());
    }

    #[test]
    fn environment_overrides_file_values() {
        let settings = apply_env(
            parse("[editor]\nscroll_margin = 1\n[search]\ncase_sensitive = true\n[lsp]\ninlay_hints = false\n")
                .expect("file"),
            |name| match name {
                ENV_SCROLL_MARGIN => Some("4".to_owned()),
                ENV_SEARCH_CASE_SENSITIVE => Some("false".to_owned()),
                ENV_INLAY_HINTS => Some("true".to_owned()),
                _ => None,
            },
        )
        .expect("env");
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
        assert!(model.search.options.case_sensitive);
        assert!(!model.settings.lsp.inlay_hints);
    }
}

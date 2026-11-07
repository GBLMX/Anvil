//! Persisted theme settings.
//!
//! The first settings slice only owns `[theme]`; other sections can be added
//! without changing the path or the atomic write.

use crate::theme::{self, Choice};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 64 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsFile {
    #[serde(default)]
    theme: Option<ThemeFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    dark: Option<String>,
    #[serde(default)]
    light: Option<String>,
}

pub(crate) fn load() -> Result<(), String> {
    let path = path()?;
    load_from_path(&path)
}

pub(crate) fn load_from_path(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let metadata =
        fs::metadata(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if metadata.len() > MAX_BYTES {
        return Err(format!("{} exceeds {} bytes", path.display(), MAX_BYTES));
    }
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let choice = parse(&text)?;
    theme::apply_choice(choice, false)
}

pub(crate) fn save() -> Result<PathBuf, String> {
    let choice = theme::runtime_choice()
        .ok_or_else(|| "no runtime theme to save; use :theme or :theme-auto first".to_owned())?;
    let path = path()?;
    save_to_path(&path, choice)?;
    Ok(path)
}

pub(crate) fn save_to_path(path: &Path, choice: Choice) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let text = match choice {
        Choice::Fixed(name) => {
            format!("[theme]\nmode = \"fixed\"\nname = \"{}\"\n", escape(name))
        }
        Choice::Auto { dark, light } => format!(
            "[theme]\nmode = \"auto\"\ndark = \"{}\"\nlight = \"{}\"\n",
            escape(dark),
            escape(light)
        ),
    };
    let temporary = path.with_extension(format!("toml.tmp-{}", std::process::id()));
    fs::write(&temporary, text)
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("cannot replace {}: {error}", path.display()));
    }
    Ok(())
}

fn path() -> Result<PathBuf, String> {
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

fn parse(text: &str) -> Result<Choice, String> {
    let file: SettingsFile =
        toml_edit::de::from_str(text).map_err(|error| format!("invalid settings TOML: {error}"))?;
    let Some(theme) = file.theme else {
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

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixed_and_auto_modes() {
        assert_eq!(
            parse("[theme]\nmode = \"fixed\"\nname = \"nord\"\n").expect("fixed"),
            Choice::Fixed("nord")
        );
        assert_eq!(
            parse("[theme]\nmode = \"auto\"\ndark = \"nord\"\nlight = \"solarized-light\"\n")
                .expect("auto"),
            Choice::Auto {
                dark: "nord",
                light: "solarized-light",
            }
        );
    }

    #[test]
    fn rejects_ambiguous_or_unknown_theme_config() {
        assert!(parse("[theme]\nmode = \"auto\"\nname = \"nord\"\n").is_err());
        assert!(parse("[theme]\ndark = \"nord\"\n").is_err());
        assert!(parse("[theme]\nname = \"missing\"\n").is_err());
        assert!(parse("[theme]\nmode = \"system\"\n").is_err());
    }

    #[test]
    fn saves_and_loads_theme_choice() {
        let directory = std::env::temp_dir().join(format!("anvil-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let path = directory.join("settings.toml");
        save_to_path(&path, Choice::Fixed("nord")).expect("save");
        assert!(
            fs::read_to_string(&path)
                .expect("read")
                .contains("name = \"nord\"")
        );
        save_to_path(
            &path,
            Choice::Auto {
                dark: "nord",
                light: "solarized-light",
            },
        )
        .expect("save auto");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("mode = \"auto\""));
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn loads_fixed_theme_settings() {
        let directory =
            std::env::temp_dir().join(format!("anvil-theme-load-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let path = directory.join("settings.toml");
        save_to_path(&path, Choice::Fixed("gruvbox-dark")).expect("save");
        load_from_path(&path).expect("load");
        assert_eq!(theme::name(), "gruvbox-dark");
        theme::set_startup_theme("anvil-dark").expect("reset");
        fs::remove_dir_all(directory).expect("cleanup");
    }
}

//! Persisted theme settings.
//!
//! The first settings slice only owns `[theme]`; other sections can be added
//! without changing the path or the atomic write.

use crate::Model;
use crate::settings;
use crate::theme::{self, Choice};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
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
    let path = settings::path()?;
    load_from_path(&path)
}

pub(crate) fn load_from_path(path: &Path) -> Result<(), String> {
    if let Some(choice) = read_choice_from_path(path)? {
        theme::apply_choice(choice, false)?;
    }
    Ok(())
}

pub(crate) fn read_choice() -> Result<Option<Choice>, String> {
    read_choice_from_path(&settings::path()?)
}

pub(crate) fn read_choice_from_path(path: &Path) -> Result<Option<Choice>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata =
        fs::metadata(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if metadata.len() > settings::MAX_BYTES {
        return Err(format!(
            "{} exceeds {} bytes",
            path.display(),
            settings::MAX_BYTES
        ));
    }
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    parse(&text).map(Some)
}

pub(crate) fn save() -> Result<PathBuf, String> {
    let choice = theme::runtime_choice()
        .ok_or_else(|| "no runtime theme to save; use :theme or :theme-auto first".to_owned())?;
    let path = settings::path()?;
    save_to_path(&path, choice)?;
    Ok(path)
}

pub(crate) fn save_to_path(path: &Path, choice: Choice) -> Result<(), String> {
    let mut document = if path.exists() {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let _ = parse(&text)?;
        text.parse::<toml_edit::DocumentMut>()
            .map_err(|error| format!("invalid settings TOML: {error}"))?
    } else {
        toml_edit::DocumentMut::new()
    };
    let theme = if document.contains_key("theme") {
        document["theme"]
            .as_table_mut()
            .ok_or_else(|| "[theme] must be a table".to_owned())?
    } else {
        document.insert("theme", toml_edit::Item::Table(toml_edit::Table::new()));
        document["theme"]
            .as_table_mut()
            .expect("inserted theme table")
    };
    theme.remove("mode");
    theme.remove("name");
    theme.remove("dark");
    theme.remove("light");
    match choice {
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

impl Model {
    pub(crate) fn persist_theme_status(&self, status: String) -> String {
        if !self.appearance.persist_theme {
            return status;
        }
        match save() {
            Ok(_) => format!("{status} (saved)"),
            Err(error) => format!("{status}; E: {error}"),
        }
    }
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
        assert_eq!(
            parse("[theme]\nmode = \"fixed\"\nname = \"nord\"\n[editor]\nscroll_margin = 1\n")
                .expect("other sections"),
            Choice::Fixed("nord")
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
    fn preserves_other_settings_sections() {
        let directory =
            std::env::temp_dir().join(format!("anvil-theme-preserve-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let path = directory.join("settings.toml");
        fs::create_dir_all(&directory).expect("mkdir");
        fs::write(
            &path,
            "[editor]\nscroll_margin = 3\n\n[ui]\nreduced_motion = true\n",
        )
        .expect("write");
        save_to_path(&path, Choice::Fixed("nord")).expect("save");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("scroll_margin = 3"), "{text}");
        assert!(text.contains("reduced_motion = true"), "{text}");
        assert!(text.contains("name = \"nord\""), "{text}");
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

//! Compact file icons derived from names and extensions.

use crate::theme;
use ratatui::style::Color;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use unicode_width::UnicodeWidthStr;

pub(crate) const MAX_ICON_CONFIG_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum IconKind {
    Rust,
    Go,
    Python,
    JavaScript,
    TypeScript,
    C,
    Cpp,
    Markdown,
    Json,
    Toml,
    Shell,
    Lock,
    Makefile,
    FolderClosed,
    FolderOpen,
    DefaultFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Icon {
    pub(crate) kind: IconKind,
    pub(crate) color: fn() -> Color,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct IconTheme {
    overrides: HashMap<IconKind, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AnvilFile {
    ui: UiFile,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UiFile {
    icons: HashMap<String, String>,
}

struct IconRule {
    name: &'static str,
    icon: Icon,
}

const RULES: &[IconRule] = &[
    IconRule {
        name: "Cargo.toml",
        icon: Icon {
            kind: IconKind::Rust,
            color: theme::syntax_type,
        },
    },
    IconRule {
        name: "Cargo.lock",
        icon: Icon {
            kind: IconKind::Lock,
            color: theme::syntax_constant,
        },
    },
    IconRule {
        name: "Makefile",
        icon: Icon {
            kind: IconKind::Makefile,
            color: theme::syntax_property,
        },
    },
];

const EXTENSIONS: &[(&str, Icon)] = &[
    (
        "rs",
        Icon {
            kind: IconKind::Rust,
            color: theme::syntax_type,
        },
    ),
    (
        "go",
        Icon {
            kind: IconKind::Go,
            color: theme::syntax_type,
        },
    ),
    (
        "py",
        Icon {
            kind: IconKind::Python,
            color: theme::syntax_keyword,
        },
    ),
    (
        "js",
        Icon {
            kind: IconKind::JavaScript,
            color: theme::syntax_constant,
        },
    ),
    (
        "jsx",
        Icon {
            kind: IconKind::JavaScript,
            color: theme::syntax_constant,
        },
    ),
    (
        "ts",
        Icon {
            kind: IconKind::TypeScript,
            color: theme::syntax_link,
        },
    ),
    (
        "tsx",
        Icon {
            kind: IconKind::TypeScript,
            color: theme::syntax_link,
        },
    ),
    (
        "c",
        Icon {
            kind: IconKind::C,
            color: theme::syntax_type,
        },
    ),
    (
        "h",
        Icon {
            kind: IconKind::C,
            color: theme::syntax_type,
        },
    ),
    (
        "cpp",
        Icon {
            kind: IconKind::Cpp,
            color: theme::syntax_type,
        },
    ),
    (
        "md",
        Icon {
            kind: IconKind::Markdown,
            color: theme::syntax_link,
        },
    ),
    (
        "json",
        Icon {
            kind: IconKind::Json,
            color: theme::syntax_constant,
        },
    ),
    (
        "jsonc",
        Icon {
            kind: IconKind::Json,
            color: theme::syntax_constant,
        },
    ),
    (
        "toml",
        Icon {
            kind: IconKind::Toml,
            color: theme::syntax_property,
        },
    ),
    (
        "sh",
        Icon {
            kind: IconKind::Shell,
            color: theme::syntax_function,
        },
    ),
    (
        "zsh",
        Icon {
            kind: IconKind::Shell,
            color: theme::syntax_function,
        },
    ),
    (
        "fish",
        Icon {
            kind: IconKind::Shell,
            color: theme::syntax_function,
        },
    ),
    (
        "bash",
        Icon {
            kind: IconKind::Shell,
            color: theme::syntax_function,
        },
    ),
];

pub(crate) fn icon_for(path: &str) -> Option<Icon> {
    let name = path.rsplit('/').next().unwrap_or(path);
    if let Some(rule) = RULES.iter().find(|rule| rule.name == name) {
        return Some(rule.icon);
    }
    let extension = name.rsplit_once('.')?.1;
    EXTENSIONS
        .iter()
        .find(|(candidate, _)| extension.eq_ignore_ascii_case(candidate))
        .map(|(_, icon)| *icon)
}

pub(crate) fn default_file_icon() -> Icon {
    Icon {
        kind: IconKind::DefaultFile,
        color: theme::text_color,
    }
}

pub(crate) fn folder_icon(expanded: bool) -> Icon {
    Icon {
        kind: if expanded {
            IconKind::FolderOpen
        } else {
            IconKind::FolderClosed
        },
        color: theme::accent,
    }
}

pub(crate) fn nerd_glyph(kind: IconKind) -> &'static str {
    match kind {
        IconKind::Rust => "\u{e7a8}",
        IconKind::Go => "\u{e627}",
        IconKind::Python => "\u{e73c}",
        IconKind::JavaScript => "\u{e74e}",
        IconKind::TypeScript => "\u{e628}",
        IconKind::C => "\u{e61e}",
        IconKind::Cpp => "\u{e61d}",
        IconKind::Markdown => "\u{e73e}",
        IconKind::Json => "\u{e60b}",
        IconKind::Toml => "\u{e6b2}",
        IconKind::Shell => "\u{f489}",
        IconKind::Lock => "\u{f023}",
        IconKind::Makefile => "\u{e673}",
        IconKind::FolderClosed => "\u{e5ff}",
        IconKind::FolderOpen => "\u{e5fd}",
        IconKind::DefaultFile => "\u{f15b}",
    }
}

pub(crate) fn ascii_glyph(kind: IconKind) -> &'static str {
    match kind {
        IconKind::Rust => "[rs]",
        IconKind::Go => "[go]",
        IconKind::Python => "[py]",
        IconKind::JavaScript => "[js]",
        IconKind::TypeScript => "[ts]",
        IconKind::C => "[c]",
        IconKind::Cpp => "[cpp]",
        IconKind::Markdown => "[md]",
        IconKind::Json => "[{}]",
        IconKind::Toml => "[toml]",
        IconKind::Shell => "[sh]",
        IconKind::Lock => "[lock]",
        IconKind::Makefile => "[mk]",
        IconKind::FolderClosed => "[-]",
        IconKind::FolderOpen => "[+]",
        IconKind::DefaultFile => "[ ]",
    }
}

impl IconTheme {
    pub(crate) fn load(workspace: &Path) -> Self {
        let Ok(metadata) = fs::metadata(workspace.join(".anvil.toml")) else {
            return Self::default();
        };
        if metadata.len() > MAX_ICON_CONFIG_BYTES {
            return Self::default();
        }
        let Ok(text) = fs::read_to_string(workspace.join(".anvil.toml")) else {
            return Self::default();
        };
        let Ok(file) = toml_edit::de::from_str::<AnvilFile>(&text) else {
            return Self::default();
        };
        let mut overrides = HashMap::new();
        for (key, value) in file.ui.icons {
            let Some(kind) = icon_kind(&key) else {
                continue;
            };
            let width = UnicodeWidthStr::width(value.as_str());
            if width == 0 {
                continue;
            }
            overrides.insert(kind, value);
        }
        Self { overrides }
    }

    pub(crate) fn glyph(&self, icon: Icon, ascii: bool) -> &str {
        if ascii {
            return ascii_glyph(icon.kind);
        }
        self.overrides
            .get(&icon.kind)
            .map(String::as_str)
            .unwrap_or_else(|| nerd_glyph(icon.kind))
    }

    pub(crate) fn icon_text(&self, icon: Icon, ascii: bool) -> Option<String> {
        let glyph = self.glyph(icon, ascii);
        if ascii {
            return Some(format!("{glyph} "));
        }
        match UnicodeWidthStr::width(glyph) {
            0 => None,
            1 => Some(format!("{glyph} ")),
            _ => Some(glyph.to_owned()),
        }
    }
}

fn icon_kind(key: &str) -> Option<IconKind> {
    Some(match key {
        "rust" => IconKind::Rust,
        "go" => IconKind::Go,
        "python" => IconKind::Python,
        "javascript" => IconKind::JavaScript,
        "typescript" => IconKind::TypeScript,
        "c" => IconKind::C,
        "cpp" => IconKind::Cpp,
        "markdown" => IconKind::Markdown,
        "json" => IconKind::Json,
        "toml" => IconKind::Toml,
        "shell" => IconKind::Shell,
        "lock" => IconKind::Lock,
        "makefile" => IconKind::Makefile,
        "folder_closed" => IconKind::FolderClosed,
        "folder_open" => IconKind::FolderOpen,
        "default_file" => IconKind::DefaultFile,
        _ => return None,
    })
}

#[cfg(test)]
pub(crate) fn icon_text_with(icon: Icon, ascii: bool) -> Option<String> {
    IconTheme::default().icon_text(icon, ascii)
}

pub(crate) fn ascii_mode() -> bool {
    std::env::var_os("ANVIL_ASCII_MODE").is_some_and(|value| value == "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_cover_extensions_and_fall_back_to_text() {
        assert_eq!(icon_for("src/main.rs").expect("rust").kind, IconKind::Rust);
        assert_eq!(icon_for("Cargo.toml").expect("cargo").kind, IconKind::Rust);
        assert_eq!(icon_for("Cargo.lock").expect("lock").kind, IconKind::Lock);
        assert_eq!(icon_for("Makefile").expect("make").kind, IconKind::Makefile);
        assert_eq!(icon_for("README.unknown"), None);
        assert_eq!(default_file_icon().kind, IconKind::DefaultFile);
    }

    #[test]
    fn icon_cells_respect_display_width() {
        let theme = IconTheme::default();
        let single = theme
            .icon_text(
                Icon {
                    kind: IconKind::Rust,
                    color: theme::text_color,
                },
                false,
            )
            .expect("single width");
        assert_eq!(UnicodeWidthStr::width(single.as_str()), 2);

        let wide = theme
            .icon_text(
                Icon {
                    kind: IconKind::Rust,
                    color: theme::text_color,
                },
                true,
            )
            .expect("ascii width");
        assert_eq!(UnicodeWidthStr::width(wide.as_str()), 5);

        let override_theme = IconTheme {
            overrides: HashMap::from([(IconKind::DefaultFile, "界".to_owned())]),
        };
        let wide = override_theme
            .icon_text(default_file_icon(), false)
            .expect("override width");
        assert_eq!(UnicodeWidthStr::width(wide.as_str()), 2);
    }

    #[test]
    fn ascii_fallback_uses_text_labels() {
        let icon = folder_icon(false);
        assert_eq!(icon_text_with(icon, true).as_deref(), Some("[-] "));
        assert_eq!(nerd_glyph(IconKind::FolderOpen), "\u{e5fd}");
    }

    #[test]
    fn config_overrides_are_loaded_and_empty_values_are_ignored() {
        let directory = tempfile::tempdir().expect("tempdir");
        fs::write(
            directory.path().join(".anvil.toml"),
            "[ui.icons]\nrust = \"🦀\"\ndefault_file = \"📄\"\ntoml = \"\"\n",
        )
        .expect("write");
        let theme = IconTheme::load(directory.path());
        assert_eq!(theme.glyph(icon_for("x.rs").expect("rust"), false), "🦀");
        assert_eq!(theme.glyph(default_file_icon(), false), "📄");
        assert_eq!(
            theme.glyph(icon_for("x.toml").expect("toml"), false),
            "\u{e6b2}"
        );
    }
}

//! Compact file icons derived from names and extensions.

use crate::theme;
use ratatui::style::Color;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Icon {
    pub(crate) glyph: &'static str,
    pub(crate) color: Color,
}

struct IconRule {
    name: &'static str,
    icon: Icon,
}

const RULES: &[IconRule] = &[
    IconRule {
        name: "Cargo.toml",
        icon: Icon {
            glyph: "R",
            color: theme::SYNTAX_TYPE,
        },
    },
    IconRule {
        name: "Cargo.lock",
        icon: Icon {
            glyph: "L",
            color: theme::SYNTAX_CONSTANT,
        },
    },
    IconRule {
        name: "Dockerfile",
        icon: Icon {
            glyph: "D",
            color: theme::ACCENT,
        },
    },
    IconRule {
        name: "Makefile",
        icon: Icon {
            glyph: "M",
            color: theme::SYNTAX_PROPERTY,
        },
    },
    IconRule {
        name: "LICENSE",
        icon: Icon {
            glyph: "L",
            color: theme::WARNING,
        },
    },
];

const EXTENSIONS: &[(&str, Icon)] = &[
    (
        "rs",
        Icon {
            glyph: "R",
            color: theme::SYNTAX_TYPE,
        },
    ),
    (
        "toml",
        Icon {
            glyph: "T",
            color: theme::SYNTAX_PROPERTY,
        },
    ),
    (
        "md",
        Icon {
            glyph: "M",
            color: theme::SYNTAX_LINK,
        },
    ),
    (
        "json",
        Icon {
            glyph: "J",
            color: theme::SYNTAX_CONSTANT,
        },
    ),
    (
        "yaml",
        Icon {
            glyph: "Y",
            color: theme::SYNTAX_CONSTANT,
        },
    ),
    (
        "yml",
        Icon {
            glyph: "Y",
            color: theme::SYNTAX_CONSTANT,
        },
    ),
    (
        "sh",
        Icon {
            glyph: "$",
            color: theme::SYNTAX_FUNCTION,
        },
    ),
    (
        "py",
        Icon {
            glyph: "P",
            color: theme::SYNTAX_KEYWORD,
        },
    ),
    (
        "js",
        Icon {
            glyph: "J",
            color: theme::SYNTAX_CONSTANT,
        },
    ),
    (
        "jsx",
        Icon {
            glyph: "J",
            color: theme::SYNTAX_CONSTANT,
        },
    ),
    (
        "ts",
        Icon {
            glyph: "T",
            color: theme::SYNTAX_LINK,
        },
    ),
    (
        "tsx",
        Icon {
            glyph: "T",
            color: theme::SYNTAX_LINK,
        },
    ),
    (
        "html",
        Icon {
            glyph: "<",
            color: theme::SYNTAX_PROPERTY,
        },
    ),
    (
        "css",
        Icon {
            glyph: "#",
            color: theme::SYNTAX_LINK,
        },
    ),
    (
        "lock",
        Icon {
            glyph: "L",
            color: theme::SYNTAX_CONSTANT,
        },
    ),
];

pub(crate) fn icon_for(path: &str) -> Option<Icon> {
    let name = path.rsplit('/').next().unwrap_or(path);
    if let Some(rule) = RULES.iter().find(|rule| rule.name == name) {
        return Some(rule.icon);
    }
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    EXTENSIONS
        .iter()
        .find(|(candidate, _)| *candidate == extension)
        .map(|(_, icon)| *icon)
}

pub(crate) fn icon_text(icon: Icon) -> Option<String> {
    icon_text_with(icon, ascii_mode())
}

pub(crate) fn icon_text_with(icon: Icon, ascii: bool) -> Option<String> {
    if ascii {
        return Some(format!("[{}] ", icon.glyph));
    }
    match UnicodeWidthStr::width(icon.glyph) {
        0 | 3.. => None,
        1 => Some(format!("{} ", icon.glyph)),
        _ => Some(icon.glyph.to_owned()),
    }
}

pub(crate) fn ascii_mode() -> bool {
    std::env::var_os("ANVIL_ASCII_MODE").is_some_and(|value| value == "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_cover_extensions_and_fall_back_to_text() {
        assert_eq!(icon_for("src/main.rs").expect("rust").glyph, "R");
        assert_eq!(icon_for("Cargo.toml").expect("cargo").glyph, "R");
        assert_eq!(icon_for("README.unknown"), None);
        assert_eq!(icon_for("no_extension"), None);
    }

    #[test]
    fn icon_cells_respect_display_width() {
        let single = icon_text(Icon {
            glyph: "R",
            color: theme::TEXT,
        })
        .expect("single width");
        assert_eq!(UnicodeWidthStr::width(single.as_str()), 2);

        let wide = icon_text(Icon {
            glyph: "界",
            color: theme::TEXT,
        })
        .expect("double width");
        assert_eq!(UnicodeWidthStr::width(wide.as_str()), 2);

        let ambiguous = icon_text(Icon {
            glyph: "·",
            color: theme::TEXT,
        })
        .expect("ambiguous width");
        assert!(UnicodeWidthStr::width(ambiguous.as_str()) <= 2);

        assert!(
            icon_text(Icon {
                glyph: "abcd",
                color: theme::TEXT,
            })
            .is_none()
        );
    }

    #[test]
    fn ascii_fallback_uses_text_labels() {
        let icon = Icon {
            glyph: "R",
            color: theme::TEXT,
        };
        assert_eq!(icon_text_with(icon, true).as_deref(), Some("[R] "));
    }
}

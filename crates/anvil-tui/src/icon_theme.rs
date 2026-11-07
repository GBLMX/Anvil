//! Compact file icons derived from names and extensions.

use crate::theme;
use ratatui::style::Color;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Icon {
    pub(crate) glyph: &'static str,
    pub(crate) color: fn() -> Color,
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
            color: theme::syntax_type,
        },
    },
    IconRule {
        name: "Cargo.lock",
        icon: Icon {
            glyph: "L",
            color: theme::syntax_constant,
        },
    },
    IconRule {
        name: "Dockerfile",
        icon: Icon {
            glyph: "D",
            color: theme::accent,
        },
    },
    IconRule {
        name: "Makefile",
        icon: Icon {
            glyph: "M",
            color: theme::syntax_property,
        },
    },
    IconRule {
        name: "LICENSE",
        icon: Icon {
            glyph: "L",
            color: theme::warning,
        },
    },
];

const EXTENSIONS: &[(&str, Icon)] = &[
    (
        "rs",
        Icon {
            glyph: "R",
            color: theme::syntax_type,
        },
    ),
    (
        "toml",
        Icon {
            glyph: "T",
            color: theme::syntax_property,
        },
    ),
    (
        "md",
        Icon {
            glyph: "M",
            color: theme::syntax_link,
        },
    ),
    (
        "json",
        Icon {
            glyph: "J",
            color: theme::syntax_constant,
        },
    ),
    (
        "yaml",
        Icon {
            glyph: "Y",
            color: theme::syntax_constant,
        },
    ),
    (
        "yml",
        Icon {
            glyph: "Y",
            color: theme::syntax_constant,
        },
    ),
    (
        "sh",
        Icon {
            glyph: "$",
            color: theme::syntax_function,
        },
    ),
    (
        "py",
        Icon {
            glyph: "P",
            color: theme::syntax_keyword,
        },
    ),
    (
        "js",
        Icon {
            glyph: "J",
            color: theme::syntax_constant,
        },
    ),
    (
        "jsx",
        Icon {
            glyph: "J",
            color: theme::syntax_constant,
        },
    ),
    (
        "ts",
        Icon {
            glyph: "T",
            color: theme::syntax_link,
        },
    ),
    (
        "tsx",
        Icon {
            glyph: "T",
            color: theme::syntax_link,
        },
    ),
    (
        "html",
        Icon {
            glyph: "<",
            color: theme::syntax_property,
        },
    ),
    (
        "css",
        Icon {
            glyph: "#",
            color: theme::syntax_link,
        },
    ),
    (
        "lock",
        Icon {
            glyph: "L",
            color: theme::syntax_constant,
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
            color: theme::text_color,
        })
        .expect("single width");
        assert_eq!(UnicodeWidthStr::width(single.as_str()), 2);

        let wide = icon_text(Icon {
            glyph: "界",
            color: theme::text_color,
        })
        .expect("double width");
        assert_eq!(UnicodeWidthStr::width(wide.as_str()), 2);

        let ambiguous = icon_text(Icon {
            glyph: "·",
            color: theme::text_color,
        })
        .expect("ambiguous width");
        assert!(UnicodeWidthStr::width(ambiguous.as_str()) <= 2);

        assert!(
            icon_text(Icon {
                glyph: "abcd",
                color: theme::text_color,
            })
            .is_none()
        );
    }

    #[test]
    fn ascii_fallback_uses_text_labels() {
        let icon = Icon {
            glyph: "R",
            color: theme::text_color,
        };
        assert_eq!(icon_text_with(icon, true).as_deref(), Some("[R] "));
    }
}

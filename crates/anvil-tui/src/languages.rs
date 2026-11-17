//! Read-only language readiness surface.

use super::*;

const LANGUAGES: &[(&str, &[&str])] = &[
    ("Rust", &["rs"]),
    ("Go", &["go"]),
    ("Python", &["py", "pyi"]),
    ("TypeScript", &["ts", "mts", "cts"]),
    ("TSX", &["tsx"]),
    ("JavaScript", &["js", "mjs", "cjs"]),
    ("JSX", &["jsx"]),
    ("C", &["c", "h"]),
    ("C++", &["cc", "cpp", "cxx", "hpp", "hh", "hxx"]),
    ("Java", &["java"]),
    ("Lua", &["lua"]),
    ("JSON", &["json", "jsonc"]),
    ("YAML", &["yaml", "yml"]),
    ("Shell", &["sh", "bash", "zsh"]),
    ("Markdown", &["md", "markdown"]),
    ("TOML", &["toml"]),
    ("HTML", &["html", "htm"]),
    ("CSS", &["css"]),
    ("SCSS", &["scss"]),
    ("LESS", &["less"]),
    ("Zig", &["zig"]),
    ("LaTeX", &["tex", "ltx", "sty", "cls"]),
    ("Typst", &["typ"]),
];

impl Model {
    pub(crate) fn show_languages(&mut self) {
        let text = self.language_readiness_text();
        self.show_read_only(&text, "Languages: :q returns", "languages");
    }

    fn language_readiness_text(&self) -> String {
        let mut lines = vec![
            "Language readiness".to_owned(),
            format!("Workspace: {}", self.workspace.path.display()),
            String::new(),
        ];
        for (name, extensions) in LANGUAGES {
            lines.push((*name).to_owned());
            lines.push(format!("  extensions: {}", extensions.join(", ")));
            let syntax = extensions
                .iter()
                .any(|extension| anvil_syntax::supports_path(&probe_path(extension)));
            lines.push(format!(
                "  syntax: {}",
                if syntax {
                    "yes"
                } else {
                    "missing (grammar feature not compiled)"
                }
            ));
            match self.lsp_profile(extensions) {
                Some(settings) => {
                    let roots = if settings.root_markers.is_empty() {
                        "none".to_owned()
                    } else {
                        settings.root_markers.join(", ")
                    };
                    let available = if tool_on_path(&settings.tool) {
                        "found"
                    } else {
                        "not found on PATH"
                    };
                    lines.push(format!(
                        "  lsp: {} ({} / {}) · roots {} · server {available}",
                        settings.label, settings.tool, settings.language_id, roots
                    ));
                }
                None => lines.push("  lsp: no built-in profile".to_owned()),
            }
            let formatter = extensions
                .iter()
                .find_map(|extension| crate::format::formatter_language(&probe_path(extension)));
            lines.push(format!(
                "  formatter: {}",
                formatter.map_or_else(
                    || "LSP fallback".to_owned(),
                    |language| format!("external {language} (LSP fallback)")
                )
            ));
            lines.push("  linter: not configured".to_owned());
            lines.push("  tests/debug: not configured".to_owned());
            lines.push(String::new());
        }
        lines.push(
            "Nothing is installed or downloaded here; use anvil doctor for tool installation."
                .to_owned(),
        );
        lines.join("\n")
    }

    fn lsp_profile(&self, extensions: &[&str]) -> Option<anvil_lsp_client::LspSettings> {
        extensions.iter().find_map(|extension| {
            crate::lsp_ui::lsp_settings_with_root(
                &probe_path(extension),
                &self.workspace.path,
                self.workspace.language_tooling_service.as_deref(),
            )
        })
    }
}

fn probe_path(extension: &str) -> PathBuf {
    PathBuf::from(format!("probe.{extension}"))
}

fn tool_on_path(tool: &str) -> bool {
    let path = Path::new(tool);
    if path.components().count() > 1 {
        return path.is_file();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|directory| {
        directory.join(tool).is_file()
            || cfg!(windows) && directory.join(format!("{tool}.exe")).is_file()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EditorState;

    #[test]
    fn language_readiness_reports_core_languages() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.workspace.path = PathBuf::from("/ws");
        let text = model.language_readiness_text();
        assert!(text.contains("Rust"), "{text}");
        assert!(text.contains("syntax: yes"), "{text}");
        assert!(text.contains("lsp: rust-analyzer"), "{text}");
        assert!(text.contains("formatter: external rust"), "{text}");
        assert!(text.contains("linter: not configured"), "{text}");
        assert!(text.contains("TypeScript"), "{text}");
    }
}

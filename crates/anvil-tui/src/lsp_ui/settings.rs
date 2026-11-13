//! Language-server discovery and per-extension settings.

use std::path::{Path, PathBuf};

use anvil_module::language::{LanguageServerSpec, LanguageToolingService};

pub(crate) struct LspSettings {
    pub(crate) tool: String,
    pub(crate) args: Vec<String>,
    pub(crate) language_id: String,
    pub(crate) root_markers: Vec<String>,
    pub(crate) label: String,
    pub(crate) initialization_options: Option<serde_json::Value>,
}

impl From<LanguageServerSpec> for LspSettings {
    fn from(spec: LanguageServerSpec) -> Self {
        Self {
            tool: spec.tool,
            args: spec.args,
            language_id: spec.language_id,
            root_markers: spec.root_markers,
            label: spec.label,
            initialization_options: spec.initialization_options,
        }
    }
}

#[cfg(test)]
pub(crate) fn lsp_settings(path: &Path) -> Option<LspSettings> {
    lsp_settings_with_service(path, None)
}

pub(crate) fn lsp_settings_with_service(
    path: &Path,
    service: Option<&dyn LanguageToolingService>,
) -> Option<LspSettings> {
    service
        .and_then(|service| service.server_for(path))
        .map(LspSettings::from)
        .or_else(|| builtin_lsp_settings(path))
}

fn builtin_lsp_settings(path: &Path) -> Option<LspSettings> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let (candidates, args, language_id, root_markers, label, env) = match extension.as_str() {
        "rs" => (
            vec!["rust-analyzer"],
            Vec::new(),
            "rust",
            vec!["Cargo.toml"],
            "rust-analyzer",
            "ANVIL_LSP_RUST",
        ),
        "go" => (
            vec!["gopls"],
            Vec::new(),
            "go",
            vec!["go.work", "go.mod"],
            "gopls",
            "ANVIL_LSP_GO",
        ),
        "py" | "pyi" => (
            vec!["pyright-langserver", "basedpyright-langserver", "pylsp"],
            Vec::new(),
            "python",
            vec![
                "pyproject.toml",
                "setup.py",
                "setup.cfg",
                "requirements.txt",
            ],
            "python language server",
            "ANVIL_LSP_PYTHON",
        ),
        "ts" | "mts" | "cts" => (
            vec!["typescript-language-server", "vtsls"],
            vec!["--stdio".to_owned()],
            "typescript",
            vec!["tsconfig.json", "jsconfig.json", "package.json"],
            "typescript language server",
            "ANVIL_LSP_TYPESCRIPT",
        ),
        "tsx" => (
            vec!["typescript-language-server", "vtsls"],
            vec!["--stdio".to_owned()],
            "typescriptreact",
            vec!["tsconfig.json", "jsconfig.json", "package.json"],
            "typescript language server",
            "ANVIL_LSP_TYPESCRIPT",
        ),
        "js" | "mjs" | "cjs" => (
            vec!["typescript-language-server", "vtsls"],
            vec!["--stdio".to_owned()],
            "javascript",
            vec!["tsconfig.json", "jsconfig.json", "package.json"],
            "typescript language server",
            "ANVIL_LSP_TYPESCRIPT",
        ),
        "jsx" => (
            vec!["typescript-language-server", "vtsls"],
            vec!["--stdio".to_owned()],
            "javascriptreact",
            vec!["tsconfig.json", "jsconfig.json", "package.json"],
            "typescript language server",
            "ANVIL_LSP_TYPESCRIPT",
        ),
        "c" | "h" => (
            vec!["clangd"],
            Vec::new(),
            "c",
            vec![
                "compile_commands.json",
                "compile_flags.txt",
                ".clangd",
                "CMakeLists.txt",
                "Makefile",
            ],
            "clangd",
            "ANVIL_LSP_CLANGD",
        ),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => (
            vec!["clangd"],
            Vec::new(),
            "cpp",
            vec![
                "compile_commands.json",
                "compile_flags.txt",
                ".clangd",
                "CMakeLists.txt",
                "Makefile",
            ],
            "clangd",
            "ANVIL_LSP_CLANGD",
        ),
        "java" => (
            vec!["jdtls"],
            Vec::new(),
            "java",
            vec![
                "pom.xml",
                "build.gradle",
                "build.gradle.kts",
                "settings.gradle",
                "settings.gradle.kts",
                "gradlew",
            ],
            "jdtls",
            "ANVIL_LSP_JAVA",
        ),
        "lua" => (
            vec!["lua-language-server"],
            Vec::new(),
            "lua",
            vec![".luarc.json", ".luarc.jsonc", ".stylua.toml", "selene.toml"],
            "lua-language-server",
            "ANVIL_LSP_LUA",
        ),
        "json" => (
            vec!["vscode-json-language-server", "json-languageserver"],
            vec!["--stdio".to_owned()],
            "json",
            vec!["package.json", "tsconfig.json", "jsconfig.json"],
            "json language server",
            "ANVIL_LSP_JSON",
        ),
        "jsonc" => (
            vec!["vscode-json-language-server", "json-languageserver"],
            vec!["--stdio".to_owned()],
            "jsonc",
            vec!["package.json", "tsconfig.json", "jsconfig.json"],
            "json language server",
            "ANVIL_LSP_JSON",
        ),
        "yaml" | "yml" => (
            vec!["yaml-language-server"],
            vec!["--stdio".to_owned()],
            "yaml",
            vec![".yamllint", ".yamllint.yaml", ".yamllint.yml", "Chart.yaml"],
            "yaml-language-server",
            "ANVIL_LSP_YAML",
        ),
        "sh" | "bash" | "zsh" => (
            vec!["bash-language-server"],
            vec!["start".to_owned()],
            "shellscript",
            vec![".shellcheckrc", ".bashrc", ".zshrc"],
            "bash-language-server",
            "ANVIL_LSP_SHELL",
        ),
        "md" | "markdown" => (
            vec!["marksman"],
            vec!["server".to_owned()],
            "markdown",
            vec![".marksman.toml", ".git"],
            "marksman",
            "ANVIL_LSP_MARKDOWN",
        ),
        "toml" => (
            vec!["taplo"],
            vec!["lsp".to_owned(), "stdio".to_owned()],
            "toml",
            vec!["taplo.toml", ".taplo.toml", "Cargo.toml"],
            "taplo",
            "ANVIL_LSP_TOML",
        ),
        "html" | "htm" => (
            vec!["vscode-html-language-server"],
            vec!["--stdio".to_owned()],
            "html",
            vec!["package.json", "index.html"],
            "html language server",
            "ANVIL_LSP_HTML",
        ),
        "css" => (
            vec!["vscode-css-language-server"],
            vec!["--stdio".to_owned()],
            "css",
            vec!["package.json"],
            "css language server",
            "ANVIL_LSP_CSS",
        ),
        "scss" => (
            vec!["vscode-css-language-server"],
            vec!["--stdio".to_owned()],
            "scss",
            vec!["package.json"],
            "css language server",
            "ANVIL_LSP_CSS",
        ),
        "less" => (
            vec!["vscode-css-language-server"],
            vec!["--stdio".to_owned()],
            "less",
            vec!["package.json"],
            "css language server",
            "ANVIL_LSP_CSS",
        ),
        "zig" => (
            vec!["zls"],
            Vec::new(),
            "zig",
            vec!["build.zig", "build.zig.zon"],
            "zls",
            "ANVIL_LSP_ZIG",
        ),
        _ => return None,
    };
    let (tool, args) = std::env::var_os(env)
        .or_else(|| std::env::var_os("ANVIL_LSP"))
        .and_then(|path| {
            let path = path.to_string_lossy().into_owned();
            (!path.is_empty()).then_some(path)
        })
        .or_else(|| find_tool(&candidates))
        .map(|tool| {
            let args = if tool.ends_with("pyright-langserver") {
                vec!["--stdio".to_owned()]
            } else {
                args.clone()
            };
            (tool, args)
        })?;
    let initialization_options = tsserver_options(&tool, path);
    Some(LspSettings {
        tool,
        args,
        language_id: language_id.to_owned(),
        root_markers: root_markers.into_iter().map(str::to_owned).collect(),
        label: label.to_owned(),
        initialization_options,
    })
}

fn find_tool(candidates: &[&str]) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        for candidate in candidates {
            let path = directory.join(candidate);
            if path.is_file() {
                return Some(path.to_string_lossy().into_owned());
            }
        }
    }
    candidates.first().map(|candidate| (*candidate).to_owned())
}

fn tsserver_options(tool: &str, source: &Path) -> Option<serde_json::Value> {
    if !tool.ends_with("typescript-language-server") {
        return None;
    }
    let path = std::env::var_os("ANVIL_TSSERVER")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| find_project_tsserver(source))
        .or_else(find_tsserver)?;
    Some(serde_json::json!({"tsserver": {"path": path}}))
}

fn find_project_tsserver(source: &Path) -> Option<PathBuf> {
    let mut directory = source.parent()?;
    loop {
        let candidate = directory.join("node_modules/typescript/lib/tsserver.js");
        if candidate.is_file() {
            return Some(candidate);
        }
        directory = directory.parent()?;
    }
}
fn find_tsserver() -> Option<PathBuf> {
    let tsc = find_tool(&["tsc"])?;
    let tsc = PathBuf::from(tsc).canonicalize().ok()?;
    let candidate = tsc.parent()?.parent()?.join("lib/tsserver.js");
    candidate.is_file().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    struct FakeProvider;

    impl LanguageToolingService for FakeProvider {
        fn server_for(&self, path: &Path) -> Option<LanguageServerSpec> {
            (path.extension().and_then(|extension| extension.to_str()) == Some("nix")).then(|| {
                LanguageServerSpec {
                    tool: "nixd".to_owned(),
                    args: vec![],
                    language_id: "nix".to_owned(),
                    root_markers: vec!["flake.nix".to_owned()],
                    label: "nixd".to_owned(),
                    initialization_options: None,
                }
            })
        }
    }

    #[test]
    fn service_precedes_builtin_and_falls_back() {
        let provider = FakeProvider;
        let nix = lsp_settings_with_service(Path::new("flake.nix"), Some(&provider))
            .expect("module provider");
        assert_eq!(nix.tool, "nixd");
        assert_eq!(nix.language_id, "nix");

        let rust = lsp_settings_with_service(Path::new("src/main.rs"), Some(&provider))
            .expect("builtin fallback");
        assert_eq!(rust.language_id, "rust");
    }
}

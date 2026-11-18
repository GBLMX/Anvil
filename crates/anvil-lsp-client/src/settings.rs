//! Language-server discovery and per-extension settings.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anvil_module::language::{LanguageServerSpec, LanguageToolingService};

/// Resolved language-server launch profile.
#[derive(Debug, Clone)]
pub struct LspSettings {
    pub tool: String,
    pub args: Vec<String>,
    pub language_id: String,
    pub root_markers: Vec<String>,
    pub label: String,
    pub initialization_options: Option<serde_json::Value>,
    pub env: BTreeMap<String, String>,
    pub settings: serde_json::Value,
}

impl From<LanguageServerSpec> for LspSettings {
    fn from(spec: LanguageServerSpec) -> Self {
        Self {
            tool: spec.tool,
            args: spec.args,
            language_id: spec.language_id,
            root_markers: spec.root_markers,
            initialization_options: spec.initialization_options,
            label: spec.label,
            env: BTreeMap::new(),
            settings: serde_json::Value::Null,
        }
    }
}

/// Resolves the built-in launch profile for `path`.
pub fn lsp_settings(path: &Path) -> Option<LspSettings> {
    lsp_settings_with_service(path, None, |_, _| None)
}

/// Resolves a module provider first, then the built-in catalog.
///
/// `command_override` is consulted before executable discovery so a frontend
/// can apply user settings without owning the catalog.
pub fn lsp_settings_with_service<F>(
    path: &Path,
    service: Option<&dyn LanguageToolingService>,
    command_override: F,
) -> Option<LspSettings>
where
    F: Fn(&str, &[String]) -> Option<(String, Vec<String>)>,
{
    service
        .and_then(|service| service.server_for(path))
        .map(LspSettings::from)
        .or_else(|| builtin_lsp_settings(path, &command_override))
}

fn builtin_lsp_settings<F>(path: &Path, command_override: &F) -> Option<LspSettings>
where
    F: Fn(&str, &[String]) -> Option<(String, Vec<String>)>,
{
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
    let command = std::env::var_os(env)
        .or_else(|| std::env::var_os("ANVIL_LSP"))
        .and_then(|path| {
            let path = path.to_string_lossy().into_owned();
            (!path.is_empty()).then_some((path, args.clone()))
        })
        .or_else(|| command_override(language_id, &args))
        .or_else(|| find_tool(&candidates).map(|tool| (tool, args.clone())))?;
    let (tool, mut args) = command;
    if tool.ends_with("pyright-langserver") && args.is_empty() {
        args = vec!["--stdio".to_owned()];
    }
    let initialization_options = tsserver_options(&tool, path);
    Some(LspSettings {
        tool,
        args,
        language_id: language_id.to_owned(),
        root_markers: root_markers.into_iter().map(str::to_owned).collect(),
        label: label.to_owned(),
        initialization_options,
        env: BTreeMap::new(),
        settings: serde_json::Value::Null,
    })
}

/// Finds the first executable candidate in the shared tool search path.
pub fn find_tool_path(candidates: &[&str]) -> Option<PathBuf> {
    find_tool_in(candidates, &tool_search_directories())
}

fn find_tool(candidates: &[&str]) -> Option<String> {
    find_tool_path(candidates)
        .map(|path| path.to_string_lossy().into_owned())
        .or_else(|| candidates.first().map(|candidate| (*candidate).to_owned()))
}

fn tool_search_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        directories.extend(std::env::split_paths(&path));
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        directories.push(parent.to_path_buf());
    }
    if let Some(home) = std::env::var_os("HOME") {
        directories.extend(user_tool_directories(Path::new(&home)));
    }
    directories
}

fn user_tool_directories(home: &Path) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    for variable in ["MISE_DATA_DIR", "ASDF_DATA_DIR"] {
        if let Some(data_dir) = std::env::var_os(variable)
            && !data_dir.is_empty()
        {
            directories.push(PathBuf::from(data_dir).join("shims"));
        }
    }
    directories.extend(
        [
            ".local/bin",
            "bin",
            ".cargo/bin",
            "go/bin",
            ".local/share/vite-plus/bin",
            ".local/share/mise/shims",
            ".asdf/shims",
            ".bun/bin",
        ]
        .into_iter()
        .map(|relative| home.join(relative)),
    );
    directories
}

fn find_tool_in(candidates: &[&str], directories: &[PathBuf]) -> Option<PathBuf> {
    for directory in directories {
        for candidate in candidates {
            let path = directory.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
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
    fn tool_search_falls_back_to_user_bins() {
        let directory = tempfile::tempdir().expect("tempdir");
        let tool = directory.path().join("marksman");
        std::fs::write(&tool, b"test").expect("write tool");
        let found = find_tool_in(&["marksman"], &[directory.path().to_path_buf()])
            .expect("tool in fallback directory");
        assert_eq!(found, tool);
    }

    #[test]
    fn user_tool_directories_include_common_installers() {
        let home = Path::new("/home/test");
        let directories = user_tool_directories(home);
        assert!(directories.contains(&home.join("go/bin")));
        assert!(directories.contains(&home.join(".local/share/vite-plus/bin")));
        assert!(directories.contains(&home.join(".local/share/mise/shims")));
        assert!(directories.contains(&home.join(".asdf/shims")));
    }

    #[test]
    fn user_tool_directories_include_data_dir_shims() {
        let home = Path::new("/home/test");
        let data_dir = tempfile::tempdir().expect("tempdir");
        unsafe { std::env::set_var("ASDF_DATA_DIR", data_dir.path()) };
        let directories = user_tool_directories(home);
        unsafe { std::env::remove_var("ASDF_DATA_DIR") };
        let custom = data_dir.path().join("shims");
        let default = home.join(".asdf/shims");
        let custom_index = directories
            .iter()
            .position(|path| path == &custom)
            .expect("custom shim directory");
        let default_index = directories
            .iter()
            .position(|path| path == &default)
            .expect("default shim directory");
        assert!(custom_index < default_index);
    }

    #[test]
    fn tool_search_includes_executable_parent() {
        let executable = std::env::current_exe().expect("current executable");
        let parent = executable.parent().expect("executable parent");
        assert!(tool_search_directories().contains(&parent.to_path_buf()));
    }

    #[test]
    fn service_precedes_builtin_and_falls_back() {
        let provider = FakeProvider;
        let nix = lsp_settings_with_service(Path::new("flake.nix"), Some(&provider), |_, _| None)
            .expect("module provider");
        assert_eq!(nix.tool, "nixd");
        assert_eq!(nix.language_id, "nix");

        let rust =
            lsp_settings_with_service(Path::new("src/main.rs"), Some(&provider), |_, _| None)
                .expect("builtin fallback");
        assert_eq!(rust.language_id, "rust");
    }

    #[test]
    fn builtin_catalog_includes_compiled_language_defaults() {
        let c = lsp_settings(Path::new("main.c")).expect("clangd");
        assert!(c.tool.ends_with("clangd"));
        assert_eq!(c.language_id, "c");
        let cpp = lsp_settings(Path::new("main.cpp")).expect("clangd cpp");
        assert!(cpp.tool.ends_with("clangd"));
        assert_eq!(cpp.language_id, "cpp");
        let java = lsp_settings(Path::new("Main.java")).expect("jdtls");
        assert!(java.tool.ends_with("jdtls"));
        assert_eq!(java.language_id, "java");
    }

    #[test]
    fn builtin_catalog_covers_supported_languages() {
        for (path, language) in [
            ("main.rs", "rust"),
            ("main.go", "go"),
            ("main.py", "python"),
            ("main.ts", "typescript"),
            ("main.tsx", "typescriptreact"),
            ("main.js", "javascript"),
            ("main.jsx", "javascriptreact"),
            ("main.c", "c"),
            ("main.cpp", "cpp"),
            ("main.h", "c"),
            ("main.java", "java"),
            ("main.lua", "lua"),
            ("main.json", "json"),
            ("main.jsonc", "jsonc"),
            ("main.yaml", "yaml"),
            ("main.yml", "yaml"),
            ("main.sh", "shellscript"),
            ("README.md", "markdown"),
            ("Cargo.toml", "toml"),
            ("index.html", "html"),
            ("style.css", "css"),
            ("style.scss", "scss"),
            ("main.zig", "zig"),
        ] {
            assert_eq!(
                lsp_settings(Path::new(path))
                    .unwrap_or_else(|| panic!("missing settings for {path}"))
                    .language_id,
                language,
                "{path}"
            );
        }
        assert_eq!(
            lsp_settings(Path::new("main.ts")).expect("typescript").args,
            vec!["--stdio"]
        );
        assert_eq!(
            lsp_settings(Path::new("main.sh")).expect("shell").args,
            vec!["start"]
        );
        assert_eq!(
            lsp_settings(Path::new("README.md")).expect("markdown").args,
            vec!["server"]
        );
        assert_eq!(
            lsp_settings(Path::new("Cargo.toml")).expect("toml").args,
            vec!["lsp", "stdio"]
        );
        assert!(lsp_settings(Path::new("README.txt")).is_none());
    }

    #[test]
    fn typescript_uses_configured_tsserver() {
        let directory = std::env::temp_dir().join(format!("anvil-ts-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("temp dir");
        let tsserver = directory.join("tsserver.js");
        std::fs::write(&tsserver, "").expect("tsserver");
        unsafe { std::env::set_var("ANVIL_TSSERVER", &tsserver) };
        let settings = lsp_settings(Path::new("main.ts")).expect("typescript");
        unsafe { std::env::remove_var("ANVIL_TSSERVER") };
        let project = directory.join("project");
        let project_tsserver = project.join("node_modules/typescript/lib/tsserver.js");
        std::fs::create_dir_all(project_tsserver.parent().expect("tsserver parent"))
            .expect("project tsserver dir");
        std::fs::write(&project_tsserver, "").expect("project tsserver");
        let source = project.join("src/main.ts");
        std::fs::create_dir_all(source.parent().expect("source parent")).expect("source dir");
        unsafe { std::env::set_var("ANVIL_LSP_TYPESCRIPT", "typescript-language-server") };
        let project_settings = lsp_settings(&source).expect("typescript");
        unsafe { std::env::remove_var("ANVIL_LSP_TYPESCRIPT") };
        let project_options = project_settings
            .initialization_options
            .expect("project tsserver options");
        assert_eq!(
            project_options["tsserver"]["path"],
            project_tsserver.to_string_lossy().as_ref()
        );
        let options = settings.initialization_options.expect("tsserver options");
        assert_eq!(
            options["tsserver"]["path"],
            tsserver.to_string_lossy().as_ref()
        );
        std::fs::remove_dir_all(directory).expect("cleanup");
    }
}

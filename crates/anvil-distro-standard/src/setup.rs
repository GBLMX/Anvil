//! Read-only language-tool setup discovery and planning.

use std::{env, path::Path, path::PathBuf};

use anvil_lsp_client::find_tool_path;
use anvil_module::language::LanguageToolingService;
use serde_json::json;

use crate::{RunError, args::SetupMode};

#[derive(Clone, Copy, Debug)]
enum Provider {
    Rustup(&'static str),
    Npm(&'static str),
    Go(&'static str),
    Pipx(&'static str),
    Cargo(&'static str),
    System(&'static str),
    Manual(&'static str),
}

impl Provider {
    fn label(self) -> &'static str {
        match self {
            Self::Rustup(_) => "rustup",
            Self::Npm(_) => "npm",
            Self::Go(_) => "go",
            Self::Pipx(_) => "pipx",
            Self::Cargo(_) => "cargo",
            Self::System(_) => "system",
            Self::Manual(_) => "manual",
        }
    }

    fn command(self) -> String {
        match self {
            Self::Rustup(component) => format!("rustup component add {component}"),
            Self::Npm(package) => format!("npm install -g {package}"),
            Self::Go(package) => format!("go install {package}"),
            Self::Pipx(package) => format!("pipx install {package}"),
            Self::Cargo(package) => format!("cargo install {package} --locked"),
            Self::System(package) => system_install_command(package),
            Self::Manual(hint) => hint.to_owned(),
        }
    }

    fn requires(self) -> &'static [&'static str] {
        match self {
            Self::Rustup(_) => &["rustup"],
            Self::Npm(_) => &["node", "npm"],
            Self::Go(_) => &["go"],
            Self::Pipx(_) => &["pipx"],
            Self::Cargo(_) => &["cargo"],
            Self::System(_) | Self::Manual(_) => &[],
        }
    }

    fn requires_root(self) -> bool {
        matches!(self, Self::System(_))
    }
}

struct LanguageStatus {
    name: &'static str,
    tool: Option<String>,
    label: Option<String>,
    path: Option<PathBuf>,
    provider: Option<Provider>,
}

impl LanguageStatus {
    fn state(&self) -> &'static str {
        match (&self.tool, &self.path) {
            (None, _) => "no_profile",
            (Some(_), Some(_)) => "found",
            (Some(_), None) => "missing",
        }
    }

    fn action(&self) -> &'static str {
        if self.state() == "missing" {
            "install"
        } else {
            "none"
        }
    }

    fn matches(&self, filter: &str) -> bool {
        self.name.eq_ignore_ascii_case(filter)
            || self
                .tool
                .as_deref()
                .is_some_and(|tool| tool.eq_ignore_ascii_case(filter))
            || self
                .label
                .as_deref()
                .is_some_and(|label| label.eq_ignore_ascii_case(filter))
    }
}

pub(super) fn run_setup(
    mode: SetupMode,
    json_output: bool,
    plain: bool,
    tool: Option<String>,
    service: Option<&dyn LanguageToolingService>,
) -> Result<i32, RunError> {
    let cwd = env::current_dir().map_err(|error| RunError::Args(error.to_string()))?;
    let mut statuses = collect(&cwd, service);
    if let Some(filter) = tool {
        statuses.retain(|status| status.matches(&filter));
        if statuses.is_empty() {
            return Err(RunError::Args(format!(
                "unknown setup tool or language: {filter}"
            )));
        }
    }
    let found = statuses
        .iter()
        .filter(|status| status.state() == "found")
        .count();
    let missing = statuses
        .iter()
        .filter(|status| status.state() == "missing")
        .count();
    let no_profile = statuses
        .iter()
        .filter(|status| status.state() == "no_profile")
        .count();

    if json_output {
        print_json(mode, &statuses, found, missing, no_profile)?;
    } else if plain {
        print_plain(&statuses);
    } else if mode == SetupMode::Plan {
        print_plan(&statuses, found, missing, no_profile);
    } else {
        print_check(&statuses, found, missing, no_profile);
    }
    Ok(0)
}

fn collect(root: &Path, service: Option<&dyn LanguageToolingService>) -> Vec<LanguageStatus> {
    anvil_tui::language_probes()
        .iter()
        .map(|(name, extensions)| {
            let extension = extensions.first().copied().unwrap_or("txt");
            let probe = Path::new("probe").with_extension(extension);
            let settings = anvil_tui::resolve_lsp_settings_with_service(&probe, root, service);
            match settings {
                Some(settings) => {
                    let path = resolve_existing(&settings.tool);
                    let provider = provider_for(&settings.tool, &settings.label);
                    LanguageStatus {
                        name,
                        tool: Some(settings.tool),
                        label: Some(settings.label),
                        path,
                        provider: Some(provider),
                    }
                }
                None => LanguageStatus {
                    name,
                    tool: None,
                    label: None,
                    path: None,
                    provider: None,
                },
            }
        })
        .collect()
}

fn resolve_existing(tool: &str) -> Option<PathBuf> {
    let path = Path::new(tool);
    if path.is_absolute() || path.components().count() > 1 {
        return path.is_file().then(|| path.to_path_buf());
    }
    find_tool_path(&[tool])
}

fn print_check(statuses: &[LanguageStatus], found: usize, missing: usize, no_profile: usize) {
    println!(
        "Language tools ({} / {})",
        env::consts::OS,
        env::consts::ARCH
    );
    for status in statuses {
        match status.state() {
            "found" => println!(
                "  {:<28} found    {}",
                status.name,
                status
                    .path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ),
            "missing" => println!(
                "  {:<28} missing  {}",
                status.name,
                status.tool.as_deref().unwrap_or("")
            ),
            _ => println!("  {:<28} no profile", status.name),
        }
    }
    println!();
    println!("Summary: {found} found, {missing} missing, {no_profile} no profile");
    if missing > 0 {
        println!("Run `anvil setup plan` to see installation commands.");
    }
}

fn print_plan(statuses: &[LanguageStatus], found: usize, missing: usize, no_profile: usize) {
    println!(
        "Language tools ({} / {})",
        env::consts::OS,
        env::consts::ARCH
    );
    for status in statuses {
        println!("  {}", status.name);
        match status.state() {
            "found" => {
                println!("    status: found");
                println!(
                    "    tool: {}",
                    status
                        .label
                        .as_deref()
                        .or(status.tool.as_deref())
                        .unwrap_or("")
                );
                println!(
                    "    path: {}",
                    status
                        .path
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default()
                );
                println!("    action: none");
            }
            "missing" => {
                println!("    status: missing");
                println!("    tool: {}", status.tool.as_deref().unwrap_or(""));
                if let Some(provider) = status.provider {
                    println!("    provider: {}", provider.label());
                    println!("    command: {}", provider.command());
                    let requires = provider.requires();
                    if !requires.is_empty() {
                        println!("    requires: {}", requires.join(", "));
                    }
                    println!(
                        "    requires root: {}",
                        if provider.requires_root() {
                            "yes"
                        } else {
                            "no"
                        }
                    );
                }
            }
            _ => {
                println!("    status: no profile");
                println!("    action: none");
            }
        }
    }
    println!();
    println!("Summary: {found} found, {missing} missing, {no_profile} no profile");
    println!("Nothing changed.");
}

fn print_plain(statuses: &[LanguageStatus]) {
    for status in statuses {
        let path = status
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let provider = status
            .provider
            .map(|provider| provider.label())
            .unwrap_or("none");
        let command = status
            .provider
            .map(|provider| provider.command())
            .unwrap_or_default();
        println!(
            "language={} id={} status={} action={} tool={} path={} provider={} requires_root={} command={}",
            status.name,
            status.name.to_ascii_lowercase(),
            status.state(),
            status.action(),
            status.tool.as_deref().unwrap_or(""),
            path,
            provider,
            status
                .provider
                .is_some_and(|provider| provider.requires_root()),
            command
        );
    }
}

fn print_json(
    mode: SetupMode,
    statuses: &[LanguageStatus],
    found: usize,
    missing: usize,
    no_profile: usize,
) -> Result<(), RunError> {
    let tools: Vec<serde_json::Value> = statuses
        .iter()
        .map(|status| {
            let provider = status.provider;
            json!({
                "id": status.name.to_ascii_lowercase(),
                "language": status.name,
                "status": status.state(),
                "action": status.action(),
                "tool": status.tool,
                "label": status.label,
                "path": status.path.as_ref().map(|path| path.to_string_lossy().into_owned()),
                "provider": provider.map(|provider| provider.label()),
                "command": provider.map(|provider| provider.command()),
                "requires": provider.map(|provider| provider.requires()),
                "requires_root": provider.is_some_and(|provider| provider.requires_root()),
            })
        })
        .collect();
    let document = json!({
        "schema_version": 1,
        "mode": mode.as_str(),
        "platform": {
            "os": env::consts::OS,
            "arch": env::consts::ARCH,
        },
        "ok": true,
        "counts": {
            "found": found,
            "missing": missing,
            "no_profile": no_profile,
        },
        "tools": tools,
        "errors": [],
    });
    let text = serde_json::to_string_pretty(&document)
        .map_err(|error| RunError::Args(error.to_string()))?;
    println!("{text}");
    Ok(())
}

fn provider_for(tool: &str, label: &str) -> Provider {
    let name = Path::new(tool)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(tool)
        .to_ascii_lowercase();
    let label = label.to_ascii_lowercase();
    if name.contains("rust-analyzer") || label.contains("rust-analyzer") {
        Provider::Rustup("rust-analyzer")
    } else if name.contains("gopls") || label.contains("gopls") {
        Provider::Go("golang.org/x/tools/gopls@v0.23.0")
    } else if name.contains("typescript-language-server") || label.contains("typescript") {
        Provider::Npm("typescript-language-server@5.1.3 typescript")
    } else if name.contains("vtsls") {
        Provider::Npm("@vtsls/language-server")
    } else if name.contains("pyright") || label.contains("pyright") {
        Provider::Npm("pyright")
    } else if name.contains("pylsp") || label.contains("pylsp") {
        Provider::Pipx("python-lsp-server")
    } else if name.contains("clangd") || label.contains("clangd") {
        Provider::Manual("install clangd through your LLVM/clang package")
    } else if name.contains("jdtls") || label.contains("jdtls") {
        Provider::Manual("install eclipse.jdt.ls and put jdtls on PATH")
    } else if name.contains("lua-language-server") {
        Provider::System("lua-language-server")
    } else if name.contains("json-language-server") {
        Provider::Npm("vscode-json-languageserver")
    } else if name.contains("yaml-language-server") {
        Provider::Npm("yaml-language-server")
    } else if name.contains("bash-language-server") {
        Provider::Npm("bash-language-server")
    } else if name.contains("marksman") {
        Provider::Manual("install marksman from its GitHub releases")
    } else if name.contains("taplo") {
        Provider::Cargo("taplo-cli")
    } else if name.contains("html-language-server") {
        Provider::Npm("vscode-html-languageserver-bin")
    } else if name.contains("css-language-server") {
        Provider::Npm("vscode-css-languageserver-bin")
    } else if name.contains("zls") {
        Provider::System("zls")
    } else {
        Provider::Manual("install a compatible language server for this language")
    }
}

fn system_install_command(package: &str) -> String {
    if find_tool_path(&["pacman"]).is_some() {
        format!("sudo pacman -S --needed {package}")
    } else if find_tool_path(&["apt"]).is_some() {
        format!("sudo apt install {package}")
    } else if find_tool_path(&["dnf"]).is_some() {
        format!("sudo dnf install {package}")
    } else {
        format!("install {package} with your system package manager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_follow_resolved_tools() {
        assert_eq!(
            provider_for("rust-analyzer", "rust-analyzer").label(),
            "rustup"
        );
        assert_eq!(provider_for("gopls", "gopls").label(), "go");
        assert_eq!(
            provider_for("typescript-language-server", "typescript language server").label(),
            "npm"
        );
        assert_eq!(
            provider_for("unknown-server", "custom server").label(),
            "manual"
        );
    }

    #[test]
    fn status_matching_accepts_language_and_tool_names() {
        let status = LanguageStatus {
            name: "Rust",
            tool: Some("rust-analyzer".to_owned()),
            label: Some("rust-analyzer".to_owned()),
            path: None,
            provider: Some(Provider::Rustup("rust-analyzer")),
        };
        assert!(status.matches("Rust"));
        assert!(status.matches("rust-analyzer"));
        assert!(!status.matches("gopls"));
    }
}

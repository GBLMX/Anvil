//! Read-only language-tool setup discovery and planning.

use std::{env, path::PathBuf};

use anvil_lsp_client::find_tool_path;
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

#[derive(Debug)]
struct ToolSpec {
    id: &'static str,
    candidates: &'static [&'static str],
    provider: Provider,
}

const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        id: "rust-analyzer",
        candidates: &["rust-analyzer"],
        provider: Provider::Rustup("rust-analyzer"),
    },
    ToolSpec {
        id: "gopls",
        candidates: &["gopls"],
        provider: Provider::Go("golang.org/x/tools/gopls@v0.23.0"),
    },
    ToolSpec {
        id: "typescript-language-server",
        candidates: &["typescript-language-server"],
        provider: Provider::Npm("typescript-language-server@5.1.3 typescript"),
    },
    ToolSpec {
        id: "vtsls",
        candidates: &["vtsls"],
        provider: Provider::Npm("@vtsls/language-server"),
    },
    ToolSpec {
        id: "clangd",
        candidates: &["clangd"],
        provider: Provider::Manual("install clangd through your LLVM/clang package"),
    },
    ToolSpec {
        id: "pyright",
        candidates: &["pyright", "pyright-langserver"],
        provider: Provider::Npm("pyright"),
    },
    ToolSpec {
        id: "pylsp",
        candidates: &["pylsp"],
        provider: Provider::Pipx("python-lsp-server"),
    },
    ToolSpec {
        id: "jdtls",
        candidates: &["jdtls"],
        provider: Provider::Manual("install eclipse.jdt.ls and put jdtls on PATH"),
    },
    ToolSpec {
        id: "lua-language-server",
        candidates: &["lua-language-server"],
        provider: Provider::System("lua-language-server"),
    },
    ToolSpec {
        id: "vscode-json-language-server",
        candidates: &["vscode-json-language-server", "json-languageserver"],
        provider: Provider::Npm("vscode-json-languageserver"),
    },
    ToolSpec {
        id: "yaml-language-server",
        candidates: &["yaml-language-server"],
        provider: Provider::Npm("yaml-language-server"),
    },
    ToolSpec {
        id: "bash-language-server",
        candidates: &["bash-language-server"],
        provider: Provider::Npm("bash-language-server"),
    },
    ToolSpec {
        id: "marksman",
        candidates: &["marksman"],
        provider: Provider::Manual("install marksman from its GitHub releases"),
    },
    ToolSpec {
        id: "taplo",
        candidates: &["taplo"],
        provider: Provider::Cargo("taplo-cli"),
    },
    ToolSpec {
        id: "vscode-html-language-server",
        candidates: &["vscode-html-language-server"],
        provider: Provider::Npm("vscode-html-languageserver-bin"),
    },
    ToolSpec {
        id: "vscode-css-language-server",
        candidates: &["vscode-css-language-server"],
        provider: Provider::Npm("vscode-css-languageserver-bin"),
    },
    ToolSpec {
        id: "zls",
        candidates: &["zls"],
        provider: Provider::System("zls"),
    },
];

struct ToolStatus {
    spec: &'static ToolSpec,
    path: Option<PathBuf>,
}

pub(super) fn run_setup(
    mode: SetupMode,
    json_output: bool,
    plain: bool,
    tool: Option<String>,
) -> Result<i32, RunError> {
    let specs = selected_specs(tool.as_deref())?;
    let statuses: Vec<ToolStatus> = specs
        .into_iter()
        .map(|spec| ToolStatus {
            spec,
            path: find_tool_path(spec.candidates),
        })
        .collect();
    let found = statuses
        .iter()
        .filter(|status| status.path.is_some())
        .count();
    let missing = statuses.len().saturating_sub(found);

    if json_output {
        print_json(mode, &statuses, found, missing)?;
    } else if plain {
        print_plain(&statuses);
    } else if mode == SetupMode::Plan {
        print_plan(&statuses, found, missing);
    } else {
        print_check(&statuses, found, missing);
    }
    Ok(0)
}

fn selected_specs(tool: Option<&str>) -> Result<Vec<&'static ToolSpec>, RunError> {
    let Some(tool) = tool else {
        return Ok(TOOLS.iter().collect());
    };
    TOOLS
        .iter()
        .find(|spec| spec.id == tool)
        .map(|spec| vec![spec])
        .ok_or_else(|| RunError::Args(format!("unknown setup tool: {tool}")))
}

fn print_check(statuses: &[ToolStatus], found: usize, missing: usize) {
    println!(
        "Language tools ({} / {})",
        env::consts::OS,
        env::consts::ARCH
    );
    for status in statuses {
        match &status.path {
            Some(path) => println!("  {:<28} found    {}", status.spec.id, path.display()),
            None => println!("  {:<28} missing", status.spec.id),
        }
    }
    println!();
    println!("Summary: {found} found, {missing} missing");
    if missing > 0 {
        println!("Run `anvil setup plan` to see installation commands.");
    }
}

fn print_plan(statuses: &[ToolStatus], found: usize, missing: usize) {
    println!(
        "Language tools ({} / {})",
        env::consts::OS,
        env::consts::ARCH
    );
    for status in statuses {
        println!("  {}", status.spec.id);
        match &status.path {
            Some(path) => {
                println!("    status: found");
                println!("    path: {}", path.display());
                println!("    action: none");
            }
            None => {
                println!("    status: missing");
                println!("    provider: {}", status.spec.provider.label());
                println!("    command: {}", status.spec.provider.command());
                let requires = status.spec.provider.requires();
                if !requires.is_empty() {
                    println!("    requires: {}", requires.join(", "));
                }
                println!(
                    "    requires root: {}",
                    if status.spec.provider.requires_root() {
                        "yes"
                    } else {
                        "no"
                    }
                );
            }
        }
    }
    println!();
    println!("Summary: {found} found, {missing} missing");
    println!("Nothing changed.");
}

fn print_plain(statuses: &[ToolStatus]) {
    for status in statuses {
        let (state, path, action) = match &status.path {
            Some(path) => ("found", path.display().to_string(), "none"),
            None => ("missing", String::new(), "install"),
        };
        println!(
            "id={} status={} action={} path={} provider={} requires_root={} command={}",
            status.spec.id,
            state,
            action,
            path,
            status.spec.provider.label(),
            status.spec.provider.requires_root(),
            status.spec.provider.command(),
        );
    }
}

fn print_json(
    mode: SetupMode,
    statuses: &[ToolStatus],
    found: usize,
    missing: usize,
) -> Result<(), RunError> {
    let tools: Vec<serde_json::Value> = statuses
        .iter()
        .map(|status| {
            let path = status
                .path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned());
            json!({
                "id": status.spec.id,
                "status": if path.is_some() { "found" } else { "missing" },
                "path": path,
                "provider": status.spec.provider.label(),
                "command": status.spec.provider.command(),
                "requires": status.spec.provider.requires(),
                "requires_root": status.spec.provider.requires_root(),
                "action": if path.is_some() { "none" } else { "install" },
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
        },
        "tools": tools,
        "errors": [],
    });
    let text = serde_json::to_string_pretty(&document)
        .map_err(|error| RunError::Args(error.to_string()))?;
    println!("{text}");
    Ok(())
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
    fn catalog_ids_are_unique() {
        let mut ids: Vec<&str> = TOOLS.iter().map(|spec| spec.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TOOLS.len());
    }

    #[test]
    fn filtering_rejects_unknown_tools() {
        let error = selected_specs(Some("not-a-server")).unwrap_err();
        assert!(error.to_string().contains("unknown setup tool"));
    }

    #[test]
    fn known_tool_filter_returns_one_entry() {
        let specs = selected_specs(Some("rust-analyzer")).expect("known tool");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].id, "rust-analyzer");
    }
}

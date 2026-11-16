use std::{env, ffi::OsString, path::Path, process};

mod args;
mod doctor;

use args::{Command, parse_args};
use doctor::{json_escape, run_doctor};

const USAGE: &str = "\
anvil — a terminal editor with a selection-first Rust kernel

Usage:
  anvil [OPTIONS] [FILE|-]
  anvil doctor [--json] [--profile auto|dev|installed]
  anvil modules [--json]

Options:
  -w, --workspace <DIR>  Workspace directory; --workspace=DIR is also accepted
  --editor              External-editor mode; allows one explicit file outside
                        the cwd by using its parent as the default workspace
  --read-only           Reject mutating commands
  --truecolor[=MODE]    auto, always, or never; overrides SSH color detection
  -h, --help             Show this help and exit
  -V, --version          Show the version and exit
  --                     Treat the next argument as FILE

Environment:
  ANVIL_GATEWAY          Explicit path to anvil-gateway
  ANVIL_RG               Explicit ripgrep path for fuzzy file listing
  ANVIL_THEME            Startup theme by name; :theme lists built-ins
  ANVIL_TRUECOLOR       Force 24-bit RGB (1) or 256-color fallback (0)
  ANVIL_SETTINGS         Explicit settings.toml path
  ANVIL_EDITOR_SCROLL_MARGIN  Viewport scroll margin (0-16)
  ANVIL_EDITOR_MAX_BUFFERS  Maximum foreground buffers (1-99)
  ANVIL_SEARCH_CASE_SENSITIVE Search default: true or false
  ANVIL_LSP_INLAY_HINTS  Inlay hints: true or false
  ANVIL_BUNDLED_PLUGIN_DIR  Explicit bundled official plugin directory
  ANVIL_PLUGIN_DIR       Additional local plugin directory

Quick open:
  Space f                Fuzzy file finder; the selected file previews through
                         the gateway. Enter opens it.
  Space b                Buffer picker; type to filter, Enter switches.

FILE must be inside the selected workspace. Use -- before a file name that
starts with a dash.
";

/// Where a distribution obtains its startup configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigContract {
    /// Reuse the TUI settings, theme, and keymap loaders unchanged.
    ///
    /// This is a composition consumer, not a second parser. It does not
    /// justify extracting an `anvil-config` crate.
    SharedTui,
}

/// Static composition metadata for one distribution.
#[derive(Clone, Copy, Debug)]
pub struct Distribution {
    /// Process name used in diagnostics and version output.
    pub name: &'static str,
    /// Help text printed for `--help`.
    pub usage: &'static str,
    /// Whether the standard Git module is part of this composition.
    pub git: bool,
    /// Whether the optional workspace-index module is part of this composition.
    pub workspace_index: bool,
    /// Whether the optional desktop URL module is part of this composition.
    pub desktop: bool,
    /// Whether the optional LaTeX language-tooling module is part of this composition.
    pub language_latex: bool,
    /// Whether the optional Typst language-tooling module is part of this composition.
    pub language_typst: bool,
    /// Configuration consumed by this distribution.
    pub config: ConfigContract,
}

/// The standard Anvil distribution.
pub const STANDARD: Distribution = Distribution {
    config: ConfigContract::SharedTui,
    name: "anvil",
    usage: USAGE,
    git: true,
    workspace_index: false,
    desktop: false,
    language_latex: false,
    language_typst: false,
};

/// The full Anvil distribution, enabling every official module.
pub const FULL: Distribution = Distribution {
    config: ConfigContract::SharedTui,
    name: "anvil",
    usage: USAGE,
    git: true,
    workspace_index: true,
    desktop: true,
    language_latex: true,
    language_typst: true,
};

#[derive(Debug)]
enum RunError {
    Args(String),
    Io(std::io::Error),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Args(error) => formatter.write_str(error),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl From<std::io::Error> for RunError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Runs one distribution entrypoint and exits the process with its status.
pub fn main(distribution: &Distribution) -> ! {
    match run(distribution, env::args_os().skip(1)) {
        Ok(code) => process::exit(code),
        Err(RunError::Args(error)) => {
            eprintln!("{}: {error}", distribution.name);
            process::exit(2);
        }
        Err(RunError::Io(error)) => {
            eprintln!("{}: {error}", distribution.name);
            process::exit(1);
        }
    }
}

#[cfg(feature = "official-workspace-index")]
fn compose_workspace_index(
    distribution: &Distribution,
) -> Result<
    Option<std::sync::Arc<dyn anvil_module::workspace_index::WorkspaceIndexService>>,
    RunError,
> {
    if !distribution.workspace_index {
        return Ok(None);
    }
    let mut registry = anvil_module::ModuleRegistry::new();
    registry
        .register(&anvil_mod_workspace_index::WorkspaceIndexModule)
        .map_err(|error| RunError::Args(error.to_string()))?;
    Ok(registry.workspace_index_service())
}

#[cfg(not(feature = "official-workspace-index"))]
fn compose_workspace_index(
    distribution: &Distribution,
) -> Result<
    Option<std::sync::Arc<dyn anvil_module::workspace_index::WorkspaceIndexService>>,
    RunError,
> {
    if distribution.workspace_index {
        return Err(RunError::Args(
            "distribution requires official-workspace-index, but the feature is not compiled"
                .to_owned(),
        ));
    }
    Ok(None)
}

#[cfg(feature = "official-git")]
fn compose_git(
    distribution: &Distribution,
) -> Result<Option<std::sync::Arc<dyn anvil_module::git::GitService>>, RunError> {
    if !distribution.git {
        return Ok(None);
    }
    let mut registry = anvil_module::ModuleRegistry::new();
    registry
        .register(&anvil_mod_git::GitModule)
        .map_err(|error| RunError::Args(error.to_string()))?;
    Ok(registry.git_service())
}

#[cfg(feature = "official-language-latex")]
fn latex_tooling()
-> Result<std::sync::Arc<dyn anvil_module::language::LanguageToolingService>, RunError> {
    let mut registry = anvil_module::ModuleRegistry::new();
    registry
        .register(&anvil_mod_language_latex::LatexModule)
        .map_err(|error| RunError::Args(error.to_string()))?;
    registry.language_tooling_service().ok_or_else(|| {
        RunError::Args("official-language-latex registered no tooling service".to_owned())
    })
}

#[cfg(not(feature = "official-language-latex"))]
fn latex_tooling()
-> Result<std::sync::Arc<dyn anvil_module::language::LanguageToolingService>, RunError> {
    Err(RunError::Args(
        "distribution requires official-language-latex, but the feature is not compiled".to_owned(),
    ))
}

#[cfg(feature = "official-language-typst")]
fn typst_tooling()
-> Result<std::sync::Arc<dyn anvil_module::language::LanguageToolingService>, RunError> {
    let mut registry = anvil_module::ModuleRegistry::new();
    registry
        .register(&anvil_mod_language_typst::TypstModule)
        .map_err(|error| RunError::Args(error.to_string()))?;
    registry.language_tooling_service().ok_or_else(|| {
        RunError::Args("official-language-typst registered no tooling service".to_owned())
    })
}

#[cfg(not(feature = "official-language-typst"))]
fn typst_tooling()
-> Result<std::sync::Arc<dyn anvil_module::language::LanguageToolingService>, RunError> {
    Err(RunError::Args(
        "distribution requires official-language-typst, but the feature is not compiled".to_owned(),
    ))
}

fn compose_language_tooling(
    distribution: &Distribution,
) -> Result<Option<std::sync::Arc<dyn anvil_module::language::LanguageToolingService>>, RunError> {
    let mut providers = Vec::new();
    if distribution.language_latex {
        providers.push(latex_tooling()?);
    }
    if distribution.language_typst {
        providers.push(typst_tooling()?);
    }
    Ok(match providers.len() {
        0 => None,
        1 => providers.pop(),
        _ => Some(std::sync::Arc::new(
            anvil_module::language::LanguageToolingProviders::new(providers),
        )),
    })
}

#[cfg(feature = "official-desktop")]
fn compose_desktop(
    distribution: &Distribution,
) -> Result<Option<std::sync::Arc<dyn anvil_module::desktop::DesktopService>>, RunError> {
    if !distribution.desktop {
        return Ok(None);
    }
    let mut registry = anvil_module::ModuleRegistry::new();
    registry
        .register(&anvil_mod_desktop::DesktopModule)
        .map_err(|error| RunError::Args(error.to_string()))?;
    Ok(registry.desktop_service())
}

#[cfg(not(feature = "official-desktop"))]
fn compose_desktop(
    distribution: &Distribution,
) -> Result<Option<std::sync::Arc<dyn anvil_module::desktop::DesktopService>>, RunError> {
    if distribution.desktop {
        return Err(RunError::Args(
            "distribution requires official-desktop, but the feature is not compiled".to_owned(),
        ));
    }
    Ok(None)
}

#[cfg(not(feature = "official-git"))]
fn compose_git(
    distribution: &Distribution,
) -> Result<Option<std::sync::Arc<dyn anvil_module::git::GitService>>, RunError> {
    if distribution.git {
        return Err(RunError::Args(
            "distribution requires official-git, but the feature is not compiled".to_owned(),
        ));
    }
    Ok(None)
}

fn run<I>(distribution: &Distribution, arguments: I) -> Result<i32, RunError>
where
    I: IntoIterator<Item = OsString>,
{
    match parse_args(arguments) {
        Ok(Command::Help) => {
            print!("{}", distribution.usage);
            Ok(0)
        }
        Ok(Command::Version) => {
            println!("{} {}", distribution.name, env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Ok(Command::Doctor { json, profile }) => run_doctor(distribution, json, profile),
        Ok(Command::Modules { json }) => run_modules(distribution, json),
        Ok(Command::Run {
            workspace,
            path,
            editor,
            stdin,
            read_only,
            truecolor,
        }) => {
            if !stdin
                && !editor
                && forward_to_running(distribution, &workspace, path.as_deref(), read_only)?
            {
                return Ok(0);
            }
            if let Some(enabled) = truecolor {
                unsafe { env::set_var("ANVIL_TRUECOLOR", if enabled { "1" } else { "0" }) };
            }
            let git = compose_git(distribution)?;
            let workspace_index = compose_workspace_index(distribution)?;
            let desktop = compose_desktop(distribution)?;
            let language_tooling = compose_language_tooling(distribution)?;
            Ok(anvil_tui::run_with_all_services(
                path,
                workspace,
                editor,
                stdin,
                read_only,
                anvil_tui::DistributionServices {
                    git,
                    workspace_index,
                    desktop,
                    language_tooling,
                },
            )?)
        }
        Err(error) => Err(RunError::Args(error)),
    }
}

type ModuleEntry = (&'static str, anvil_module::ModuleMetadata);

fn module_catalog(distribution: &Distribution) -> Vec<ModuleEntry> {
    let _ = distribution;
    let mut modules = Vec::new();
    #[cfg(feature = "official-git")]
    if distribution.git {
        modules.push((
            "official-git",
            anvil_module::Module::metadata(&anvil_mod_git::GitModule),
        ));
    }
    #[cfg(feature = "official-workspace-index")]
    if distribution.workspace_index {
        modules.push((
            "official-workspace-index",
            anvil_module::Module::metadata(&anvil_mod_workspace_index::WorkspaceIndexModule),
        ));
    }
    #[cfg(feature = "official-desktop")]
    if distribution.desktop {
        modules.push((
            "official-desktop",
            anvil_module::Module::metadata(&anvil_mod_desktop::DesktopModule),
        ));
    }
    #[cfg(feature = "official-language-latex")]
    if distribution.language_latex {
        modules.push((
            "official-language-latex",
            anvil_module::Module::metadata(&anvil_mod_language_latex::LatexModule),
        ));
    }
    #[cfg(feature = "official-language-typst")]
    if distribution.language_typst {
        modules.push((
            "official-language-typst",
            anvil_module::Module::metadata(&anvil_mod_language_typst::TypstModule),
        ));
    }
    modules
}

fn run_modules(distribution: &Distribution, json: bool) -> Result<i32, RunError> {
    let modules = module_catalog(distribution);
    if json {
        let mut out = String::from("{\"schema_version\":1,\"distribution\":\"");
        out.push_str(&json_escape(distribution.name));
        out.push_str("\",\"version\":\"");
        out.push_str(env!("CARGO_PKG_VERSION"));
        out.push_str("\",\"modules\":[");
        for (index, (feature, module)) in modules.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"id\":\"");
            out.push_str(&json_escape(module.id.as_str()));
            out.push_str("\",\"name\":\"");
            out.push_str(&json_escape(module.name));
            out.push_str("\",\"contract\":\"");
            out.push_str(&format!("{}..{}", module.contract.min, module.contract.max));
            out.push_str("\",\"version\":\"");
            out.push_str(env!("CARGO_PKG_VERSION"));
            out.push_str("\",\"source\":\"compiled-in\",\"enabled\":true,\"feature\":\"");
            out.push_str(feature);
            out.push_str("\",\"commands\":");
            out.push_str(&module.commands.len().to_string());
            out.push('}');
        }
        out.push_str("]}");
        println!("{out}");
    } else {
        println!(
            "{} {} modules (distribution {})",
            distribution.name,
            env!("CARGO_PKG_VERSION"),
            distribution.name
        );
        for (feature, module) in &modules {
            println!(
                "{:<28} {:<30} contract {}-{}  commands {:<3} feature={} enabled",
                module.id.as_str(),
                module.name,
                module.contract.min,
                module.contract.max,
                module.commands.len(),
                feature
            );
        }
    }
    Ok(0)
}

fn forward_to_running(
    distribution: &Distribution,
    workspace: &Path,
    path: Option<&Path>,
    read_only: bool,
) -> std::io::Result<bool> {
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let mut client = match anvil_control::Client::connect_for(&workspace) {
        Ok(client) => client,
        Err(_) => return Ok(false),
    };
    let status = match client.status() {
        Ok(status) => status,
        Err(_) => return Ok(false),
    };
    let running = status
        .workspace
        .canonicalize()
        .unwrap_or_else(|_| status.workspace.clone());
    if running != workspace {
        return Ok(false);
    }
    if read_only {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "a writable {} instance is already running",
                distribution.name
            ),
        ));
    }
    let Some(path) = path else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("another {} instance is already running", distribution.name),
        ));
    };
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };
    let absolute = absolute.canonicalize().map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{}: {error}", path.display()),
        )
    })?;
    if !absolute.starts_with(&workspace) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path is outside workspace: {}", path.display()),
        ));
    }
    client
        .open(&absolute, 1, 1)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "official-git")]
    #[test]
    fn standard_composition_registers_the_git_module() {
        assert!(compose_git(&STANDARD).expect("compose").is_some());
        let no_git = Distribution {
            git: false,
            ..STANDARD
        };
        assert!(compose_git(&no_git).expect("compose").is_none());
    }

    #[cfg(not(feature = "official-git"))]
    #[test]
    fn missing_git_feature_is_a_startup_error() {
        assert!(compose_git(&STANDARD).is_err());
        let no_git = Distribution {
            git: false,
            ..STANDARD
        };
        assert!(compose_git(&no_git).expect("compose").is_none());
    }
    #[cfg(feature = "official-workspace-index")]
    #[test]
    fn opt_in_workspace_index_registers_the_service() {
        let with_index = Distribution {
            workspace_index: true,
            ..STANDARD
        };
        assert!(
            compose_workspace_index(&with_index)
                .expect("compose")
                .is_some()
        );
        assert!(
            compose_workspace_index(&STANDARD)
                .expect("compose")
                .is_none()
        );
    }

    #[cfg(not(feature = "official-workspace-index"))]
    #[test]
    fn missing_workspace_index_feature_is_a_startup_error() {
        let with_index = Distribution {
            workspace_index: true,
            ..STANDARD
        };
        assert!(compose_workspace_index(&with_index).is_err());
        assert!(
            compose_workspace_index(&STANDARD)
                .expect("compose")
                .is_none()
        );
    }

    #[cfg(feature = "official-desktop")]
    #[test]
    fn opt_in_desktop_registers_the_service() {
        let with_desktop = Distribution {
            desktop: true,
            ..STANDARD
        };
        assert!(compose_desktop(&with_desktop).expect("compose").is_some());
        assert!(compose_desktop(&STANDARD).expect("compose").is_none());
    }

    #[cfg(not(feature = "official-desktop"))]
    #[test]
    fn missing_desktop_feature_is_a_startup_error() {
        let with_desktop = Distribution {
            desktop: true,
            ..STANDARD
        };
        assert!(compose_desktop(&with_desktop).is_err());
        assert!(compose_desktop(&STANDARD).expect("compose").is_none());
    }

    #[cfg(feature = "official-language-latex")]
    #[test]
    fn opt_in_latex_registers_the_language_tooling_service() {
        let with_latex = Distribution {
            language_latex: true,
            ..STANDARD
        };
        assert!(
            compose_language_tooling(&with_latex)
                .expect("compose")
                .is_some()
        );
        assert!(
            compose_language_tooling(&STANDARD)
                .expect("compose")
                .is_none()
        );
    }

    #[cfg(not(feature = "official-language-latex"))]
    #[test]
    fn missing_latex_feature_is_a_startup_error() {
        let with_latex = Distribution {
            language_latex: true,
            ..STANDARD
        };
        assert!(compose_language_tooling(&with_latex).is_err());
        assert!(
            compose_language_tooling(&STANDARD)
                .expect("compose")
                .is_none()
        );
    }

    #[cfg(feature = "official-language-typst")]
    #[test]
    fn opt_in_typst_registers_the_language_tooling_service() {
        let with_typst = Distribution {
            language_typst: true,
            ..STANDARD
        };
        assert!(
            compose_language_tooling(&with_typst)
                .expect("compose")
                .is_some()
        );
        assert!(
            compose_language_tooling(&STANDARD)
                .expect("compose")
                .is_none()
        );
    }

    #[cfg(not(feature = "official-language-typst"))]
    #[test]
    fn missing_typst_feature_is_a_startup_error() {
        let with_typst = Distribution {
            language_typst: true,
            ..STANDARD
        };
        assert!(compose_language_tooling(&with_typst).is_err());
        assert!(
            compose_language_tooling(&STANDARD)
                .expect("compose")
                .is_none()
        );
    }

    #[cfg(all(
        feature = "official-language-latex",
        feature = "official-language-typst"
    ))]
    #[test]
    fn both_language_providers_compose() {
        let both = Distribution {
            language_latex: true,
            language_typst: true,
            ..STANDARD
        };
        let service = compose_language_tooling(&both)
            .expect("compose")
            .expect("service");
        assert_eq!(
            service
                .server_for(std::path::Path::new("paper.tex"))
                .expect("latex")
                .tool,
            "texlab"
        );
        assert_eq!(
            service
                .server_for(std::path::Path::new("paper.typ"))
                .expect("typst")
                .tool,
            "tinymist"
        );
    }

    #[cfg(feature = "full")]
    #[test]
    fn full_distribution_enables_every_module() {
        let modules = module_catalog(&FULL);
        assert_eq!(modules.len(), 5);
        for feature in [
            "official-git",
            "official-workspace-index",
            "official-desktop",
            "official-language-latex",
            "official-language-typst",
        ] {
            assert!(
                modules.iter().any(|(name, _)| *name == feature),
                "missing {feature}"
            );
        }
        assert!(compose_workspace_index(&FULL).expect("index").is_some());
        assert!(compose_desktop(&FULL).expect("desktop").is_some());
        assert!(compose_language_tooling(&FULL).expect("language").is_some());
    }

    #[cfg(feature = "official-git")]
    #[test]
    fn module_catalog_lists_the_compiled_git_module() {
        let modules = module_catalog(&STANDARD);
        assert!(
            modules
                .iter()
                .any(|(feature, module)| *feature == "official-git"
                    && module.id.as_str() == "official.git"),
            "missing official.git in {modules:?}"
        );
    }
}

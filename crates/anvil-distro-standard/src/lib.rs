#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process,
};

const USAGE: &str = "\
anvil — a terminal editor with a selection-first Rust kernel

Usage:
  anvil [OPTIONS] [FILE|-]
  anvil doctor [--json] [--profile auto|dev|installed]

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

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Run {
        workspace: PathBuf,
        path: Option<PathBuf>,
        editor: bool,
        stdin: bool,
        read_only: bool,
        truecolor: Option<bool>,
    },
    Help,
    Doctor {
        json: bool,
        profile: DoctorProfileChoice,
    },
    Version,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DoctorProfileChoice {
    Auto,
    Dev,
    Installed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DoctorProfile {
    Dev,
    Installed,
}

impl DoctorProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Installed => "installed",
        }
    }
}

impl DoctorProfileChoice {
    fn resolve(self) -> DoctorProfile {
        match self {
            Self::Dev => DoctorProfile::Dev,
            Self::Installed => DoctorProfile::Installed,
            Self::Auto if installed_layout_root().is_some() => DoctorProfile::Installed,
            Self::Auto => DoctorProfile::Dev,
        }
    }
}

fn installed_layout_root() -> Option<PathBuf> {
    let executable = env::current_exe().ok()?;
    let bin = executable.parent()?;
    if bin.file_name().and_then(|name| name.to_str()) != Some("bin") {
        return None;
    }
    let prefix = bin.parent()?;
    prefix
        .join("share/anvil")
        .is_dir()
        .then(|| prefix.to_path_buf())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DoctorLevel {
    Ok,
    Warn,
    Error,
}

impl DoctorLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

struct DoctorCheck {
    id: &'static str,
    level: DoctorLevel,
    message: String,
    remediation: String,
}

fn check(
    id: &'static str,
    level: DoctorLevel,
    message: impl Into<String>,
    remediation: impl Into<String>,
) -> DoctorCheck {
    DoctorCheck {
        id,
        level,
        message: message.into(),
        remediation: remediation.into(),
    }
}

fn run_doctor(
    distribution: &Distribution,
    json: bool,
    profile_choice: DoctorProfileChoice,
) -> Result<i32, RunError> {
    let profile = profile_choice.resolve();
    let binary_dir = env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(Path::to_path_buf));
    let checks = doctor_checks(profile, binary_dir.as_deref());
    let errors = checks
        .iter()
        .filter(|check| check.level == DoctorLevel::Error)
        .count();
    let exit_code = doctor_exit_code(&checks);
    if json {
        println!(
            "{}",
            doctor_json(distribution, profile, &checks, exit_code == 0)
        );
    } else {
        println!("{} doctor ({})", distribution.name, profile.as_str());
        for check in &checks {
            println!(
                "{:<5} {:<16} {}",
                check.level.as_str(),
                check.id,
                check.message
            );
            if !check.remediation.is_empty() {
                println!("      remediation: {}", check.remediation);
            }
        }
        println!();
        if errors > 0 {
            println!("{errors} error(s); fix the failing checks before using Anvil.");
        } else {
            println!("installation checks passed.");
        }
    }
    Ok(exit_code)
}

fn doctor_exit_code(checks: &[DoctorCheck]) -> i32 {
    usize::from(checks.iter().any(|check| check.level == DoctorLevel::Error)) as i32
}
fn doctor_checks(profile: DoctorProfile, binary_dir: Option<&Path>) -> Vec<DoctorCheck> {
    vec![
        check_workspace(),
        check_gateway(profile, binary_dir),
        check_companion(
            "anvil-exec-runner",
            profile,
            binary_dir,
            profile == DoctorProfile::Installed,
        ),
        check_companion("anvil-plugin-host", profile, binary_dir, false),
        check_companion("anvil-ctl", profile, binary_dir, false),
        check_companion("anvil-mcp", profile, binary_dir, false),
        check_locale(),
        check_colors(),
        check_remote(),
        check_runtime_dir(),
        check_config(),
        check_landlock(),
        check_lsp(),
    ]
}

fn check_workspace() -> DoctorCheck {
    match env::current_dir() {
        Ok(path) if path.is_dir() => {
            check("workspace", DoctorLevel::Ok, path.display().to_string(), "")
        }
        Ok(path) => check(
            "workspace",
            DoctorLevel::Error,
            format!("not a directory: {}", path.display()),
            "run Anvil from a real workspace directory",
        ),
        Err(error) => check(
            "workspace",
            DoctorLevel::Error,
            error.to_string(),
            "check the current directory permissions",
        ),
    }
}

fn check_gateway(profile: DoctorProfile, binary_dir: Option<&Path>) -> DoctorCheck {
    let path = env::var_os("ANVIL_GATEWAY")
        .map(PathBuf::from)
        .or_else(|| binary_dir.map(|directory| directory.join("anvil-gateway")));
    match path {
        Some(path) => check_binary(
            "gateway",
            profile,
            &path,
            profile == DoctorProfile::Installed,
        ),
        None => check(
            "gateway",
            if profile == DoctorProfile::Installed {
                DoctorLevel::Error
            } else {
                DoctorLevel::Warn
            },
            "cannot locate anvil-gateway",
            "install it beside anvil or set ANVIL_GATEWAY",
        ),
    }
}

fn check_companion(
    id: &'static str,
    profile: DoctorProfile,
    binary_dir: Option<&Path>,
    required: bool,
) -> DoctorCheck {
    let Some(path) = binary_dir.map(|directory| directory.join(id)) else {
        return check(
            id,
            if required {
                DoctorLevel::Error
            } else {
                DoctorLevel::Warn
            },
            "cannot determine the Anvil executable directory",
            "install the complete Anvil release layout",
        );
    };
    check_binary(id, profile, &path, required)
}

fn check_binary(
    id: &'static str,
    profile: DoctorProfile,
    path: &Path,
    required: bool,
) -> DoctorCheck {
    if path.is_file() && is_executable(path) {
        return check(id, DoctorLevel::Ok, path.display().to_string(), "");
    }
    let level = if required {
        DoctorLevel::Error
    } else {
        DoctorLevel::Warn
    };
    let (context, remediation) = match profile {
        DoctorProfile::Dev => (
            "dev target sidecar missing",
            "build the sidecar or use --profile installed for a release layout",
        ),
        DoctorProfile::Installed => (
            "installed sidecar missing",
            "install the complete Anvil release layout",
        ),
    };
    check(
        id,
        level,
        format!("{context}: {}", path.display()),
        remediation,
    )
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn nonempty_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn check_locale() -> DoctorCheck {
    let Some(locale) = ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .find_map(nonempty_env)
    else {
        return check(
            "locale",
            DoctorLevel::Error,
            "LC_ALL/LC_CTYPE/LANG is unset",
            "export LANG=C.UTF-8",
        );
    };
    let lower = locale.to_ascii_lowercase();
    if lower.contains("utf-8") || lower.contains("utf8") {
        check("locale", DoctorLevel::Ok, locale, "")
    } else {
        check(
            "locale",
            DoctorLevel::Error,
            format!("{locale} is not UTF-8"),
            "export LANG=C.UTF-8 or another UTF-8 locale",
        )
    }
}

fn check_colors() -> DoctorCheck {
    let term = env::var("TERM").unwrap_or_default().to_ascii_lowercase();
    if term == "dumb" {
        return check(
            "truecolor",
            DoctorLevel::Warn,
            "TERM=dumb",
            "run in a real terminal; a full-screen TUI needs cursor control",
        );
    }
    let forced = env::var("ANVIL_TRUECOLOR").ok();
    match forced.as_deref() {
        Some("0") => return check("truecolor", DoctorLevel::Ok, "forced 256-color", ""),
        Some("1") => return check("truecolor", DoctorLevel::Ok, "forced 24-bit", ""),
        _ => {}
    }
    let colorterm = env::var("COLORTERM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if colorterm.contains("truecolor")
        || colorterm.contains("24bit")
        || term.contains("direct")
        || term.contains("truecolor")
    {
        check(
            "truecolor",
            DoctorLevel::Ok,
            format!("detected; COLORTERM={colorterm:?} TERM={term:?}"),
            "",
        )
    } else {
        check(
            "truecolor",
            DoctorLevel::Warn,
            "not detected; using 256 colors",
            "set ANVIL_TRUECOLOR=1 or --truecolor=always if the terminal supports 24-bit color",
        )
    }
}

fn check_remote() -> DoctorCheck {
    let ssh = ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        .any(|name| env::var_os(name).is_some());
    check_remote_session(
        ssh,
        &env::var("TERM").unwrap_or_default().to_ascii_lowercase(),
        &env::var("COLORTERM")
            .unwrap_or_default()
            .to_ascii_lowercase(),
        env::var_os("TMUX").is_some(),
    )
}

fn check_remote_session(ssh: bool, term: &str, colorterm: &str, tmux: bool) -> DoctorCheck {
    if !ssh {
        return check("remote", DoctorLevel::Ok, "local session", "");
    }
    let truecolor = colorterm.contains("truecolor")
        || colorterm.contains("24bit")
        || term.contains("direct")
        || term.contains("truecolor");
    let mut notes = Vec::new();
    if term == "dumb" {
        notes.push("TERM=dumb");
    }
    if !truecolor {
        notes.push("COLORTERM was not forwarded");
    }
    if tmux {
        notes.push("inside tmux/screen");
    }
    if notes.is_empty() {
        check(
            "remote",
            DoctorLevel::Ok,
            format!("SSH session; TERM={term:?} COLORTERM={colorterm:?}"),
            "",
        )
    } else {
        check(
            "remote",
            DoctorLevel::Warn,
            format!(
                "SSH session; TERM={term:?} COLORTERM={colorterm:?}; {}",
                notes.join("; ")
            ),
            "use ANVIL_TRUECOLOR=1 or --truecolor=always; configure tmux terminal-features for RGB",
        )
    }
}
fn check_runtime_dir() -> DoctorCheck {
    if let Some(runtime) = nonempty_env("XDG_RUNTIME_DIR") {
        let path = PathBuf::from(runtime).join("anvil");
        if path.exists() && !path.is_dir() {
            return check(
                "runtime-dir",
                DoctorLevel::Error,
                format!("not a directory: {}", path.display()),
                "remove the conflicting path or change XDG_RUNTIME_DIR",
            );
        }
        let level = if path.is_dir() {
            DoctorLevel::Ok
        } else {
            DoctorLevel::Warn
        };
        let state = if level == DoctorLevel::Ok {
            "exists"
        } else {
            "will be created"
        };
        return check(
            "runtime-dir",
            level,
            format!("{} ({state})", path.display()),
            "",
        );
    }
    let Some(home) = nonempty_env("HOME") else {
        return check(
            "runtime-dir",
            DoctorLevel::Error,
            "XDG_RUNTIME_DIR is unset and HOME is unavailable",
            "set XDG_RUNTIME_DIR or HOME",
        );
    };
    let path = PathBuf::from(home).join(".local/state/anvil/runtime/anvil");
    check(
        "runtime-dir",
        DoctorLevel::Warn,
        format!("XDG_RUNTIME_DIR is unset; fallback {}", path.display()),
        "set XDG_RUNTIME_DIR to a private user runtime directory",
    )
}

fn config_path() -> Option<PathBuf> {
    if let Some(path) = nonempty_env("ANVIL_SETTINGS") {
        return Some(PathBuf::from(path));
    }
    if let Some(config) = nonempty_env("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(config).join("anvil/settings.toml"));
    }
    nonempty_env("HOME").map(|home| PathBuf::from(home).join(".config/anvil/settings.toml"))
}

fn check_config() -> DoctorCheck {
    let Some(path) = config_path() else {
        return check(
            "config",
            DoctorLevel::Error,
            "cannot resolve ANVIL_SETTINGS, XDG_CONFIG_HOME, or HOME",
            "set ANVIL_SETTINGS or HOME",
        );
    };
    if path.is_file() {
        check("config", DoctorLevel::Ok, path.display().to_string(), "")
    } else if nonempty_env("ANVIL_SETTINGS").is_some() {
        check(
            "config",
            DoctorLevel::Error,
            format!("ANVIL_SETTINGS does not exist: {}", path.display()),
            "create the file or unset ANVIL_SETTINGS",
        )
    } else {
        check(
            "config",
            DoctorLevel::Warn,
            format!("not found; defaults will be used ({})", path.display()),
            "create the file or run :settings to save defaults",
        )
    }
}

fn check_landlock() -> DoctorCheck {
    #[cfg(target_os = "linux")]
    {
        match fs::read_to_string("/sys/kernel/security/lsm") {
            Ok(lsm) if lsm.contains("landlock") => {
                check("landlock", DoctorLevel::Ok, "Landlock is active", "")
            }
            Ok(_) if seccomp_available() => check(
                "landlock",
                DoctorLevel::Warn,
                "Landlock unavailable; seccomp fallback is active",
                "enable Landlock in the kernel or accept the documented seccomp fallback",
            ),
            Ok(_) => check(
                "landlock",
                DoctorLevel::Error,
                "neither Landlock nor seccomp is available",
                "use a Linux kernel with Landlock or seccomp",
            ),
            Err(error) if seccomp_available() => check(
                "landlock",
                DoctorLevel::Warn,
                format!("cannot inspect LSMs: {error}; seccomp fallback is active"),
                "check kernel security permissions or accept the documented fallback",
            ),
            Err(error) => check(
                "landlock",
                DoctorLevel::Error,
                format!("cannot inspect LSMs: {error}; no seccomp fallback"),
                "check kernel security permissions",
            ),
        }
    }
    #[cfg(not(target_os = "linux"))]
    check(
        "landlock",
        DoctorLevel::Error,
        "Anvil is Linux-first; sandbox checks are unavailable",
        "run Anvil on Linux with Landlock or seccomp",
    )
}

#[cfg(target_os = "linux")]
fn seccomp_available() -> bool {
    fs::read_to_string("/proc/sys/kernel/seccomp/actions_avail")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

const LSP_TOOLS: &[&str] = &[
    "rust-analyzer",
    "gopls",
    "pyright",
    "pylsp",
    "typescript-language-server",
    "vtsls",
    "clangd",
    "jdtls",
    "lua-language-server",
    "vscode-json-language-server",
    "yaml-language-server",
    "bash-language-server",
    "marksman",
    "taplo",
    "vscode-html-language-server",
    "vscode-css-language-server",
    "zls",
];

fn check_lsp() -> DoctorCheck {
    let Some(path) = env::var_os("PATH") else {
        return check(
            "lsp",
            DoctorLevel::Warn,
            "PATH is unset; no language servers can be discovered",
            "put language servers on PATH",
        );
    };
    let mut found = Vec::new();
    for directory in env::split_paths(&path) {
        for tool in LSP_TOOLS {
            let candidate = directory.join(tool);
            if candidate.is_file() && is_executable(&candidate) && !found.contains(tool) {
                found.push(*tool);
            }
        }
    }
    if found.is_empty() {
        check(
            "lsp",
            DoctorLevel::Warn,
            format!(
                "no supported language server found on PATH ({} candidates)",
                LSP_TOOLS.len()
            ),
            "install the language servers you use, for example rust-analyzer or gopls",
        )
    } else {
        let missing = LSP_TOOLS.len().saturating_sub(found.len());
        check(
            "lsp",
            DoctorLevel::Ok,
            format!(
                "found {} server(s): {}; {missing} optional server(s) missing",
                found.len(),
                found.join(", ")
            ),
            "",
        )
    }
}

fn doctor_json(
    distribution: &Distribution,
    profile: DoctorProfile,
    checks: &[DoctorCheck],
    ok: bool,
) -> String {
    let mut out = String::from("{\"schema_version\":1,\"anvil_version\":\"");
    out.push_str(&json_escape(env!("CARGO_PKG_VERSION")));
    out.push_str("\",\"profile\":\"");
    out.push_str(profile.as_str());
    out.push_str("\",\"distribution\":\"");
    out.push_str(&json_escape(distribution.name));
    out.push_str("\",\"ok\":");
    out.push_str(if ok { "true" } else { "false" });
    out.push_str(",\"checks\":[");
    for (index, check) in checks.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":\"");
        out.push_str(&json_escape(check.id));
        out.push_str("\",\"status\":\"");
        out.push_str(check.level.as_str());
        out.push_str("\",\"message\":\"");
        out.push_str(&json_escape(&check.message));
        out.push_str("\",\"remediation\":\"");
        out.push_str(&json_escape(&check.remediation));
        out.push_str("\"}");
    }
    out.push_str("]}");
    out
}

fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out
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

fn parse_args<I>(arguments: I) -> Result<Command, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = arguments.into_iter().peekable();
    if arguments
        .peek()
        .is_some_and(|argument| argument == "doctor")
    {
        arguments.next();
        return parse_doctor_args(arguments);
    }
    let mut workspace = None;
    let mut path = None;
    let mut editor = false;
    let mut stdin = false;
    let mut read_only = false;
    let mut truecolor = None;

    while let Some(argument) = arguments.next() {
        if argument == "--help" || argument == "-h" {
            return Ok(Command::Help);
        }
        if argument == "--version" || argument == "-V" {
            return Ok(Command::Version);
        }
        if argument == "--editor" {
            editor = true;
            continue;
        }
        if argument == "--read-only" {
            read_only = true;
            continue;
        }
        if argument == "--truecolor" {
            truecolor = Some(true);
            continue;
        }
        if let Some(value) = argument
            .to_str()
            .and_then(|value| value.strip_prefix("--truecolor="))
        {
            truecolor = match value.to_ascii_lowercase().as_str() {
                "auto" => None,
                "always" | "true" | "1" | "on" => Some(true),
                "never" | "false" | "0" | "off" => Some(false),
                _ => return Err(format!("invalid --truecolor mode: {value}")),
            };
            continue;
        }
        if argument == "-" {
            stdin = true;
            continue;
        }
        if argument == "--workspace" || argument == "-w" {
            let Some(value) = arguments.next() else {
                return Err("--workspace requires a directory".to_owned());
            };
            workspace = Some(PathBuf::from(value));
            continue;
        }
        if let Some(value) = argument
            .to_str()
            .and_then(|value| value.strip_prefix("--workspace="))
        {
            if value.is_empty() {
                return Err("--workspace requires a directory".to_owned());
            }
            workspace = Some(PathBuf::from(value));
            continue;
        }
        if argument == "--" {
            path = arguments.next().map(PathBuf::from);
            if arguments.next().is_some() {
                return Err("only one file may be opened".to_owned());
            }
            break;
        }
        if argument.to_string_lossy().starts_with('-') {
            return Err(format!("unknown option: {}", argument.to_string_lossy()));
        }
        if path.is_some() {
            return Err("only one file may be opened".to_owned());
        }
        path = Some(PathBuf::from(argument));
    }

    if stdin && path.is_some() {
        return Err("stdin and FILE are mutually exclusive".to_owned());
    }
    if editor && stdin {
        return Err("--editor cannot be combined with stdin".to_owned());
    }
    let workspace = match workspace {
        Some(workspace) => workspace,
        None => workspace_for_path(path.as_deref())?,
    };
    Ok(Command::Run {
        workspace,
        path,
        editor,
        stdin,
        read_only,
        truecolor,
    })
}

fn parse_doctor_args<I>(arguments: I) -> Result<Command, String>
where
    I: Iterator<Item = OsString>,
{
    let mut json = false;
    let mut profile = DoctorProfileChoice::Auto;
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--json") => json = true,
            Some("--profile") => {
                let Some(value) = arguments.next() else {
                    return Err("--profile requires auto, dev, or installed".to_owned());
                };
                profile = parse_doctor_profile(&value.to_string_lossy())?;
            }
            Some(value) if value.starts_with("--profile=") => {
                profile = parse_doctor_profile(&value["--profile=".len()..])?;
            }
            Some("-h" | "--help") => return Ok(Command::Help),
            _ => {
                return Err(format!(
                    "unknown doctor option: {}",
                    argument.to_string_lossy()
                ));
            }
        }
    }
    Ok(Command::Doctor { json, profile })
}

fn parse_doctor_profile(value: &str) -> Result<DoctorProfileChoice, String> {
    match value {
        "auto" => Ok(DoctorProfileChoice::Auto),
        "dev" => Ok(DoctorProfileChoice::Dev),
        "installed" => Ok(DoctorProfileChoice::Installed),
        _ => Err(format!(
            "invalid doctor profile: {value}; expected auto, dev, or installed"
        )),
    }
}

fn workspace_for_path(path: Option<&Path>) -> Result<PathBuf, String> {
    let cwd = env::current_dir().map_err(|error| error.to_string())?;
    let Some(path) = path else {
        return Ok(cwd);
    };
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let absolute = absolute.canonicalize().unwrap_or(absolute);
    let parent = absolute.parent().unwrap_or(&cwd);
    if parent.starts_with(&cwd) {
        Ok(cwd)
    } else {
        Ok(parent
            .canonicalize()
            .unwrap_or_else(|_| parent.to_path_buf()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_workspace_and_file() {
        let command = parse_args(args(&["-w", "/tmp/workspace", "src/main.rs"])).unwrap();
        assert_eq!(
            command,
            Command::Run {
                workspace: PathBuf::from("/tmp/workspace"),
                path: Some(PathBuf::from("src/main.rs")),
                editor: false,
                stdin: false,
                read_only: false,
                truecolor: None,
            }
        );
    }

    #[test]
    fn parses_workspace_equals_form() {
        let command = parse_args(args(&["--workspace=/tmp/workspace"])).unwrap();
        assert_eq!(
            command,
            Command::Run {
                workspace: PathBuf::from("/tmp/workspace"),
                path: None,
                editor: false,
                stdin: false,
                read_only: false,
                truecolor: None,
            }
        );
    }

    #[test]
    fn parses_truecolor_modes() {
        let Command::Run { truecolor, .. } = parse_args(args(&["--truecolor=always"])).unwrap()
        else {
            panic!("expected run command");
        };
        assert_eq!(truecolor, Some(true));

        let Command::Run { truecolor, .. } = parse_args(args(&["--truecolor=never"])).unwrap()
        else {
            panic!("expected run command");
        };
        assert_eq!(truecolor, Some(false));

        let Command::Run { truecolor, .. } = parse_args(args(&["--truecolor=auto"])).unwrap()
        else {
            panic!("expected run command");
        };
        assert_eq!(truecolor, None);

        assert!(
            parse_args(args(&["--truecolor=sometimes"]))
                .unwrap_err()
                .contains("invalid --truecolor mode")
        );
    }

    #[test]
    fn parses_double_dash_file() {
        let command = parse_args(args(&["--workspace", "/tmp/workspace", "--", "-notes"])).unwrap();
        assert_eq!(
            command,
            Command::Run {
                workspace: PathBuf::from("/tmp/workspace"),
                path: Some(PathBuf::from("-notes")),
                editor: false,
                stdin: false,
                read_only: false,
                truecolor: None,
            }
        );
    }

    #[test]
    fn file_outside_cwd_uses_its_parent_as_workspace() {
        let directory = tempfile::tempdir().expect("tempdir");
        let file = directory.path().join("outside.txt");
        std::fs::write(&file, "text").expect("write");
        let command = parse_args(args(&[file.to_str().expect("utf-8 path")])).unwrap();
        let Command::Run {
            workspace,
            path,
            editor,
            ..
        } = command
        else {
            panic!("expected run command");
        };
        assert_eq!(
            workspace,
            directory.path().canonicalize().expect("canonical")
        );
        assert_eq!(path, Some(file));
        assert!(!editor);
    }

    #[test]
    fn parses_stdin_and_read_only() {
        let command = parse_args(args(&["-", "--read-only"])).unwrap();
        let Command::Run {
            path,
            editor,
            stdin,
            read_only,
            ..
        } = command
        else {
            panic!("expected run command");
        };
        assert_eq!(path, None);
        assert!(!editor);
        assert!(stdin);
        assert!(read_only);
    }

    #[test]
    fn rejects_stdin_with_a_file_or_editor_mode() {
        assert!(
            parse_args(args(&["-", "file.rs"]))
                .unwrap_err()
                .contains("mutually exclusive")
        );
        assert!(
            parse_args(args(&["--editor", "-"]))
                .unwrap_err()
                .contains("cannot be combined")
        );
    }

    #[test]
    fn read_only_file_sets_the_flag_without_stdin() {
        let command = parse_args(args(&["--read-only", "file.rs"])).unwrap();
        let Command::Run {
            path,
            editor,
            stdin,
            read_only,
            ..
        } = command
        else {
            panic!("expected run command");
        };
        assert_eq!(path, Some(PathBuf::from("file.rs")));
        assert!(!editor);
        assert!(!stdin);
        assert!(read_only);
    }

    #[test]
    fn editor_mode_uses_the_external_file_parent_as_workspace() {
        let directory = tempfile::tempdir().expect("tempdir");
        let file = directory.path().join("message.txt");
        std::fs::write(&file, "text").expect("write");
        let command = parse_args(args(&["--editor", file.to_str().expect("utf-8 path")])).unwrap();
        assert_eq!(
            command,
            Command::Run {
                workspace: directory.path().canonicalize().expect("canonical"),
                path: Some(file),
                editor: true,
                stdin: false,
                read_only: false,
                truecolor: None,
            }
        );
    }

    #[test]
    fn accepts_help_flags() {
        assert_eq!(parse_args(args(&["-h"])).unwrap(), Command::Help);
        assert_eq!(parse_args(args(&["--help"])).unwrap(), Command::Help);
    }

    #[test]
    fn help_documents_preview_and_gateway_override() {
        assert!(STANDARD.usage.contains("Space f"));
        assert!(STANDARD.usage.contains("previews"));
        assert!(STANDARD.usage.contains("ANVIL_GATEWAY"));
        assert!(USAGE.contains("ANVIL_EDITOR_MAX_BUFFERS"));
        assert!(USAGE.contains("Space b"));
    }

    #[test]
    fn accepts_version_flags() {
        assert_eq!(parse_args(args(&["-V"])).unwrap(), Command::Version);
        assert_eq!(parse_args(args(&["--version"])).unwrap(), Command::Version);
    }

    #[test]
    fn rejects_unknown_option() {
        let error = parse_args(args(&["--wat"])).unwrap_err();
        assert!(error.contains("unknown option"));
    }

    #[test]
    fn rejects_missing_workspace_value() {
        let error = parse_args(args(&["--workspace"])).unwrap_err();
        assert!(error.contains("requires a directory"));
    }

    #[test]
    fn rejects_extra_files() {
        let error = parse_args(args(&["one", "two"])).unwrap_err();
        assert!(error.contains("only one file"));
    }
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
        use anvil_module::language::LanguageToolingService;
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
        assert!(FULL.git);
        assert!(FULL.workspace_index);
        assert!(FULL.desktop);
        assert!(FULL.language_latex);
        assert!(FULL.language_typst);
        assert!(compose_workspace_index(&FULL).expect("index").is_some());
        assert!(compose_desktop(&FULL).expect("desktop").is_some());
        assert!(compose_language_tooling(&FULL).expect("language").is_some());
    }

    #[test]
    fn parses_doctor_and_json_flag() {
        assert_eq!(
            parse_args(args(&["doctor"])).unwrap(),
            Command::Doctor {
                json: false,
                profile: DoctorProfileChoice::Auto,
            }
        );
        assert_eq!(
            parse_args(args(&["doctor", "--json"])).unwrap(),
            Command::Doctor {
                json: true,
                profile: DoctorProfileChoice::Auto,
            }
        );
        assert_eq!(
            parse_args(args(&["doctor", "--profile", "installed"])).unwrap(),
            Command::Doctor {
                json: false,
                profile: DoctorProfileChoice::Installed,
            }
        );
        assert_eq!(
            parse_args(args(&["doctor", "--profile=dev"])).unwrap(),
            Command::Doctor {
                json: false,
                profile: DoctorProfileChoice::Dev,
            }
        );
        assert!(parse_args(args(&["doctor", "--profile", "broken"])).is_err());
        assert!(parse_args(args(&["doctor", "--profile"])).is_err());
        assert!(parse_args(args(&["doctor", "--wat"])).is_err());
        assert_eq!(
            parse_args(args(&["doctor", "--help"])).unwrap(),
            Command::Help
        );
    }

    #[test]
    fn doctor_json_has_stable_schema() {
        assert_eq!(json_escape("a\"b\nc"), "a\\\"b\\nc");
        let checks = vec![check(
            "gateway",
            DoctorLevel::Error,
            "missing \"gateway\"",
            "install it",
        )];
        let output = doctor_json(&STANDARD, DoctorProfile::Installed, &checks, false);
        assert!(output.contains("\"schema_version\":1"));
        assert!(output.contains(&format!(
            "\"anvil_version\":\"{}\"",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(output.contains("\"profile\":\"installed\""));
        assert!(output.contains("\"distribution\":\"anvil\""));
        assert!(output.contains("\"ok\":false"));
        assert!(output.contains("\"id\":\"gateway\""));
        assert!(output.contains("\"message\":\"missing \\\"gateway\\\"\""));
        assert!(output.contains("\"remediation\":\"install it\""));
    }

    #[test]
    fn dev_profile_downgrades_missing_sidecars() {
        let directory = tempfile::tempdir().expect("tempdir");
        let dev = check_companion(
            "anvil-exec-runner",
            DoctorProfile::Dev,
            Some(directory.path()),
            false,
        );
        assert_eq!(dev.level, DoctorLevel::Warn);
        let installed = check_companion(
            "anvil-exec-runner",
            DoctorProfile::Installed,
            Some(directory.path()),
            true,
        );
        assert_eq!(installed.level, DoctorLevel::Error);
    }

    #[test]
    fn doctor_exit_code_tracks_required_errors() {
        let directory = tempfile::tempdir().expect("tempdir");
        let dev = vec![check_companion(
            "gateway",
            DoctorProfile::Dev,
            Some(directory.path()),
            false,
        )];
        assert_eq!(doctor_exit_code(&dev), 0);
        let installed = vec![check_companion(
            "gateway",
            DoctorProfile::Installed,
            Some(directory.path()),
            true,
        )];
        assert_eq!(doctor_exit_code(&installed), 1);
    }

    #[test]
    fn installed_profile_accepts_fake_sidecars_in_json() {
        let directory = tempfile::tempdir().expect("tempdir");
        let names = [
            "anvil-gateway",
            "anvil-exec-runner",
            "anvil-plugin-host",
            "anvil-ctl",
            "anvil-mcp",
        ];
        for name in names {
            let path = directory.path().join(name);
            fs::write(&path, "").expect("sidecar");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("permissions");
            }
        }
        let checks = vec![
            check_binary(
                "gateway",
                DoctorProfile::Installed,
                &directory.path().join("anvil-gateway"),
                true,
            ),
            check_companion(
                "anvil-exec-runner",
                DoctorProfile::Installed,
                Some(directory.path()),
                true,
            ),
            check_companion(
                "anvil-plugin-host",
                DoctorProfile::Installed,
                Some(directory.path()),
                false,
            ),
            check_companion(
                "anvil-ctl",
                DoctorProfile::Installed,
                Some(directory.path()),
                false,
            ),
            check_companion(
                "anvil-mcp",
                DoctorProfile::Installed,
                Some(directory.path()),
                false,
            ),
        ];
        assert!(checks.iter().all(|check| check.level == DoctorLevel::Ok));
        assert_eq!(doctor_exit_code(&checks), 0);
        let json = doctor_json(&STANDARD, DoctorProfile::Installed, &checks, true);
        assert!(json.contains("\"schema_version\":1"));
        assert!(json.contains("\"profile\":\"installed\""));
        assert!(json.contains("\"remediation\":\"\""));
    }

    #[test]
    fn remote_check_distinguishes_ssh_capabilities() {
        assert_eq!(
            check_remote_session(false, "xterm-256color", "", false).level,
            DoctorLevel::Ok
        );
        assert_eq!(
            check_remote_session(true, "xterm-256color", "", false).level,
            DoctorLevel::Warn
        );
        assert_eq!(
            check_remote_session(true, "xterm-direct", "", false).level,
            DoctorLevel::Ok
        );
        assert_eq!(
            check_remote_session(true, "xterm-direct", "truecolor", true).level,
            DoctorLevel::Warn
        );
    }
}

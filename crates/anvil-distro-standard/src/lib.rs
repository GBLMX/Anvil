use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process,
};

const USAGE: &str = "\
anvil — a terminal editor with a selection-first Rust kernel

Usage:
  anvil [OPTIONS] [FILE|-]

Options:
  -w, --workspace <DIR>  Workspace directory; --workspace=DIR is also accepted
  --editor              External-editor mode; allows one explicit file outside
                        the cwd by using its parent as the default workspace
  --read-only           Reject mutating commands
  -h, --help             Show this help and exit
  -V, --version          Show the version and exit
  --                     Treat the next argument as FILE

Environment:
  ANVIL_GATEWAY          Explicit path to anvil-gateway
  ANVIL_RG               Explicit ripgrep path for fuzzy file listing
  ANVIL_THEME            Startup theme: anvil-dark, tokyonight, omp-kde,
                         helix-base16, one-dark, gruvbox-dark, nord,
                         catppuccin-mocha, solarized-light
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
}

/// The standard Anvil distribution.
pub const STANDARD: Distribution = Distribution {
    name: "anvil",
    usage: USAGE,
    git: true,
    workspace_index: false,
    desktop: false,
    language_latex: false,
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
    },
    Help,
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
fn compose_language_tooling(
    distribution: &Distribution,
) -> Result<Option<std::sync::Arc<dyn anvil_module::language::LanguageToolingService>>, RunError> {
    if !distribution.language_latex {
        return Ok(None);
    }
    let mut registry = anvil_module::ModuleRegistry::new();
    registry
        .register(&anvil_mod_language_latex::LatexModule)
        .map_err(|error| RunError::Args(error.to_string()))?;
    Ok(registry.language_tooling_service())
}

#[cfg(not(feature = "official-language-latex"))]
fn compose_language_tooling(
    distribution: &Distribution,
) -> Result<Option<std::sync::Arc<dyn anvil_module::language::LanguageToolingService>>, RunError> {
    if distribution.language_latex {
        return Err(RunError::Args(
            "distribution requires official-language-latex, but the feature is not compiled"
                .to_owned(),
        ));
    }
    Ok(None)
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
        Ok(Command::Run {
            workspace,
            path,
            editor,
            stdin,
            read_only,
        }) => {
            if !stdin
                && !editor
                && forward_to_running(distribution, &workspace, path.as_deref(), read_only)?
            {
                return Ok(0);
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
        Err(_) => {
            let mut legacy = match anvil_control::Client::connect() {
                Ok(client) => client,
                Err(_) => return Ok(false),
            };
            match legacy.status() {
                Ok(value) => {
                    let running = value
                        .workspace
                        .canonicalize()
                        .unwrap_or_else(|_| value.workspace.clone());
                    if running != workspace {
                        return Ok(false);
                    }
                    client = legacy;
                    value
                }
                Err(_) => return Ok(false),
            }
        }
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
    let mut arguments = arguments.into_iter();
    let mut workspace = None;
    let mut path = None;
    let mut editor = false;
    let mut stdin = false;
    let mut read_only = false;

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
    })
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
            }
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
        assert!(parse_args(args(&["-", "file.rs"]))
            .unwrap_err()
            .contains("mutually exclusive"));
        assert!(parse_args(args(&["--editor", "-"]))
            .unwrap_err()
            .contains("cannot be combined"));
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
        assert!(compose_workspace_index(&with_index)
            .expect("compose")
            .is_some());
        assert!(compose_workspace_index(&STANDARD)
            .expect("compose")
            .is_none());
    }

    #[cfg(not(feature = "official-workspace-index"))]
    #[test]
    fn missing_workspace_index_feature_is_a_startup_error() {
        let with_index = Distribution {
            workspace_index: true,
            ..STANDARD
        };
        assert!(compose_workspace_index(&with_index).is_err());
        assert!(compose_workspace_index(&STANDARD)
            .expect("compose")
            .is_none());
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
        assert!(compose_language_tooling(&with_latex)
            .expect("compose")
            .is_some());
        assert!(compose_language_tooling(&STANDARD)
            .expect("compose")
            .is_none());
    }

    #[cfg(not(feature = "official-language-latex"))]
    #[test]
    fn missing_latex_feature_is_a_startup_error() {
        let with_latex = Distribution {
            language_latex: true,
            ..STANDARD
        };
        assert!(compose_language_tooling(&with_latex).is_err());
        assert!(compose_language_tooling(&STANDARD)
            .expect("compose")
            .is_none());
    }
}

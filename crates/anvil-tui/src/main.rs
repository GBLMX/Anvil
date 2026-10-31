use std::{env, ffi::OsString, path::PathBuf, process};

const USAGE: &str = "\
anvil — a terminal editor with a selection-first Rust kernel

Usage:
  anvil [OPTIONS] [FILE]

Options:
  -w, --workspace <DIR>  Workspace directory (default: current directory)
  -h, --help             Show this help and exit
  -V, --version          Show the version and exit
  --                     Treat the next argument as FILE

FILE must be inside the selected workspace. Use -- before a file name that
starts with a dash.
";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Run {
        workspace: PathBuf,
        path: Option<PathBuf>,
    },
    Help,
    Version,
}

fn main() -> std::io::Result<()> {
    match parse_args(env::args_os().skip(1)) {
        Ok(Command::Help) => {
            print!("{USAGE}");
            Ok(())
        }
        Ok(Command::Version) => {
            println!("anvil {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Ok(Command::Run { workspace, path }) => anvil_tui::run_with_workspace(path, workspace),
        Err(error) => {
            eprintln!("anvil: {error}");
            process::exit(2);
        }
    }
}

fn parse_args<I>(arguments: I) -> Result<Command, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = arguments.into_iter();
    let mut workspace = None;
    let mut path = None;

    while let Some(argument) = arguments.next() {
        if argument == "--help" || argument == "-h" {
            return Ok(Command::Help);
        }
        if argument == "--version" || argument == "-V" {
            return Ok(Command::Version);
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

    let workspace = match workspace {
        Some(workspace) => workspace,
        None => env::current_dir().map_err(|error| error.to_string())?,
    };
    Ok(Command::Run { workspace, path })
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
            }
        );
    }

    #[test]
    fn accepts_help_flags() {
        assert_eq!(parse_args(args(&["-h"])).unwrap(), Command::Help);
        assert_eq!(parse_args(args(&["--help"])).unwrap(), Command::Help);
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
}

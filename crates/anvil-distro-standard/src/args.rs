//! Command-line parsing for the standard distribution.

use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::doctor::DoctorProfileChoice;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Command {
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
    Modules {
        json: bool,
    },
    Version,
}

pub(super) fn parse_args<I>(arguments: I) -> Result<Command, String>
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
    if arguments
        .peek()
        .is_some_and(|argument| argument == "modules")
    {
        arguments.next();
        return parse_modules_args(arguments);
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
    if path.as_deref().is_some_and(Path::is_dir) {
        if workspace.is_some() {
            return Err("--workspace cannot be combined with a directory path".to_owned());
        }
        let directory = path.take().expect("directory path");
        workspace = Some(directory.canonicalize().unwrap_or(directory));
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

fn parse_modules_args<I>(arguments: I) -> Result<Command, String>
where
    I: Iterator<Item = OsString>,
{
    let mut json = false;
    for argument in arguments {
        match argument.to_str() {
            Some("--json") => json = true,
            Some("--help" | "-h") => return Ok(Command::Help),
            _ => {
                return Err(format!(
                    "unknown modules option: {}",
                    argument.to_string_lossy()
                ));
            }
        }
    }
    Ok(Command::Modules { json })
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
    use crate::{STANDARD, USAGE};

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
    fn directory_argument_opens_as_workspace() {
        let directory = tempfile::tempdir().expect("tempdir");
        let command = parse_args(args(&[directory.path().to_str().expect("utf-8 path")])).unwrap();
        let Command::Run {
            workspace, path, ..
        } = command
        else {
            panic!("expected run command");
        };
        assert_eq!(
            workspace,
            directory.path().canonicalize().expect("canonical")
        );
        assert_eq!(path, None);
    }

    #[test]
    fn directory_argument_rejects_explicit_workspace() {
        let directory = tempfile::tempdir().expect("tempdir");
        let error = parse_args(args(&[
            "--workspace",
            directory.path().to_str().expect("utf-8 path"),
            directory.path().to_str().expect("utf-8 path"),
        ]))
        .expect_err("directory path");
        assert!(error.contains("directory path"), "{error}");
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
    fn parses_modules_json_and_rejects_unknown_options() {
        assert_eq!(
            parse_args(args(&["modules"])).unwrap(),
            Command::Modules { json: false }
        );
        assert_eq!(
            parse_args(args(&["modules", "--json"])).unwrap(),
            Command::Modules { json: true }
        );
        assert!(parse_args(args(&["modules", "--wat"])).is_err());
        assert_eq!(
            parse_args(args(&["modules", "--help"])).unwrap(),
            Command::Help
        );
    }
}

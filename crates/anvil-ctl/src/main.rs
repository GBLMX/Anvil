use anvil_control::{
    Client, EditMode, EditRange, Error as ControlError, QueryOptions, TextPoint,
    validate_remote_command,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process,
    time::{Duration, Instant},
};

const USAGE: &str = "\
anvil-ctl - control a running Anvil instance

Usage:
  anvil-ctl [OPTIONS] status
  anvil-ctl [OPTIONS] open <FILE> [+LINE[:COLUMN]]
  anvil-ctl [OPTIONS] save [FILE]
  anvil-ctl [OPTIONS] edit set <TEXT|--stdin>
  anvil-ctl [OPTIONS] edit insert <TEXT|--stdin>
  anvil-ctl [OPTIONS] edit replace <START_LINE:START_BYTE:END_LINE:END_BYTE> <TEXT|--stdin> --expected-revision <N>
  anvil-ctl [OPTIONS] query [START[:END]]
  anvil-ctl [OPTIONS] command <COMMAND>
  anvil-ctl [OPTIONS] quit [--force]

Commands:
  status    Print the running editor status
  open      Open a workspace file at a 1-based line and column
  save      Save the current buffer, optionally as FILE
  edit      Set, insert, or replace a range; --stdin reads the payload
  query     Dump bounded buffer state; default is the whole buffer
  command   Execute an Ex command through the editor state machine (alias: exec)
  quit      Quit; --force discards unsaved changes

Options:
  --json              Print one JSON object with ok/data or error
  --timeout-ms <MS>   Socket timeout (default: 5000)
  --workspace <DIR>   Select the Anvil instance for DIR (default: cwd)
  --spawn             Start Anvil in the current terminal if none is running
  --expected-revision <N>  Required revision guard for edit replace
  -h, --help          Show this help

Exit codes:
  0 success, 1 internal/local error, 2 usage, 3 no instance,
  4 timeout, 5 editor RPC error

The control socket is selected by the canonical workspace path under
$XDG_RUNTIME_DIR/anvil/, or ~/.local/state/anvil/runtime/anvil/ when
XDG_RUNTIME_DIR is unset.
";

#[derive(Debug)]
struct Options {
    json: bool,
    spawn: bool,
    timeout: Duration,
    workspace: PathBuf,
    positional: Vec<String>,
}

#[derive(Debug)]
struct CliError {
    code: i32,
    kind: &'static str,
    message: String,
    rpc_code: Option<i32>,
}

impl CliError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            kind: "usage",
            message: message.into(),
            rpc_code: None,
        }
    }

    fn local(message: impl Into<String>) -> Self {
        Self {
            code: 1,
            kind: "local",
            message: message.into(),
            rpc_code: None,
        }
    }

    fn no_instance(message: impl Into<String>) -> Self {
        Self {
            code: 3,
            kind: "no_instance",
            message: message.into(),
            rpc_code: None,
        }
    }

    fn timeout(message: impl Into<String>) -> Self {
        Self {
            code: 4,
            kind: "timeout",
            message: message.into(),
            rpc_code: None,
        }
    }
}

fn main() {
    match run() {
        Ok(()) => {}
        Err((options, error)) => {
            if options.json {
                eprintln!(
                    "{}",
                    json!({
                        "ok": false,
                        "error": {
                            "exit_code": error.code,
                            "kind": error.kind,
                            "message": error.message,
                            "rpc_code": error.rpc_code,
                        }
                    })
                );
            } else {
                eprintln!("anvil-ctl: {}", error.message);
            }
            process::exit(error.code);
        }
    }
}

enum EditText {
    Inline(String),
    Stdin,
}
enum CliCommand {
    Status,
    Open {
        file: PathBuf,
        line: usize,
        column: usize,
    },
    Save {
        path: Option<PathBuf>,
    },
    Edit {
        mode: EditMode,
        text: EditText,
        range: Option<EditRange>,
        expected_revision: Option<u64>,
    },
    Query(QueryOptions),
    Command(String),
    Quit {
        force: bool,
    },
}

impl CliCommand {
    fn name(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Open { .. } => "open",
            Self::Save { .. } => "save",
            Self::Edit { .. } => "edit",
            Self::Query(_) => "query",
            Self::Command(_) => "command",
            Self::Quit { .. } => "quit",
        }
    }
}

#[allow(clippy::result_large_err)]
fn run() -> Result<(), (Options, CliError)> {
    let options = parse_options().map_err(|error| {
        (
            Options {
                json: std::env::args().any(|argument| argument == "--json"),
                spawn: false,
                timeout: Duration::from_secs(5),
                workspace: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                positional: Vec::new(),
            },
            error,
        )
    })?;
    if options.positional.is_empty() {
        if !options.json {
            print!("{USAGE}");
        }
        return Err((options, CliError::usage("a command is required")));
    }
    match options.positional[0].as_str() {
        "-h" | "--help" => {
            print!("{USAGE}");
            Ok(())
        }
        "--version" => {
            println!("anvil-ctl {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            let command =
                parse_command(&options).map_err(|error| (clone_options(&options), error))?;
            execute(options, command)
        }
    }
}

fn parse_command(options: &Options) -> Result<CliCommand, CliError> {
    let name = options.positional[0].as_str();
    let arguments = &options.positional[1..];
    match name {
        "status" => {
            require_no_args(arguments)?;
            Ok(CliCommand::Status)
        }
        "open" => {
            let Some(file) = arguments.first() else {
                return Err(CliError::usage("open requires a file"));
            };
            if arguments.len() > 2 {
                return Err(CliError::usage("open takes at most two arguments"));
            }
            let (line, column) = match arguments.get(1) {
                Some(location) => parse_location(location)?,
                None => (1, 1),
            };
            Ok(CliCommand::Open {
                file: PathBuf::from(file),
                line,
                column,
            })
        }
        "save" => {
            if arguments.len() > 1 {
                return Err(CliError::usage("save takes at most one path"));
            }
            Ok(CliCommand::Save {
                path: arguments.first().map(PathBuf::from),
            })
        }
        "edit" => {
            let Some(mode) = arguments.first() else {
                return Err(CliError::usage("edit requires set, insert, or replace"));
            };
            match mode.as_str() {
                "set" | "insert" => {
                    let mode = if mode == "set" {
                        EditMode::Set
                    } else {
                        EditMode::Insert
                    };
                    let Some(text) = arguments.get(1) else {
                        return Err(CliError::usage("edit requires text or --stdin"));
                    };
                    if arguments.len() > 2 {
                        return Err(CliError::usage("edit takes mode and text"));
                    }
                    Ok(CliCommand::Edit {
                        mode,
                        text: edit_text(text),
                        range: None,
                        expected_revision: None,
                    })
                }
                "replace" => {
                    let Some(range) = arguments.get(1) else {
                        return Err(CliError::usage(
                            "edit replace requires START_LINE:START_BYTE:END_LINE:END_BYTE",
                        ));
                    };
                    let Some(text) = arguments.get(2) else {
                        return Err(CliError::usage("edit replace requires text or --stdin"));
                    };
                    Ok(CliCommand::Edit {
                        mode: EditMode::Replace,
                        text: edit_text(text),
                        range: Some(parse_edit_range(range)?),
                        expected_revision: Some(parse_expected_revision(&arguments[3..])?),
                    })
                }
                _ => Err(CliError::usage("edit mode must be set, insert, or replace")),
            }
        }
        "query" => Ok(CliCommand::Query(parse_query_options(arguments)?)),
        "command" | "exec" => {
            if arguments.is_empty() {
                return Err(CliError::usage("command requires a command"));
            }
            let command = arguments.join(" ");
            validate_remote_command(&command)
                .map_err(|error| CliError::usage(error.to_string()))?;
            Ok(CliCommand::Command(command))
        }
        "quit" => {
            let force = match arguments {
                [] => false,
                [flag] if flag == "--force" => true,
                _ => return Err(CliError::usage("quit accepts only --force")),
            };
            Ok(CliCommand::Quit { force })
        }
        _ => Err(CliError::usage(format!("unknown command: {name}"))),
    }
}

fn edit_text(value: &str) -> EditText {
    if value == "--stdin" {
        EditText::Stdin
    } else {
        EditText::Inline(value.to_owned())
    }
}

fn parse_edit_range(value: &str) -> Result<EditRange, CliError> {
    let parts = value.split(':').collect::<Vec<_>>();
    let [start_line, start_byte, end_line, end_byte] = parts.as_slice() else {
        return Err(CliError::usage(
            "edit range must be START_LINE:START_BYTE:END_LINE:END_BYTE",
        ));
    };
    Ok(EditRange {
        start: TextPoint {
            line: parse_positive(start_line, "start line")?,
            byte_column: parse_positive(start_byte, "start byte")?,
        },
        end: TextPoint {
            line: parse_positive(end_line, "end line")?,
            byte_column: parse_positive(end_byte, "end byte")?,
        },
    })
}

fn parse_positive(value: &str, label: &str) -> Result<usize, CliError> {
    let value = value
        .parse::<usize>()
        .map_err(|_| CliError::usage(format!("invalid {label}: {value}")))?;
    if value == 0 {
        return Err(CliError::usage(format!("{label} must be 1-based")));
    }
    Ok(value)
}

fn parse_expected_revision(arguments: &[String]) -> Result<u64, CliError> {
    match arguments {
        [flag, value] if flag == "--expected-revision" => parse_revision(value),
        [flag] => match flag.strip_prefix("--expected-revision=") {
            Some(value) => parse_revision(value),
            None => Err(CliError::usage(
                "edit replace requires --expected-revision <N>",
            )),
        },
        _ => Err(CliError::usage(
            "edit replace requires --expected-revision <N>",
        )),
    }
}

fn parse_revision(value: &str) -> Result<u64, CliError> {
    value
        .parse::<u64>()
        .map_err(|_| CliError::usage(format!("invalid expected revision: {value}")))
}
#[allow(clippy::result_large_err)]
fn execute(options: Options, command: CliCommand) -> Result<(), (Options, CliError)> {
    let mut client = Client::connect_for_with_timeout(&options.workspace, options.timeout)
        .map_err(|error| (clone_options(&options), map_error(error)))?;
    if let Err(error) = client.ping() {
        if !options.spawn {
            return Err((
                clone_options(&options),
                CliError::no_instance(format!("no running Anvil instance: {error}")),
            ));
        }
        spawn_anvil(&options.workspace, options.timeout)
            .map_err(|error| (clone_options(&options), error))?;
    }
    let name = command.name();
    let result = match command {
        CliCommand::Status => {
            let status = client
                .status()
                .map_err(|error| (clone_options(&options), map_error(error)))?;
            let value = serde_json::to_value(&status)
                .map_err(|error| (clone_options(&options), CliError::local(error.to_string())))?;
            if options.json {
                print_json("status", value);
            } else {
                println!("version: {}", status.version);
                println!("workspace: {}", status.workspace.display());
                println!(
                    "file: {}",
                    status
                        .file
                        .as_deref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "-".to_owned())
                );
                println!("dirty: {}", status.dirty);
            }
            return Ok(());
        }
        CliCommand::Open { file, line, column } => json!(
            client
                .open(&file, line, column)
                .map_err(|error| (clone_options(&options), map_error(error)))?
        ),
        CliCommand::Save { path } => json!(
            client
                .save(path.as_deref())
                .map_err(|error| (clone_options(&options), map_error(error)))?
        ),
        CliCommand::Edit {
            mode,
            text,
            range,
            expected_revision,
        } => {
            let text = match text {
                EditText::Inline(text) => text,
                EditText::Stdin => {
                    read_stdin_text().map_err(|error| (clone_options(&options), error))?
                }
            };
            let result = match (mode, range, expected_revision) {
                (EditMode::Replace, Some(range), Some(expected_revision)) => client
                    .replace(range, &text, expected_revision)
                    .map_err(|error| (clone_options(&options), map_error(error)))?,
                _ => client
                    .edit_with_revision(mode, &text, expected_revision)
                    .map_err(|error| (clone_options(&options), map_error(error)))?,
            };
            json!(result)
        }
        CliCommand::Query(query) => json!(
            client
                .query_with(query)
                .map_err(|error| (clone_options(&options), map_error(error)))?
        ),
        CliCommand::Command(command) => json!(
            client
                .exec(&command)
                .map_err(|error| (clone_options(&options), map_error(error)))?
        ),
        CliCommand::Quit { force } => json!(
            client
                .quit(force)
                .map_err(|error| (clone_options(&options), map_error(error)))?
        ),
    };
    if options.json {
        print_json(name, result);
    } else {
        print_human(name, &result);
    }
    Ok(())
}

fn print_json(command: &str, data: Value) {
    println!(
        "{}",
        json!({
            "ok": true,
            "command": command,
            "data": data,
        })
    );
}

fn print_human(command: &str, data: &Value) {
    match command {
        "query" => {
            if let Some(text) = data.get("text").and_then(Value::as_str) {
                print!("{text}");
            }
        }
        "open" | "save" => {
            if let Some(path) = data.get("path").and_then(Value::as_str) {
                println!("{command}: {path}");
            } else {
                println!("{command}: ok");
            }
        }
        _ => {
            if let Some(status) = data.get("status").and_then(Value::as_str) {
                println!("{status}");
            } else {
                println!("{command}: ok");
            }
        }
    }
}

fn require_no_args(arguments: &[String]) -> Result<(), CliError> {
    if arguments.is_empty() {
        Ok(())
    } else {
        Err(CliError::usage("command takes no arguments"))
    }
}

fn parse_options() -> Result<Options, CliError> {
    let mut json = false;
    let mut spawn = false;
    let mut timeout = Duration::from_secs(5);
    let mut workspace =
        std::env::current_dir().map_err(|error| CliError::local(error.to_string()))?;
    let mut positional = Vec::new();
    let mut args = std::env::args().skip(1).peekable();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--json" => json = true,
            "--spawn" => spawn = true,
            "--timeout-ms" => {
                let value = args
                    .next()
                    .ok_or_else(|| CliError::usage("--timeout-ms requires a value"))?;
                timeout = parse_timeout(&value)?;
            }
            "--workspace" => {
                workspace = PathBuf::from(
                    args.next()
                        .ok_or_else(|| CliError::usage("--workspace requires a directory"))?,
                );
            }
            "--expected-revision" => {
                positional.push(argument);
                positional.push(
                    args.next()
                        .ok_or_else(|| CliError::usage("--expected-revision requires a value"))?,
                );
            }
            value if value.starts_with("--expected-revision=") => positional.push(argument),
            value if value.starts_with("--timeout-ms=") => {
                timeout = parse_timeout(&value["--timeout-ms=".len()..])?;
            }
            value if value.starts_with("--workspace=") => {
                workspace = PathBuf::from(&value["--workspace=".len()..]);
            }
            "-h" | "--help" => positional.push(argument),
            "--force" => positional.push(argument),
            "--version" => positional.push(argument),
            value if value.starts_with('-') => {
                return Err(CliError::usage(format!("unknown option: {value}")));
            }
            _ => positional.push(argument),
        }
    }
    Ok(Options {
        json,
        spawn,
        timeout,
        workspace,
        positional,
    })
}

fn parse_timeout(value: &str) -> Result<Duration, CliError> {
    let millis = value
        .parse::<u64>()
        .map_err(|_| CliError::usage(format!("invalid timeout: {value}")))?;
    if millis == 0 {
        return Err(CliError::usage("timeout must be positive"));
    }
    Ok(Duration::from_millis(millis))
}

fn parse_location(value: &str) -> Result<(usize, usize), CliError> {
    let value = value
        .strip_prefix('+')
        .ok_or_else(|| CliError::usage("location must start with '+'"))?;
    let (line, column) = value.split_once(':').unwrap_or((value, "1"));
    let line = line
        .parse::<usize>()
        .map_err(|_| CliError::usage(format!("invalid line: {line}")))?;
    let column = column
        .parse::<usize>()
        .map_err(|_| CliError::usage(format!("invalid column: {column}")))?;
    if line == 0 || column == 0 {
        return Err(CliError::usage("line and column must be 1-based"));
    }
    Ok((line, column))
}

fn parse_query_options(arguments: &[String]) -> Result<QueryOptions, CliError> {
    let [range] = arguments else {
        if arguments.is_empty() {
            return Ok(QueryOptions::default());
        }
        return Err(CliError::usage("query takes at most one range"));
    };
    let (start, end) = range.split_once(':').unwrap_or((range, ""));
    let start = parse_line_number(start)?;
    let end = if end.is_empty() {
        None
    } else {
        Some(parse_line_number(end)?)
    };
    if end.is_some_and(|end| end < start) {
        return Err(CliError::usage("query end must not precede start"));
    }
    Ok(QueryOptions {
        start_line: Some(start),
        end_line: end,
        include_text: true,
    })
}

fn parse_line_number(value: &str) -> Result<usize, CliError> {
    let line = value
        .parse::<usize>()
        .map_err(|_| CliError::usage(format!("invalid line: {value}")))?;
    if line == 0 {
        return Err(CliError::usage("line must be 1-based"));
    }
    Ok(line)
}

const MAX_STDIN_BYTES: usize = 4 * 1024 * 1024;

fn read_stdin_text() -> Result<String, CliError> {
    read_stdin_text_from(std::io::stdin().lock())
}

fn read_stdin_text_from<R: std::io::Read>(reader: R) -> Result<String, CliError> {
    use std::io::Read;
    let mut text = String::new();
    reader
        .take(MAX_STDIN_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|error| CliError::local(format!("stdin: {error}")))?;
    if text.len() > MAX_STDIN_BYTES {
        return Err(CliError::usage("stdin edit exceeds 4 MiB"));
    }
    Ok(text)
}

fn spawn_anvil(workspace: &Path, timeout: Duration) -> Result<(), CliError> {
    use std::io::IsTerminal;

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(CliError::usage("--spawn requires a terminal"));
    }
    let binary = std::env::var_os("ANVIL_BIN").unwrap_or_else(|| "anvil".into());
    let mut child = std::process::Command::new(binary)
        .arg("--workspace")
        .arg(workspace)
        .spawn()
        .map_err(|error| CliError::local(format!("cannot spawn Anvil: {error}")))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(CliError::no_instance(format!(
                    "spawned Anvil exited prematurely: {status}"
                )));
            }
            Ok(None) => {}
            Err(error) => {
                return Err(CliError::local(format!(
                    "cannot inspect spawned Anvil: {error}"
                )));
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let attempt = remaining.min(Duration::from_millis(100));
        if let Ok(mut client) = Client::connect_for_with_timeout(workspace, attempt)
            && client.ping().is_ok()
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50).min(remaining));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(CliError::no_instance(format!(
        "spawned Anvil did not start its control socket within {} ms",
        timeout.as_millis()
    )))
}

fn map_error(error: ControlError) -> CliError {
    match error {
        ControlError::Timeout => CliError::timeout("control request timed out"),
        ControlError::Rpc { code, message } => CliError {
            code: 5,
            kind: "rpc",
            message,
            rpc_code: Some(code),
        },
        ControlError::RuntimeDir(message) => CliError::no_instance(message),
        ControlError::AlreadyRunning | ControlError::Lock(_) => {
            CliError::local("control socket is busy")
        }
        error => CliError::local(error.to_string()),
    }
}

fn clone_options(options: &Options) -> Options {
    Options {
        json: options.json,
        spawn: options.spawn,
        timeout: options.timeout,
        workspace: options.workspace.clone(),
        positional: options.positional.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_locations() {
        assert_eq!(parse_location("+12").unwrap(), (12, 1));
        assert_eq!(parse_location("+12:3").unwrap(), (12, 3));
        assert!(parse_location("12").is_err());
        assert!(parse_location("+0:1").is_err());
        assert!(parse_location("+1:0").is_err());
    }

    #[test]
    fn parses_timeout() {
        assert_eq!(parse_timeout("25").unwrap(), Duration::from_millis(25));
        assert!(parse_timeout("0").is_err());
        assert!(parse_timeout("no").is_err());
        assert!(parse_timeout("abc").is_err());
        assert!(parse_timeout("-1").is_err());
    }

    fn options(positional: &[&str]) -> Options {
        Options {
            json: true,
            spawn: false,
            timeout: Duration::from_secs(1),
            workspace: PathBuf::from("."),
            positional: positional.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn validates_commands_before_connecting() {
        assert!(parse_command(&options(&["bogus"])).is_err());
        assert!(parse_command(&options(&["open"])).is_err());
        assert!(parse_command(&options(&["edit", "replace", "x"])).is_err());
        assert!(parse_command(&options(&["command", ":pkg", "install"])).is_err());
        assert!(parse_command(&options(&["command", ":git-diff"])).is_ok());
        assert!(parse_command(&options(&["status", "open", "file.rs"])).is_err());
        assert!(matches!(
            parse_command(&options(&["edit", "set", " "])).unwrap(),
            CliCommand::Edit {
                mode: EditMode::Set,
                text: EditText::Inline(text),
                ..
            } if text == " "
        ));
        assert!(matches!(
            parse_command(&options(&["edit", "set", "--stdin"])).unwrap(),
            CliCommand::Edit {
                mode: EditMode::Set,
                text: EditText::Stdin,
                ..
            }
        ));
        assert!(matches!(
            parse_command(&options(&["quit", "--force"])).unwrap(),
            CliCommand::Quit { force: true }
        ));
    }

    #[test]
    fn parses_replace_ranges_and_revisions() {
        let parsed = parse_command(&options(&[
            "edit",
            "replace",
            "1:7:1:11",
            "gamma",
            "--expected-revision",
            "9",
        ]))
        .expect("replace");
        assert!(matches!(
            parsed,
            CliCommand::Edit {
                mode: EditMode::Replace,
                text: EditText::Inline(text),
                range: Some(EditRange {
                    start: TextPoint {
                        line: 1,
                        byte_column: 7
                    },
                    end: TextPoint {
                        line: 1,
                        byte_column: 11
                    }
                }),
                expected_revision: Some(9),
            } if text == "gamma"
        ));
        assert!(parse_command(&options(&["edit", "replace", "1:7:1:11", "x"])).is_err());
        assert!(
            parse_command(&options(&[
                "edit",
                "replace",
                "1:0:1:11",
                "x",
                "--expected-revision",
                "1",
            ]))
            .is_err()
        );
    }

    #[test]
    fn limits_stdin_payload_before_reading_unbounded_data() {
        let exact = vec![b'a'; MAX_STDIN_BYTES];
        assert_eq!(
            read_stdin_text_from(Cursor::new(exact))
                .expect("exact limit")
                .len(),
            MAX_STDIN_BYTES
        );
        let over = vec![b'a'; MAX_STDIN_BYTES + 1];
        assert!(read_stdin_text_from(Cursor::new(over)).is_err());
    }

    #[test]
    fn parses_bounded_query() {
        assert!(matches!(
            parse_command(&options(&["query", "2:3"])).unwrap(),
            CliCommand::Query(QueryOptions {
                start_line: Some(2),
                end_line: Some(3),
                include_text: true,
            })
        ));
        assert!(parse_command(&options(&["query", "0:1"])).is_err());
        assert!(parse_command(&options(&["query", "3:2"])).is_err());
    }

    #[test]
    fn maps_control_errors_to_exit_codes() {
        assert_eq!(map_error(ControlError::Timeout).code, 4);
        assert_eq!(
            map_error(ControlError::Rpc {
                code: -32002,
                message: "denied".to_owned(),
            })
            .code,
            5
        );
        assert_eq!(
            map_error(ControlError::RuntimeDir("missing".to_owned())).code,
            3
        );
    }
}

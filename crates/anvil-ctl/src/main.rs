use anvil_control::{
    validate_remote_command, Client, EditMode, Error as ControlError, QueryOptions,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process,
    time::Duration,
};

const USAGE: &str = "\
anvil-ctl - control a running Anvil instance

Usage:
  anvil-ctl [OPTIONS] status
  anvil-ctl [OPTIONS] open <FILE> [+LINE[:COLUMN]]
  anvil-ctl [OPTIONS] save [FILE]
  anvil-ctl [OPTIONS] edit set <TEXT|--stdin>
  anvil-ctl [OPTIONS] edit insert <TEXT|--stdin>
  anvil-ctl [OPTIONS] query [START[:END]]
  anvil-ctl [OPTIONS] command <COMMAND>
  anvil-ctl [OPTIONS] quit [--force]

Commands:
  status    Print the running editor status
  open      Open a workspace file at a 1-based line and column
  save      Save the current buffer, optionally as FILE
  edit      Replace (`set`) or insert (`insert`); --stdin reads the payload
  query     Dump bounded buffer state; default is the whole buffer
  command   Execute an Ex command through the editor state machine (alias: exec)
  quit      Quit; --force discards unsaved changes

Options:
  --json              Print one JSON object with ok/data or error
  --timeout-ms <MS>   Socket timeout (default: 5000)
  --workspace <DIR>   Select the Anvil instance for DIR (default: cwd)
  --spawn             Start Anvil in the current terminal if none is running
  -h, --help          Show this help

Exit codes:
  0 success, 1 internal/local error, 2 usage, 3 no instance,
  4 timeout, 5 editor RPC error

The control socket is selected by the canonical workspace path under
$XDG_RUNTIME_DIR/anvil/.
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
                return Err(CliError::usage("edit requires set or insert"));
            };
            let mode = match mode.as_str() {
                "set" => EditMode::Set,
                "insert" => EditMode::Insert,
                _ => return Err(CliError::usage("edit mode must be set or insert")),
            };
            let Some(text) = arguments.get(1) else {
                return Err(CliError::usage("edit requires text or --stdin"));
            };
            if arguments.len() > 2 {
                return Err(CliError::usage("edit takes mode and text"));
            }
            let text = if text == "--stdin" {
                EditText::Stdin
            } else {
                EditText::Inline(text.clone())
            };
            Ok(CliCommand::Edit { mode, text })
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
        spawn_anvil(&options.workspace).map_err(|error| (clone_options(&options), error))?;
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
        CliCommand::Open { file, line, column } => client
            .open(&file, line, column)
            .map_err(|error| (clone_options(&options), map_error(error)))?,
        CliCommand::Save { path } => client
            .save(path.as_deref())
            .map_err(|error| (clone_options(&options), map_error(error)))?,
        CliCommand::Edit { mode, text } => {
            let text = match text {
                EditText::Inline(text) => text,
                EditText::Stdin => {
                    read_stdin_text().map_err(|error| (clone_options(&options), error))?
                }
            };
            json!(client
                .edit(mode, &text)
                .map_err(|error| (clone_options(&options), map_error(error)))?)
        }
        CliCommand::Query(query) => json!(client
            .query_with(query)
            .map_err(|error| (clone_options(&options), map_error(error)))?),
        CliCommand::Command(command) => json!(client
            .exec(&command)
            .map_err(|error| (clone_options(&options), map_error(error)))?),
        CliCommand::Quit { force } => client
            .quit(force)
            .map_err(|error| (clone_options(&options), map_error(error)))?,
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

fn read_stdin_text() -> Result<String, CliError> {
    use std::io::Read;

    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|error| CliError::local(format!("stdin: {error}")))?;
    if text.len() > 4 * 1024 * 1024 {
        return Err(CliError::usage("stdin edit exceeds 4 MiB"));
    }
    Ok(text)
}

fn spawn_anvil(workspace: &Path) -> Result<(), CliError> {
    use std::io::IsTerminal;

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(CliError::usage("--spawn requires a terminal"));
    }
    let binary = std::env::var_os("ANVIL_BIN").unwrap_or_else(|| "anvil".into());
    std::process::Command::new(binary)
        .arg("--workspace")
        .arg(workspace)
        .spawn()
        .map_err(|error| CliError::local(format!("cannot spawn Anvil: {error}")))?;
    let mut client =
        Client::connect_for_with_timeout(workspace, Duration::from_secs(5)).map_err(map_error)?;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if client.ping().is_ok() {
            return Ok(());
        }
    }
    Err(CliError::no_instance(
        "spawned Anvil did not start its control socket",
    ))
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
        assert!(matches!(
            parse_command(&options(&["edit", "set", "--stdin"])).unwrap(),
            CliCommand::Edit {
                mode: EditMode::Set,
                text: EditText::Stdin,
            }
        ));
        assert!(matches!(
            parse_command(&options(&["quit", "--force"])).unwrap(),
            CliCommand::Quit { force: true }
        ));
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

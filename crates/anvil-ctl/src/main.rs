use anvil_control::{Client, Error as ControlError};
use serde_json::{Value, json};
use std::{path::PathBuf, process, time::Duration};

const USAGE: &str = "\
anvil-ctl - control a running Anvil instance

Usage:
  anvil-ctl [OPTIONS] status
  anvil-ctl [OPTIONS] open <FILE> [+LINE[:COLUMN]]
  anvil-ctl [OPTIONS] save [FILE]
  anvil-ctl [OPTIONS] edit set <TEXT>
  anvil-ctl [OPTIONS] edit insert <TEXT>
  anvil-ctl [OPTIONS] query
  anvil-ctl [OPTIONS] command <COMMAND>
  anvil-ctl [OPTIONS] quit [--force]

Commands:
  status    Print the running editor status
  open      Open a workspace file at a 1-based line and column
  save      Save the current buffer, optionally as FILE
  edit      Replace the buffer (`set`) or insert at the cursor (`insert`)
  query     Dump buffer, cursor, undo, and tab state
  command   Execute an Ex command through the editor state machine
  quit      Quit; --force discards unsaved changes

Options:
  --json              Print one JSON object with ok/data or error
  --timeout-ms <MS>   Socket timeout (default: 5000)
  --spawn             Start Anvil in the current terminal if none is running
  -h, --help          Show this help

Exit codes:
  0 success, 1 internal/local error, 2 usage, 3 no instance,
  4 timeout, 5 editor RPC error

The control socket is $XDG_RUNTIME_DIR/anvil/control.sock.
";

#[derive(Debug)]
struct Options {
    json: bool,
    spawn: bool,
    timeout: Duration,
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
        mode: String,
        text: String,
    },
    Query,
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
            Self::Query => "query",
            Self::Command(_) => "command",
            Self::Quit { .. } => "quit",
        }
    }
}

fn run() -> Result<(), (Options, CliError)> {
    let options = parse_options().map_err(|error| {
        (
            Options {
                json: std::env::args().any(|argument| argument == "--json"),
                spawn: false,
                timeout: Duration::from_secs(5),
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
            if !matches!(mode.as_str(), "set" | "insert") {
                return Err(CliError::usage("edit mode must be set or insert"));
            }
            let Some(text) = arguments.get(1) else {
                return Err(CliError::usage("edit requires text"));
            };
            if arguments.len() > 2 {
                return Err(CliError::usage("edit takes mode and text"));
            }
            Ok(CliCommand::Edit {
                mode: mode.clone(),
                text: text.clone(),
            })
        }
        "query" => {
            require_no_args(arguments)?;
            Ok(CliCommand::Query)
        }
        "command" => {
            if arguments.is_empty() {
                return Err(CliError::usage("command requires a command"));
            }
            Ok(CliCommand::Command(arguments.join(" ")))
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

fn execute(options: Options, command: CliCommand) -> Result<(), (Options, CliError)> {
    let mut client = Client::connect_with_timeout(options.timeout)
        .map_err(|error| (clone_options(&options), map_error(error)))?;
    if let Err(error) = client.ping() {
        if !options.spawn {
            return Err((
                clone_options(&options),
                CliError::no_instance(format!("no running Anvil instance: {error}")),
            ));
        }
        spawn_anvil().map_err(|error| (clone_options(&options), error))?;
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
        CliCommand::Edit { mode, text } => client
            .edit(&mode, &text)
            .map_err(|error| (clone_options(&options), map_error(error)))?,
        CliCommand::Query => client
            .query()
            .map_err(|error| (clone_options(&options), map_error(error)))?,
        CliCommand::Command(command) => client
            .exec(&command)
            .map_err(|error| (clone_options(&options), map_error(error)))?,
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
            value if value.starts_with("--timeout-ms=") => {
                timeout = parse_timeout(&value["--timeout-ms=".len()..])?;
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

fn spawn_anvil() -> Result<(), CliError> {
    use std::io::IsTerminal;

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(CliError::usage("--spawn requires a terminal"));
    }
    let workspace = std::env::current_dir().map_err(|error| CliError::local(error.to_string()))?;
    let binary = std::env::var_os("ANVIL_BIN").unwrap_or_else(|| "anvil".into());
    std::process::Command::new(binary)
        .arg("--workspace")
        .arg(&workspace)
        .spawn()
        .map_err(|error| CliError::local(format!("cannot spawn Anvil: {error}")))?;
    let mut client = Client::connect_with_timeout(Duration::from_secs(5)).map_err(map_error)?;
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
            positional: positional.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn validates_commands_before_connecting() {
        assert!(parse_command(&options(&["bogus"])).is_err());
        assert!(parse_command(&options(&["open"])).is_err());
        assert!(parse_command(&options(&["edit", "replace", "x"])).is_err());
        assert!(matches!(
            parse_command(&options(&["quit", "--force"])).unwrap(),
            CliCommand::Quit { force: true }
        ));
    }
}

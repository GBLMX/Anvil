use anvil_control::Client;
use std::{path::PathBuf, process};

const USAGE: &str = "\
anvil-ctl — control a running Anvil instance

Usage:
  anvil-ctl [--spawn] status
  anvil-ctl [--spawn] open <FILE> [+LINE[:COLUMN]]
  anvil-ctl [--spawn] exec <COMMAND>

Commands:
  status   Print the running editor status
  open     Open a workspace-relative file at a 1-based line and column
  exec     Run an allowlisted editor command: format, w, wq, q

Options:
  --spawn  Start Anvil in the current workspace when no instance is running

The control socket is $XDG_RUNTIME_DIR/anvil/control.sock.
";

fn main() {
    match run() {
        Ok(()) => {}
        Err((code, message)) => {
            eprintln!("anvil-ctl: {message}");
            process::exit(code);
        }
    }
}

fn run() -> Result<(), (i32, String)> {
    let mut args = std::env::args().skip(1);
    let Some(first) = args.next() else {
        print!("{USAGE}");
        return Err((2, "a command is required".to_owned()));
    };
    if first == "--help" || first == "-h" {
        print!("{USAGE}");
        return Ok(());
    }
    let spawn = first == "--spawn";
    let command = if spawn {
        args.next()
            .ok_or_else(|| (2, "--spawn requires a command".to_owned()))?
    } else {
        first
    };
    let arguments = args.collect::<Vec<_>>();
    let mut client =
        Client::connect().map_err(|error| (1, format!("cannot connect to Anvil: {error}")))?;
    if client.ping().is_err() {
        if !spawn {
            return Err((1, "no running Anvil instance".to_owned()));
        }
        spawn_anvil()?;
    }
    match command.as_str() {
        "status" => {
            if !arguments.is_empty() {
                return Err((2, "status takes no arguments".to_owned()));
            }
            let status = client
                .status()
                .map_err(|error| (1, format!("status failed: {error}")))?;
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
        "open" => {
            let Some(file) = arguments.first() else {
                return Err((2, "open requires a file".to_owned()));
            };
            let (line, column) = match arguments.get(1) {
                Some(location) => parse_location(location)?,
                None => (1, 1),
            };
            client
                .open(&PathBuf::from(file), line, column)
                .map_err(|error| (1, format!("open failed: {error}")))?;
        }
        "exec" => {
            if arguments.is_empty() {
                return Err((2, "exec requires a command".to_owned()));
            }
            let command = arguments.join(" ");
            client
                .exec(&command)
                .map_err(|error| (1, format!("exec failed: {error}")))?;
        }
        _ => return Err((2, format!("unknown command: {command}"))),
    }
    Ok(())
}

fn spawn_anvil() -> Result<(), (i32, String)> {
    use std::io::IsTerminal;

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err((1, "--spawn requires a terminal".to_owned()));
    }
    let workspace = std::env::current_dir()
        .map_err(|error| (1, format!("cannot resolve workspace: {error}")))?;
    let binary = std::env::var_os("ANVIL_BIN").unwrap_or_else(|| "anvil".into());
    std::process::Command::new(binary)
        .arg("--workspace")
        .arg(&workspace)
        .spawn()
        .map_err(|error| (1, format!("cannot spawn Anvil: {error}")))?;
    let mut client =
        Client::connect().map_err(|error| (1, format!("cannot connect to Anvil: {error}")))?;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if client.ping().is_ok() {
            return Ok(());
        }
    }
    Err((
        1,
        "spawned Anvil did not start its control socket".to_owned(),
    ))
}

fn parse_location(value: &str) -> Result<(usize, usize), (i32, String)> {
    let value = value
        .strip_prefix('+')
        .ok_or_else(|| (2, format!("location must start with '+': {value}")))?;
    let (line, column) = value.split_once(':').unwrap_or((value, "1"));
    let line = line
        .parse::<usize>()
        .map_err(|_| (2, format!("invalid line: {line}")))?;
    let column = column
        .parse::<usize>()
        .map_err(|_| (2, format!("invalid column: {column}")))?;
    if line == 0 || column == 0 {
        return Err((2, "line and column must be 1-based".to_owned()));
    }
    Ok((line, column))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_location_forms() {
        assert_eq!(parse_location("+12").unwrap(), (12, 1));
        assert_eq!(parse_location("+12:3").unwrap(), (12, 3));
        assert!(parse_location("12").is_err());
        assert!(parse_location("+0:1").is_err());
        assert!(parse_location("+1:0").is_err());
    }
}

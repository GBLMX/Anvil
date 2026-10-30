use std::{env, path::PathBuf, process};

fn main() -> std::io::Result<()> {
    let (workspace, path) = match parse_args() {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("anvil: {error}");
            process::exit(2);
        }
    };
    anvil_tui::run_with_workspace(path, workspace)
}

fn parse_args() -> Result<(PathBuf, Option<PathBuf>), String> {
    let mut workspace = env::current_dir().map_err(|error| error.to_string())?;
    let mut path = None;
    let mut arguments = env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--workspace" || argument == "-w" {
            let Some(value) = arguments.next() else {
                return Err("--workspace requires a directory".to_owned());
            };
            workspace = PathBuf::from(value);
        } else if argument == "--" {
            path = arguments.next().map(PathBuf::from);
            if arguments.next().is_some() {
                return Err("only one file may be opened".to_owned());
            }
        } else if argument.to_string_lossy().starts_with('-') {
            return Err(format!("unknown option: {}", argument.to_string_lossy()));
        } else if path.is_some() {
            return Err("only one file may be opened".to_owned());
        } else {
            path = Some(PathBuf::from(argument));
        }
    }
    Ok((workspace, path))
}

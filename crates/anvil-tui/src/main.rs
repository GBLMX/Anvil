use std::{env, path::PathBuf};

fn main() -> std::io::Result<()> {
    let path = env::args_os().nth(1).map(PathBuf::from);
    anvil_tui::run(path)
}

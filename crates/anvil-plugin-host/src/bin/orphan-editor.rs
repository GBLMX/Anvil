#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    use std::io::Write;
    let host = std::env::var_os("ANVIL_HOST_BIN")
        .ok_or_else(|| std::io::Error::other("ANVIL_HOST_BIN is required"))?;
    let token = anvil_plugin_host::Token([7; 32]);
    let process =
        anvil_plugin_host::spawn(std::path::Path::new(&host), &["--hold".to_owned()], token)?;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{}", process.id())?;
    stdout.flush()?;
    std::thread::sleep(std::time::Duration::from_secs(60));
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("orphan-editor is Linux-only");
}

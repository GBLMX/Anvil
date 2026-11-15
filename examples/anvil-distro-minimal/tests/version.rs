#[test]
fn minimal_binary_reports_its_own_distribution_name() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_anvil-minimal"))
        .arg("--version")
        .output()
        .expect("run anvil-minimal");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("anvil-minimal {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn minimal_distribution_consumes_the_shared_tui_config() {
    assert_eq!(
        anvil_distro_minimal::DISTRIBUTION.config,
        anvil_distro_standard::ConfigContract::SharedTui
    );
}

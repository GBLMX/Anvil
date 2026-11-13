const USAGE: &str = "\
anvil-minimal — a second consumer of the Anvil distribution contract

Usage:
  anvil-minimal [OPTIONS] [FILE|-]

This fixture intentionally reuses the standard composition entrypoint. It exists
to enforce the two-consumer boundary before optional modules are added.
";

fn main() {
    anvil_distro_standard::main(&anvil_distro_standard::Distribution {
        name: "anvil-minimal",
        usage: USAGE,
        git: false,
        workspace_index: false,
        desktop: false,
    });
}

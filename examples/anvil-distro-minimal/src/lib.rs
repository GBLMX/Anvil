//! Minimal second consumer of the Anvil distribution composition contract.

use anvil_distro_standard::{ConfigContract, Distribution};

const USAGE: &str = "\
anvil-minimal — a second consumer of the Anvil distribution contract

Usage:
  anvil-minimal [OPTIONS] [FILE|-]

This fixture intentionally reuses the standard composition entrypoint. It exists
to enforce the two-consumer boundary before optional modules are added.
";

/// The minimal distribution composition.
pub const DISTRIBUTION: Distribution = Distribution {
    name: "anvil-minimal",
    usage: USAGE,
    git: false,
    workspace_index: false,
    desktop: false,
    language_latex: false,
    language_typst: false,
    config: ConfigContract::SharedTui,
};

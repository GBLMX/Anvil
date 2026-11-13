#[cfg(feature = "full")]
const ACTIVE: anvil_distro_standard::Distribution = anvil_distro_standard::FULL;
#[cfg(not(feature = "full"))]
const ACTIVE: anvil_distro_standard::Distribution = anvil_distro_standard::STANDARD;

fn main() {
    anvil_distro_standard::main(&ACTIVE);
}

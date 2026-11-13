//! Native services used by Anvil frontends.
//!
//! Services expose narrow traits and deterministic in-memory implementations.
//! The default build is std-only; native backends stay optional so headless
//! tests and feature-free builds do not link desktop libraries.

pub mod clipboard;

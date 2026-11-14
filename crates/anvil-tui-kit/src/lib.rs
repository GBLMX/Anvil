//! Reusable TUI primitives shared by Anvil frontends.
//!
//! This crate owns interaction-state pieces that do not depend on Anvil's
//! editor model: the terminal lifecycle, a filtered selection table, and
//! terminal key normalization. Application-specific overlays and mouse routing
//! stay in the frontend.

pub mod input;
pub mod selection;
pub mod terminal;

pub use selection::{SelectionItem, SelectionTable};
pub use terminal::{TerminalOptions, TerminalSession};

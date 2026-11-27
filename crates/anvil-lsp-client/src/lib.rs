//! LSP discovery, transport, diagnostics, and supervised worker.

mod lsp;
mod settings;
mod worker;

pub use lsp::{
    Diagnostic, LspConnection, LspError, Position, Range, publish_diagnostics, uri_to_path,
};
pub use settings::{
    LspSettings, ToolSource, find_tool_path, lsp_settings, lsp_settings_with_service,
};
pub use worker::{LspCapabilities, LspCommand, LspConfig, LspEvent, LspWorker, file_uri};

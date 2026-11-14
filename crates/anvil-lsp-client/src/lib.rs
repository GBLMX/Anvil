//! LSP transport and supervised worker.

mod lsp;
mod worker;

pub use lsp::{
    Diagnostic, LspConnection, LspError, Position, Range, publish_diagnostics, uri_to_path,
};
pub use worker::{LspCommand, LspConfig, LspEvent, LspWorker, file_uri};

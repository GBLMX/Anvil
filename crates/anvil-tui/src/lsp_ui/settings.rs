//! Frontend binding for the shared language-server catalog.

use std::path::Path;

use anvil_lsp_client::LspSettings;
use anvil_module::language::LanguageToolingService;

pub(crate) fn lsp_settings_with_service(
    path: &Path,
    service: Option<&dyn LanguageToolingService>,
) -> Option<LspSettings> {
    anvil_lsp_client::lsp_settings_with_service(path, service, |language_id, default_args| {
        crate::settings::lsp_command_override(language_id, default_args)
    })
}

pub(crate) fn lsp_settings_with_root(
    path: &Path,
    root: &Path,
    service: Option<&dyn LanguageToolingService>,
) -> Option<LspSettings> {
    let mut settings = lsp_settings_with_service(path, service)?;
    crate::settings::apply_lsp_root_profile(root, &mut settings);
    Some(settings)
}

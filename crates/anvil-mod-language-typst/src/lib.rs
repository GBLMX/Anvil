//! Typst language-tooling provider.

use std::env;
use std::path::Path;

use anvil_module::language::{LanguageServerSpec, LanguageToolingService};

pub const MODULE_ID: anvil_module::ModuleId =
    anvil_module::ModuleId::new("official.language-typst");

const ROOT_MARKERS: &[&str] = &["typst.toml", ".git"];

#[derive(Debug, Default, Clone, Copy)]
pub struct TypstToolingService;

impl TypstToolingService {
    fn server_for_extension(extension: &str) -> Option<(&'static str, Vec<String>)> {
        match extension {
            "typ" => Some((
                "typst",
                ROOT_MARKERS.iter().map(|m| (*m).to_owned()).collect(),
            )),
            _ => None,
        }
    }
}

impl LanguageToolingService for TypstToolingService {
    fn server_for(&self, path: &Path) -> Option<LanguageServerSpec> {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let (language_id, root_markers) = Self::server_for_extension(&extension)?;
        let tool = env::var_os("ANVIL_LSP_TYPST")
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "tinymist".to_owned());
        Some(LanguageServerSpec {
            tool,
            args: vec!["lsp".to_owned()],
            language_id: language_id.to_owned(),
            root_markers,
            label: "tinymist".to_owned(),
            initialization_options: None,
        })
    }
}

pub struct TypstModule;

impl anvil_module::Module for TypstModule {
    fn metadata(&self) -> anvil_module::ModuleMetadata {
        anvil_module::ModuleMetadata {
            id: MODULE_ID,
            name: "Official Typst Language Tools",
            contract: anvil_module::ContractRange::current(),
            commands: &[],
        }
    }

    fn register(
        &self,
        registry: &mut anvil_module::ModuleRegistry,
    ) -> Result<(), anvil_module::ModuleError> {
        registry.set_language_tooling_service(std::sync::Arc::new(TypstToolingService))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_module::language::LanguageToolingService;

    #[test]
    fn maps_typst_extension() {
        let service = TypstToolingService;
        let typst = service.server_for(Path::new("paper.typ")).expect("typst");
        assert_eq!(typst.language_id, "typst");
        assert_eq!(typst.tool, "tinymist");
        assert_eq!(typst.args, ["lsp"]);
        assert!(typst.root_markers.contains(&"typst.toml".to_owned()));
    }

    #[test]
    fn ignores_other_extensions() {
        assert!(
            TypstToolingService
                .server_for(Path::new("main.rs"))
                .is_none()
        );
        assert!(
            TypstToolingService
                .server_for(Path::new("README"))
                .is_none()
        );
    }

    #[test]
    fn registers_the_language_tooling_service() {
        let mut registry = anvil_module::ModuleRegistry::new();
        registry.register(&TypstModule).expect("register");
        assert!(registry.contains(MODULE_ID));
        assert!(registry.language_tooling_service().is_some());
    }
}

//! LaTeX/BibTeX language-tooling provider.

use std::env;
use std::path::Path;

use anvil_module::language::{LanguageServerSpec, LanguageToolingService};

pub const MODULE_ID: anvil_module::ModuleId =
    anvil_module::ModuleId::new("official.language-latex");

const ROOT_MARKERS: &[&str] = &[
    "latexmkrc",
    ".latexmkrc",
    "Tectonic.toml",
    "main.tex",
    ".git",
];

#[derive(Debug, Default, Clone, Copy)]
pub struct LatexToolingService;

impl LatexToolingService {
    fn server_for_extension(extension: &str) -> Option<(&'static str, Vec<String>)> {
        match extension {
            "tex" | "ltx" | "sty" | "cls" => Some((
                "latex",
                ROOT_MARKERS.iter().map(|m| (*m).to_owned()).collect(),
            )),
            "bib" => Some((
                "bibtex",
                ROOT_MARKERS.iter().map(|m| (*m).to_owned()).collect(),
            )),
            _ => None,
        }
    }
}

impl LanguageToolingService for LatexToolingService {
    fn server_for(&self, path: &Path) -> Option<LanguageServerSpec> {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let (language_id, root_markers) = Self::server_for_extension(&extension)?;
        let tool = env::var_os("ANVIL_LSP_LATEX")
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "texlab".to_owned());
        Some(LanguageServerSpec {
            tool,
            args: Vec::new(),
            language_id: language_id.to_owned(),
            root_markers,
            label: "texlab".to_owned(),
            initialization_options: None,
        })
    }
}

pub struct LatexModule;

impl anvil_module::Module for LatexModule {
    fn metadata(&self) -> anvil_module::ModuleMetadata {
        anvil_module::ModuleMetadata {
            id: MODULE_ID,
            name: "Official LaTeX Language Tools",
            contract: anvil_module::ContractRange::current(),
            commands: &[],
        }
    }

    fn register(
        &self,
        registry: &mut anvil_module::ModuleRegistry,
    ) -> Result<(), anvil_module::ModuleError> {
        registry.set_language_tooling_service(std::sync::Arc::new(LatexToolingService))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_module::language::LanguageToolingService;

    #[test]
    fn maps_latex_and_bibtex_extensions() {
        let service = LatexToolingService;
        let tex = service.server_for(Path::new("paper.tex")).expect("tex");
        assert_eq!(tex.language_id, "latex");
        assert_eq!(tex.tool, "texlab");
        assert!(tex.root_markers.contains(&"main.tex".to_owned()));

        let bib = service
            .server_for(Path::new("references.BIB"))
            .expect("bib");
        assert_eq!(bib.language_id, "bibtex");
        assert_eq!(bib.tool, "texlab");
    }

    #[test]
    fn ignores_other_extensions() {
        assert!(
            LatexToolingService
                .server_for(Path::new("main.rs"))
                .is_none()
        );
        assert!(
            LatexToolingService
                .server_for(Path::new("README"))
                .is_none()
        );
    }

    #[test]
    fn registers_the_language_tooling_service() {
        let mut registry = anvil_module::ModuleRegistry::new();
        registry.register(&LatexModule).expect("register");
        assert!(registry.contains(MODULE_ID));
        assert!(registry.language_tooling_service().is_some());
    }
}

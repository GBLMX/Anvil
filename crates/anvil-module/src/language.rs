//! Language-tooling extension-point types.

use std::path::Path;
use std::sync::Arc;

/// UI-neutral description of one language-server launch profile.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageServerSpec {
    pub tool: String,
    pub args: Vec<String>,
    pub language_id: String,
    pub root_markers: Vec<String>,
    pub label: String,
    pub initialization_options: Option<serde_json::Value>,
}

/// UI-neutral language-tooling provider.
pub trait LanguageToolingService: Send + Sync {
    fn server_for(&self, path: &Path) -> Option<LanguageServerSpec>;
}

/// Resolves providers in registration order and returns the first match.
#[derive(Clone, Default)]
pub struct LanguageToolingProviders {
    providers: Vec<Arc<dyn LanguageToolingService>>,
}

impl LanguageToolingProviders {
    pub fn new(providers: Vec<Arc<dyn LanguageToolingService>>) -> Self {
        Self { providers }
    }
}

impl LanguageToolingService for LanguageToolingProviders {
    fn server_for(&self, path: &Path) -> Option<LanguageServerSpec> {
        self.providers
            .iter()
            .find_map(|provider| provider.server_for(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Provider(&'static str);

    impl LanguageToolingService for Provider {
        fn server_for(&self, path: &Path) -> Option<LanguageServerSpec> {
            path.extension()
                .and_then(|extension| extension.to_str())
                .filter(|extension| *extension == self.0)
                .map(|_| LanguageServerSpec {
                    tool: self.0.to_owned(),
                    args: Vec::new(),
                    language_id: self.0.to_owned(),
                    root_markers: Vec::new(),
                    label: self.0.to_owned(),
                    initialization_options: None,
                })
        }
    }

    #[test]
    fn resolves_the_first_matching_provider() {
        let providers = LanguageToolingProviders::new(vec![
            Arc::new(Provider("tex")),
            Arc::new(Provider("typ")),
        ]);
        assert_eq!(
            providers
                .server_for(Path::new("paper.typ"))
                .expect("typst")
                .tool,
            "typ"
        );
        assert!(providers.server_for(Path::new("main.rs")).is_none());
    }
}

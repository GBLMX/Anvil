use super::{CancellationToken, ProviderError, ProviderId};
use std::path::Path;

/// One directory entry made available to path completion by the core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionPathEntry {
    pub name: String,
    pub is_dir: bool,
}

/// Borrowed document context for one completion request.
pub struct CompletionContext<'a> {
    pub path: Option<&'a Path>,
    pub language_id: &'a str,
    pub revision: u64,
    pub text: &'a str,
    pub cursor: usize,
    pub prefix: &'a str,
    pub path_prefix: &'a str,
    pub path_entries: &'a [CompletionPathEntry],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompletionKind {
    #[default]
    Text,
    Keyword,
    Function,
    Variable,
    Snippet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub insert_text: String,
    pub filter_text: Option<String>,
    pub sort_text: Option<String>,
    pub kind: CompletionKind,
}

/// A source of completion items. Sources return candidates only; the core
/// owns insertion, ranking, and snippet execution.
pub trait CompletionSource: Send + Sync {
    fn id(&self) -> ProviderId;

    fn trigger_characters(&self) -> Vec<char> {
        Vec::new()
    }

    fn is_available(&self, _context: &CompletionContext<'_>) -> bool {
        true
    }

    fn complete(
        &self,
        context: &CompletionContext<'_>,
        cancel: &CancellationToken,
    ) -> Result<Vec<CompletionItem>, ProviderError>;

    fn resolve(&self, item: &CompletionItem) -> Result<CompletionItem, ProviderError> {
        Ok(item.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProviderOrigin, ProviderPriority, ProviderRegistry};
    use std::path::Path;
    use std::sync::Arc;

    struct FakeSource;

    impl CompletionSource for FakeSource {
        fn id(&self) -> ProviderId {
            ProviderId::new("test.completion")
        }

        fn complete(
            &self,
            context: &CompletionContext<'_>,
            cancel: &CancellationToken,
        ) -> Result<Vec<CompletionItem>, ProviderError> {
            cancel.check()?;
            Ok(vec![CompletionItem {
                label: context.prefix.to_owned(),
                detail: None,
                insert_text: context.prefix.to_owned(),
                filter_text: None,
                sort_text: None,
                kind: CompletionKind::Text,
            }])
        }
    }

    #[test]
    fn completion_sources_are_object_safe_and_registered() {
        let mut registry: ProviderRegistry<dyn CompletionSource> = ProviderRegistry::new();
        registry
            .register(
                ProviderId::new("test.completion"),
                ProviderPriority::new(1),
                ProviderOrigin::Builtin,
                Arc::new(FakeSource),
            )
            .expect("register");
        let provider = registry.providers()[0].provider;
        let context = CompletionContext {
            path: Some(Path::new("x.rs")),
            language_id: "rust",
            revision: 1,
            text: "val",
            cursor: 3,
            prefix: "val",
            path_prefix: "",
            path_entries: &[],
        };
        let items = provider
            .complete(&context, &CancellationToken::new())
            .expect("complete");
        assert_eq!(items[0].insert_text, "val");
    }
}

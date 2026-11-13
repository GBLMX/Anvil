use super::{CancellationToken, ProviderError, ProviderId};
use std::path::Path;

/// How a formatter executes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormatterKind {
    /// Reads the buffer from stdin and writes the result to stdout.
    #[default]
    StdinStdout,
    /// Rewrites the file in place; the core must reload it.
    InPlace,
    /// Writes a temporary file and reads the formatted result.
    TempFile,
}

/// Borrowed document context for one formatter request.
pub struct FormatContext<'a> {
    pub path: Option<&'a Path>,
    pub language_id: &'a str,
    pub revision: u64,
    pub text: &'a str,
}

/// The formatter's output. The core applies it only if the revision is current.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormattedText {
    pub provider: ProviderId,
    pub text: String,
}

/// A provider of formatted document text.
///
/// Providers return candidates only; the core owns revision checks and buffer
/// replacement.
pub trait FormatterProvider: Send + Sync {
    fn id(&self) -> ProviderId;

    fn kind(&self) -> FormatterKind {
        FormatterKind::StdinStdout
    }

    fn is_available(&self, _context: &FormatContext<'_>) -> bool {
        true
    }

    fn format(
        &self,
        context: &FormatContext<'_>,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProviderOrigin, ProviderPriority, ProviderRegistry};
    use std::sync::Arc;

    struct FakeFormatter;

    impl FormatterProvider for FakeFormatter {
        fn id(&self) -> ProviderId {
            ProviderId::new("test.formatter")
        }

        fn format(
            &self,
            context: &FormatContext<'_>,
            cancel: &CancellationToken,
        ) -> Result<Option<String>, ProviderError> {
            cancel.check()?;
            Ok(Some(context.text.to_uppercase()))
        }
    }

    #[test]
    fn formatter_is_object_safe_and_registered() {
        let mut registry: ProviderRegistry<dyn FormatterProvider> = ProviderRegistry::new();
        registry
            .register(
                ProviderId::new("test.formatter"),
                ProviderPriority::new(1),
                ProviderOrigin::Builtin,
                Arc::new(FakeFormatter),
            )
            .expect("register");
        let context = FormatContext {
            path: Some(Path::new("x.rs")),
            language_id: "rust",
            revision: 1,
            text: "fn main() {}",
        };
        let output = registry.providers()[0]
            .provider
            .format(&context, &CancellationToken::new())
            .expect("format");
        assert_eq!(output.as_deref(), Some("FN MAIN() {}"));
    }
}

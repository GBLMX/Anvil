//! Core formatter selection and revision-checked buffer replacement.

use crate::SessionError;
use crate::provider::{
    CancellationToken, FormatContext, FormattedText, FormatterProvider, ProviderError, ProviderId,
    ProviderOrigin, ProviderPriority, ProviderRegistry, RegistryError,
};
use anvil_core::TextBuffer;
use anvil_editor::EditorState;
use std::sync::Arc;

/// Ordered formatter providers plus the revision-checked apply step.
pub struct FormatService {
    providers: ProviderRegistry<dyn FormatterProvider>,
}

impl Default for FormatService {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatService {
    pub fn new() -> Self {
        Self {
            providers: ProviderRegistry::new(),
        }
    }

    pub fn register(
        &mut self,
        id: ProviderId,
        priority: ProviderPriority,
        origin: ProviderOrigin,
        provider: Arc<dyn FormatterProvider>,
    ) -> Result<(), RegistryError> {
        self.providers.register(id, priority, origin, provider)
    }

    pub fn unregister(&mut self, id: &ProviderId) -> bool {
        self.providers.unregister(id)
    }

    pub fn providers(&self) -> Vec<(ProviderId, ProviderPriority, ProviderOrigin)> {
        self.providers
            .providers()
            .into_iter()
            .map(|entry| (entry.id.clone(), entry.priority, entry.origin.clone()))
            .collect()
    }

    /// Runs the highest-priority available formatter that returns output.
    ///
    /// A provider returning `Ok(None)` or `Unavailable` defers to the next
    /// provider. Any other error stops the chain.
    pub fn format(
        &self,
        context: &FormatContext<'_>,
        cancel: &CancellationToken,
    ) -> Result<Option<FormattedText>, ProviderError> {
        cancel.check()?;
        let mut saw_available = false;
        for entry in self.providers.providers() {
            if entry.token.is_cancelled() {
                continue;
            }
            if !entry.provider.is_available(context) {
                continue;
            }
            saw_available = true;
            match entry.provider.format(context, cancel) {
                Ok(Some(text)) => {
                    return Ok(Some(FormattedText {
                        provider: entry.id.clone(),
                        text,
                    }));
                }
                Ok(None) => continue,
                Err(ProviderError::Unavailable) => continue,
                Err(error) => return Err(error),
            }
        }
        if saw_available {
            Ok(None)
        } else {
            Err(ProviderError::Unavailable)
        }
    }

    /// Replaces the buffer text only when `base_revision` is still current.
    ///
    /// Returns `false` when the formatter produced no change.
    pub fn apply(
        editor: &mut EditorState,
        base_revision: u64,
        formatted: &FormattedText,
    ) -> Result<bool, SessionError> {
        let actual = editor.revision();
        if actual != base_revision {
            return Err(SessionError::StaleRevision {
                expected: base_revision,
                actual,
            });
        }
        let len = editor.text().len_bytes();
        if editor.text().slice(0..len) == formatted.text {
            return Ok(false);
        }
        editor.apply_transaction_at(base_revision, vec![(0..len, formatted.text.clone())])?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{FormatterKind, ProviderOrigin, ProviderPriority};
    use std::path::Path;
    use std::sync::Arc;

    struct FakeFormatter {
        id: &'static str,
        output: Option<&'static str>,
        fail: bool,
    }

    impl FormatterProvider for FakeFormatter {
        fn id(&self) -> ProviderId {
            ProviderId::new(self.id)
        }

        fn kind(&self) -> FormatterKind {
            FormatterKind::StdinStdout
        }

        fn format(
            &self,
            _context: &FormatContext<'_>,
            _cancel: &CancellationToken,
        ) -> Result<Option<String>, ProviderError> {
            if self.fail {
                return Err(ProviderError::Internal("boom".to_owned()));
            }
            Ok(self.output.map(str::to_owned))
        }
    }

    fn service(providers: &[(&'static str, i32, Option<&'static str>, bool)]) -> FormatService {
        let mut service = FormatService::new();
        for (id, priority, output, fail) in providers {
            service
                .register(
                    ProviderId::new(*id),
                    ProviderPriority::new(*priority),
                    ProviderOrigin::Builtin,
                    Arc::new(FakeFormatter {
                        id,
                        output: *output,
                        fail: *fail,
                    }),
                )
                .expect("register");
        }
        service
    }

    fn context() -> FormatContext<'static> {
        FormatContext {
            path: Some(Path::new("src/main.rs")),
            language_id: "rust",
            revision: 1,
            text: "fn main(){}",
        }
    }

    #[test]
    fn highest_priority_output_wins() {
        let service = service(&[
            ("test.low", 0, Some("low"), false),
            ("test.high", 10, Some("high"), false),
        ]);
        let formatted = service
            .format(&context(), &CancellationToken::new())
            .expect("format")
            .expect("output");
        assert_eq!(formatted.provider, ProviderId::new("test.high"));
        assert_eq!(formatted.text, "high");
    }

    #[test]
    fn unavailable_output_falls_through() {
        let service = service(&[
            ("test.none", 10, None, false),
            ("test.output", 0, Some("formatted"), false),
        ]);
        let formatted = service
            .format(&context(), &CancellationToken::new())
            .expect("format")
            .expect("output");
        assert_eq!(formatted.text, "formatted");
    }

    #[test]
    fn provider_error_stops_the_chain() {
        let service = service(&[
            ("test.fail", 10, None, true),
            ("test.output", 0, Some("formatted"), false),
        ]);
        assert_eq!(
            service.format(&context(), &CancellationToken::new()),
            Err(ProviderError::Internal("boom".to_owned()))
        );
    }

    #[test]
    fn cancelled_request_does_not_run_a_provider() {
        let service = service(&[("test.output", 0, Some("formatted"), false)]);
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert_eq!(
            service.format(&context(), &cancel),
            Err(ProviderError::Cancelled)
        );
        assert_eq!(
            service.format(&context(), &CancellationToken::new()),
            Ok(Some(FormattedText {
                provider: ProviderId::new("test.output"),
                text: "formatted".to_owned(),
            }))
        );
    }

    #[test]
    fn apply_rejects_stale_revision_and_no_op_text() {
        let mut editor = EditorState::from_text("fn main(){}");
        let stale = FormattedText {
            provider: ProviderId::new("test.formatter"),
            text: "fn main() {}\n".to_owned(),
        };
        assert!(matches!(
            FormatService::apply(&mut editor, 99, &stale),
            Err(SessionError::StaleRevision { .. })
        ));
        let same = FormattedText {
            provider: ProviderId::new("test.formatter"),
            text: "fn main(){}".to_owned(),
        };
        let revision = editor.revision();
        assert!(!FormatService::apply(&mut editor, revision, &same).expect("apply"));
        let revision = editor.revision();
        assert!(FormatService::apply(&mut editor, revision, &stale).expect("apply changed"));
        assert_eq!(editor.text().slice(0..13), "fn main() {}\n");
    }
}

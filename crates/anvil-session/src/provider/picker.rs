use super::{CancellationToken, ProviderError, ProviderId};
use serde_json::Value;
use std::path::PathBuf;

/// One buffer visible to a picker source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerBuffer {
    pub index: usize,
    pub name: String,
    pub active: bool,
    pub modified: bool,
}

/// UI-neutral snapshot supplied to picker sources.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickerContext {
    pub root: PathBuf,
    pub revision: u64,
    pub buffers: Vec<PickerBuffer>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerQuery {
    pub text: String,
    pub limit: usize,
}

/// One picker result. `value` is interpreted by the source's action handler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerEntry {
    pub value: Value,
    pub ordinal: String,
    pub display: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerPreview {
    pub text: String,
}

/// A source of picker entries. Sources return data only; the core owns
/// filtering, selection, actions, and rendering.
pub trait PickerSource: Send + Sync {
    fn id(&self) -> ProviderId;

    fn is_available(&self, _context: &PickerContext) -> bool {
        true
    }

    fn query(
        &self,
        context: &PickerContext,
        query: &PickerQuery,
        cancel: &CancellationToken,
        emit: &mut dyn FnMut(Vec<PickerEntry>),
    ) -> Result<(), ProviderError>;

    fn preview(&self, _entry: &PickerEntry) -> Result<Option<PickerPreview>, ProviderError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProviderOrigin, ProviderPriority, ProviderRegistry};
    use std::sync::Arc;

    struct FakeSource;

    impl PickerSource for FakeSource {
        fn id(&self) -> ProviderId {
            ProviderId::new("test.buffers")
        }

        fn query(
            &self,
            _context: &PickerContext,
            query: &PickerQuery,
            cancel: &CancellationToken,
            emit: &mut dyn FnMut(Vec<PickerEntry>),
        ) -> Result<(), ProviderError> {
            cancel.check()?;
            emit(vec![PickerEntry {
                value: Value::from(1),
                ordinal: query.text.clone(),
                display: "one".to_owned(),
            }]);
            Ok(())
        }
    }

    #[test]
    fn picker_sources_are_object_safe_and_registered() {
        let mut registry: ProviderRegistry<dyn PickerSource> = ProviderRegistry::new();
        registry
            .register(
                ProviderId::new("test.buffers"),
                ProviderPriority::new(1),
                ProviderOrigin::Builtin,
                Arc::new(FakeSource),
            )
            .expect("register");

        let providers = registry.providers();
        let source = providers[0].provider;
        let context = PickerContext::default();
        let query = PickerQuery {
            text: "buf".to_owned(),
            limit: 10,
        };
        let cancel = CancellationToken::new();
        let mut entries = Vec::new();
        source
            .query(&context, &query, &cancel, &mut |batch| {
                entries.extend(batch)
            })
            .expect("query");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].display, "one");
    }

    #[test]
    fn cancelled_queries_stop_before_emit() {
        struct Cancelled;
        impl PickerSource for Cancelled {
            fn id(&self) -> ProviderId {
                ProviderId::new("test.cancelled")
            }

            fn query(
                &self,
                _context: &PickerContext,
                _query: &PickerQuery,
                cancel: &CancellationToken,
                emit: &mut dyn FnMut(Vec<PickerEntry>),
            ) -> Result<(), ProviderError> {
                cancel.check()?;
                emit(Vec::new());
                Ok(())
            }
        }

        let token = CancellationToken::new();
        token.cancel();
        let source = Cancelled;
        let mut emitted = false;
        let result = source.query(
            &PickerContext::default(),
            &PickerQuery {
                text: String::new(),
                limit: 1,
            },
            &token,
            &mut |_| emitted = true,
        );
        assert_eq!(result, Err(ProviderError::Cancelled));
        assert!(!emitted);
    }
}

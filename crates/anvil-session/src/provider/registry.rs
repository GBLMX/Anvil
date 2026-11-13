use crate::provider::{
    CancellationToken, ProviderId, ProviderOrigin, ProviderPriority, RegistryError,
};
use std::sync::Arc;

struct ProviderEntry<T: ?Sized> {
    id: ProviderId,
    priority: ProviderPriority,
    origin: ProviderOrigin,
    token: CancellationToken,
    provider: Arc<T>,
}

/// One registered provider and its provenance.
pub struct RegisteredProvider<'a, T: ?Sized> {
    pub id: &'a ProviderId,
    pub priority: ProviderPriority,
    pub origin: &'a ProviderOrigin,
    pub token: &'a CancellationToken,
    pub provider: &'a Arc<T>,
}

/// Ordered collection of providers for one capability.
///
/// Higher priority values run first. Equal priorities are ordered by provider
/// id so registration order never decides precedence.
pub struct ProviderRegistry<T: ?Sized> {
    entries: Vec<ProviderEntry<T>>,
}

impl<T: ?Sized> Default for ProviderRegistry<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<T: ?Sized> ProviderRegistry<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        id: ProviderId,
        priority: ProviderPriority,
        origin: ProviderOrigin,
        provider: Arc<T>,
    ) -> Result<(), RegistryError> {
        if !id.is_valid() {
            return Err(RegistryError::InvalidId(id));
        }
        if self.contains(&id) {
            return Err(RegistryError::DuplicateId(id));
        }
        self.entries.push(ProviderEntry {
            id,
            priority,
            origin,
            token: CancellationToken::new(),
            provider,
        });
        Ok(())
    }

    pub fn unregister(&mut self, id: &ProviderId) -> bool {
        let Some(index) = self.entries.iter().position(|entry| &entry.id == id) else {
            return false;
        };
        let entry = self.entries.remove(index);
        entry.token.cancel();
        true
    }

    pub fn contains(&self, id: &ProviderId) -> bool {
        self.entries.iter().any(|entry| &entry.id == id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        for entry in self.entries.drain(..) {
            entry.token.cancel();
        }
    }

    pub fn token(&self, id: &ProviderId) -> Option<CancellationToken> {
        self.entries
            .iter()
            .find(|entry| &entry.id == id)
            .map(|entry| entry.token.clone())
    }

    /// Providers in deterministic execution order.
    pub fn providers(&self) -> Vec<RegisteredProvider<'_, T>> {
        let mut entries: Vec<_> = self.entries.iter().collect();
        entries.sort_by(|left, right| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left.id.cmp(&right.id))
        });
        entries
            .into_iter()
            .map(|entry| RegisteredProvider {
                id: &entry.id,
                priority: entry.priority,
                origin: &entry.origin,
                token: &entry.token,
                provider: &entry.provider,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_api::ModuleId;

    struct FakeProvider;

    fn provider(
        id: &'static str,
        priority: i32,
        origin: ProviderOrigin,
    ) -> ProviderRegistry<FakeProvider> {
        let mut registry = ProviderRegistry::new();
        registry
            .register(
                ProviderId::new(id),
                ProviderPriority::new(priority),
                origin,
                Arc::new(FakeProvider),
            )
            .expect("register");
        registry
    }

    #[test]
    fn rejects_duplicate_ids() {
        let mut registry = provider("builtin.buffer", 0, ProviderOrigin::Builtin);
        let error = registry
            .register(
                ProviderId::new("builtin.buffer"),
                ProviderPriority::new(10),
                ProviderOrigin::Builtin,
                Arc::new(FakeProvider),
            )
            .expect_err("duplicate");
        assert_eq!(
            error,
            RegistryError::DuplicateId(ProviderId::new("builtin.buffer"))
        );
    }

    #[test]
    fn rejects_invalid_ids() {
        let mut registry = ProviderRegistry::<FakeProvider>::new();
        let error = registry
            .register(
                ProviderId::new("buffer"),
                ProviderPriority::default(),
                ProviderOrigin::Builtin,
                Arc::new(FakeProvider),
            )
            .expect_err("invalid");
        assert_eq!(error, RegistryError::InvalidId(ProviderId::new("buffer")));
    }

    #[test]
    fn orders_by_priority_then_id() {
        let origin = ProviderOrigin::Builtin;
        let mut registry = provider("builtin.low", 0, origin.clone());
        for (id, priority) in [
            ("builtin.high", 10),
            ("builtin.equal-b", 5),
            ("builtin.equal-a", 5),
        ] {
            registry
                .register(
                    ProviderId::new(id),
                    ProviderPriority::new(priority),
                    origin.clone(),
                    Arc::new(FakeProvider),
                )
                .expect("register");
        }
        let ids: Vec<_> = registry
            .providers()
            .into_iter()
            .map(|provider| provider.id.as_str().to_owned())
            .collect();
        assert_eq!(
            ids,
            [
                "builtin.high",
                "builtin.equal-a",
                "builtin.equal-b",
                "builtin.low"
            ]
        );
    }

    #[test]
    fn keeps_module_provenance() {
        let module = ModuleId::new("official.git");
        let registry = provider("official.git-status", 0, ProviderOrigin::RustModule(module));
        assert_eq!(
            registry.providers()[0].origin,
            &ProviderOrigin::RustModule(module)
        );
    }

    #[test]
    fn unregister_cancels_outstanding_token() {
        let mut registry = provider("builtin.buffer", 0, ProviderOrigin::Builtin);
        let id = ProviderId::new("builtin.buffer");
        let token = registry.token(&id).expect("token");
        assert!(!token.is_cancelled());
        assert!(registry.unregister(&id));
        assert!(token.is_cancelled());
        assert!(!registry.unregister(&id));
        assert!(registry.is_empty());
    }

    #[test]
    fn clear_cancels_every_provider() {
        let mut registry = provider("builtin.one", 0, ProviderOrigin::Builtin);
        registry
            .register(
                ProviderId::new("builtin.two"),
                ProviderPriority::default(),
                ProviderOrigin::Builtin,
                Arc::new(FakeProvider),
            )
            .expect("register");
        let one = registry
            .token(&ProviderId::new("builtin.one"))
            .expect("one");
        let two = registry
            .token(&ProviderId::new("builtin.two"))
            .expect("two");
        registry.clear();
        assert!(one.is_cancelled());
        assert!(two.is_cancelled());
        assert!(registry.is_empty());
    }
}

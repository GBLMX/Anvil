//! Data-only registry for runtime-plugin picker and completion providers.

use crate::provider::ProviderId;
use anvil_api::{ProviderKind, RegisterProviderRequest};
use std::fmt;
use std::sync::Arc;

/// Why a runtime-plugin provider registration failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginProviderError {
    InvalidId,
    InvalidTitle,
    NamespaceMismatch,
    DuplicateId(ProviderId),
}

impl fmt::Display for PluginProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId => formatter.write_str("invalid provider id"),
            Self::InvalidTitle => formatter.write_str("invalid provider title"),
            Self::NamespaceMismatch => {
                formatter.write_str("provider id is outside the plugin namespace")
            }
            Self::DuplicateId(id) => write!(formatter, "duplicate provider id: {id}"),
        }
    }
}

impl std::error::Error for PluginProviderError {}

/// One registered runtime-plugin provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginProviderRegistration {
    pub id: ProviderId,
    pub plugin_id: Arc<str>,
    pub kind: ProviderKind,
    pub title: String,
}

/// Registry of runtime-plugin providers. It stores metadata only; it never
/// grants process, filesystem, or network authority.
#[derive(Default)]
pub struct PluginProviderRegistry {
    entries: Vec<PluginProviderRegistration>,
}

impl PluginProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        plugin_id: &str,
        request: RegisterProviderRequest,
    ) -> Result<PluginProviderRegistration, PluginProviderError> {
        let id = request.id.trim();
        let title = request.title.trim();
        if id.is_empty() || id.len() > 128 {
            return Err(PluginProviderError::InvalidId);
        }
        let provider_id = ProviderId::new(id);
        if !provider_id.is_valid() {
            return Err(PluginProviderError::InvalidId);
        }
        let Some((vendor, _)) = id.split_once('.') else {
            return Err(PluginProviderError::InvalidId);
        };
        if vendor != plugin_id {
            return Err(PluginProviderError::NamespaceMismatch);
        }
        if title.is_empty() || title.len() > 256 {
            return Err(PluginProviderError::InvalidTitle);
        }
        if self.entries.iter().any(|entry| entry.id == provider_id) {
            return Err(PluginProviderError::DuplicateId(provider_id));
        }
        let registration = PluginProviderRegistration {
            id: provider_id,
            plugin_id: Arc::from(plugin_id),
            kind: request.kind,
            title: title.to_owned(),
        };
        self.entries.push(registration.clone());
        Ok(registration)
    }

    /// Removes every provider owned by a disabled or exited plugin.
    pub fn unregister_plugin(&mut self, plugin_id: &str) -> Vec<ProviderId> {
        let mut removed = Vec::new();
        self.entries.retain(|entry| {
            if entry.plugin_id.as_ref() == plugin_id {
                removed.push(entry.id.clone());
                false
            } else {
                true
            }
        });
        removed
    }

    pub fn providers(&self) -> &[PluginProviderRegistration] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str) -> RegisterProviderRequest {
        RegisterProviderRequest {
            id: id.to_owned(),
            kind: ProviderKind::Picker,
            title: "Buffers".to_owned(),
        }
    }

    #[test]
    fn registers_and_unregisters_plugin_providers() {
        let mut registry = PluginProviderRegistry::new();
        let registration = registry
            .register("debug", request("debug.buffers"))
            .unwrap();
        assert_eq!(registration.kind, ProviderKind::Picker);
        assert_eq!(registry.len(), 1);
        assert_eq!(
            registry.unregister_plugin("debug"),
            vec![ProviderId::new("debug.buffers")]
        );
        assert!(registry.is_empty());
    }

    #[test]
    fn rejects_duplicate_and_foreign_namespaces() {
        let mut registry = PluginProviderRegistry::new();
        registry
            .register("debug", request("debug.buffers"))
            .unwrap();
        assert_eq!(
            registry.register("debug", request("debug.buffers")),
            Err(PluginProviderError::DuplicateId(ProviderId::new(
                "debug.buffers"
            )))
        );
        assert_eq!(
            registry.register("debug", request("other.buffers")),
            Err(PluginProviderError::NamespaceMismatch)
        );
    }

    #[test]
    fn rejects_invalid_ids_and_titles() {
        let mut registry = PluginProviderRegistry::new();
        assert_eq!(
            registry.register("debug", request("buffers")),
            Err(PluginProviderError::InvalidId)
        );
        let mut empty_title = request("debug.buffers");
        empty_title.title.clear();
        assert_eq!(
            registry.register("debug", empty_title),
            Err(PluginProviderError::InvalidTitle)
        );
    }
}

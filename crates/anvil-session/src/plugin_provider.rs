//! Data-only registry for runtime-plugin picker and completion providers.

use crate::provider::ProviderId;
use anvil_api::{ProviderItem, ProviderKind, ProviderResponse, RegisterProviderRequest};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// Why a runtime-plugin provider registration failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginProviderError {
    InvalidId,
    InvalidTitle,
    InvalidHandler,
    NamespaceMismatch,
    DuplicateId(ProviderId),
    InvalidQuery,
    InvalidLimit,
    UnknownRequest(u64),
    MismatchedRequest { expected: u64, actual: u64 },
    TooManyItems,
    InvalidItem,
}

impl fmt::Display for PluginProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId => formatter.write_str("invalid provider id"),
            Self::InvalidTitle => formatter.write_str("invalid provider title"),
            Self::InvalidHandler => formatter.write_str("invalid provider handler"),
            Self::NamespaceMismatch => {
                formatter.write_str("provider id is outside the plugin namespace")
            }
            Self::DuplicateId(id) => write!(formatter, "duplicate provider id: {id}"),
            Self::InvalidQuery => formatter.write_str("invalid provider query"),
            Self::InvalidLimit => formatter.write_str("invalid provider limit"),
            Self::UnknownRequest(id) => write!(formatter, "unknown provider request: {id}"),
            Self::MismatchedRequest { expected, actual } => {
                write!(
                    formatter,
                    "provider response id {actual} does not match request {expected}"
                )
            }
            Self::TooManyItems => formatter.write_str("too many provider items"),
            Self::InvalidItem => formatter.write_str("invalid provider item"),
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
    pub handler: String,
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
        let handler = request.handler.trim();
        if handler.is_empty()
            || handler.len() > 128
            || !handler.bytes().enumerate().all(|(index, byte)| {
                byte == b'_'
                    || byte.is_ascii_alphanumeric() && (index > 0 || !byte.is_ascii_digit())
            })
        {
            return Err(PluginProviderError::InvalidHandler);
        }
        if self.entries.iter().any(|entry| entry.id == provider_id) {
            return Err(PluginProviderError::DuplicateId(provider_id));
        }
        let registration = PluginProviderRegistration {
            id: provider_id,
            plugin_id: Arc::from(plugin_id),
            kind: request.kind,
            title: title.to_owned(),
            handler: handler.to_owned(),
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

/// Tracks in-flight provider requests; responses are accepted once.
#[derive(Default)]
pub struct PluginProviderRequests {
    next_id: u64,
    pending: BTreeMap<u64, ProviderId>,
}

impl PluginProviderRequests {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(
        &mut self,
        provider: ProviderId,
        query: &str,
        limit: u32,
    ) -> Result<u64, PluginProviderError> {
        if query.len() > 256 {
            return Err(PluginProviderError::InvalidQuery);
        }
        if limit == 0 || limit > 200 {
            return Err(PluginProviderError::InvalidLimit);
        }
        let id = self.next_id.saturating_add(1).max(1);
        self.next_id = id;
        self.pending.insert(id, provider);
        Ok(id)
    }

    pub fn provider_for(&self, request_id: u64) -> Option<&ProviderId> {
        self.pending.get(&request_id)
    }

    pub fn cancel(&mut self, request_id: u64) -> Option<ProviderId> {
        self.pending.remove(&request_id)
    }

    pub fn cancel_provider(&mut self, provider: &ProviderId) {
        self.pending.retain(|_, pending| pending != provider);
    }

    pub fn accept(
        &mut self,
        response: ProviderResponse,
    ) -> Result<Vec<ProviderItem>, PluginProviderError> {
        self.accept_for(response.request_id, response)
    }

    /// Accepts a response only when its payload id matches the transport id.
    pub fn accept_for(
        &mut self,
        request_id: u64,
        response: ProviderResponse,
    ) -> Result<Vec<ProviderItem>, PluginProviderError> {
        if response.request_id != request_id {
            self.cancel(request_id);
            return Err(PluginProviderError::MismatchedRequest {
                expected: request_id,
                actual: response.request_id,
            });
        }
        if self.pending.remove(&request_id).is_none() {
            return Err(PluginProviderError::UnknownRequest(request_id));
        }
        if response.items.len() > 200 {
            return Err(PluginProviderError::TooManyItems);
        }
        for item in &response.items {
            if item.label.is_empty()
                || item.label.len() > 256
                || item.detail.as_ref().is_some_and(|value| value.len() > 4096)
                || item
                    .insert_text
                    .as_ref()
                    .is_some_and(|value| value.len() > 4096)
                || item.value.as_ref().is_some_and(|value| value.len() > 4096)
            {
                return Err(PluginProviderError::InvalidItem);
            }
        }
        Ok(response.items)
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
            handler: "provide".to_owned(),
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
        let mut bad_handler = request("debug.buffers");
        bad_handler.handler = "bad-name".to_owned();
        assert_eq!(
            registry.register("debug", bad_handler),
            Err(PluginProviderError::InvalidHandler)
        );
    }

    #[test]
    fn provider_requests_are_single_use_and_bounded() {
        let mut requests = PluginProviderRequests::new();
        let provider = ProviderId::new("demo.buffers");
        let id = requests.start(provider.clone(), "buf", 20).expect("start");
        let result = requests
            .accept(ProviderResponse {
                request_id: id,
                items: vec![ProviderItem {
                    label: "buffer".to_owned(),
                    value: Some("1".to_owned()),
                    detail: None,
                    insert_text: None,
                }],
            })
            .expect("accept");
        assert_eq!(result.len(), 1);
        assert_eq!(
            requests.accept(ProviderResponse {
                request_id: id,
                items: Vec::new(),
            }),
            Err(PluginProviderError::UnknownRequest(id))
        );
        assert_eq!(
            requests.start(provider.clone(), "x", 0),
            Err(PluginProviderError::InvalidLimit)
        );
        assert_eq!(
            requests.start(provider, &"x".repeat(257), 1),
            Err(PluginProviderError::InvalidQuery)
        );
    }

    #[test]
    fn mismatched_response_id_cancels_request() {
        let mut requests = PluginProviderRequests::new();
        let id = requests
            .start(ProviderId::new("demo.buffers"), "", 1)
            .expect("start");
        assert_eq!(
            requests.accept_for(
                id,
                ProviderResponse {
                    request_id: id + 1,
                    items: Vec::new(),
                },
            ),
            Err(PluginProviderError::MismatchedRequest {
                expected: id,
                actual: id + 1,
            })
        );
        assert_eq!(
            requests.accept_for(
                id,
                ProviderResponse {
                    request_id: id,
                    items: Vec::new(),
                },
            ),
            Err(PluginProviderError::UnknownRequest(id))
        );
    }

    #[test]
    fn cancelling_a_provider_drops_its_requests() {
        let mut requests = PluginProviderRequests::new();
        let provider = ProviderId::new("demo.buffers");
        let id = requests.start(provider.clone(), "", 1).expect("start");
        requests.cancel_provider(&provider);
        assert_eq!(
            requests.accept(ProviderResponse {
                request_id: id,
                items: Vec::new(),
            }),
            Err(PluginProviderError::UnknownRequest(id))
        );
    }
}

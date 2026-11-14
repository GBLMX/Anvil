//! Typed plugin event registry, subscription policy, and coalescing.

use anvil_api::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) const MAX_SUBSCRIPTIONS: usize = 64;
const MAX_PENDING_EVENTS: usize = 128;

/// One event name with a fixed payload shape and coalescing policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PluginEvent {
    DiagnosticsChanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventPolicy {
    LatestByKey,
}

impl PluginEvent {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::DiagnosticsChanged => "diagnostics.changed",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "diagnostics.changed" => Some(Self::DiagnosticsChanged),
            _ => None,
        }
    }

    fn policy(self) -> EventPolicy {
        match self {
            Self::DiagnosticsChanged => EventPolicy::LatestByKey,
        }
    }

    fn coalesce_key(self, payload: &Value) -> Option<String> {
        match self {
            Self::DiagnosticsChanged => payload
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }
    }
}

/// Coalesced diagnostics summary sent by `diagnostics.changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsChanged {
    pub root: String,
    pub path: String,
    pub generation: u64,
    pub error_count: u32,
    pub warning_count: u32,
    pub first_error: Option<String>,
}

struct Subscription {
    events: HashSet<PluginEvent>,
    sequence: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PluginEventStats {
    pub delivered: u64,
    pub dropped: u64,
    pub handler_errors: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EventStats {
    pub delivered: u64,
    pub dropped: u64,
    pub handler_errors: u64,
    pub plugins: HashMap<String, PluginEventStats>,
}

#[derive(Debug)]
pub(crate) struct Delivery {
    pub(crate) plugin_id: String,
    pub(crate) subscription_id: u64,
    pub(crate) event: PluginEvent,
    pub(crate) sequence: u64,
    pub(crate) payload: Value,
}

/// Subscription and coalescing state, independent of host transport.
#[derive(Default)]
pub(crate) struct PluginEventBus {
    by_plugin: HashMap<String, HashMap<u64, Subscription>>,
    pending: VecDeque<Delivery>,
    pending_keys: HashMap<(u64, String), usize>,
    next_subscription: u64,
    stats: EventStats,
}

impl PluginEventBus {
    pub(crate) fn subscribe(
        &mut self,
        plugin_id: &str,
        events: &[String],
    ) -> Result<u64, (ApiError, String)> {
        let mut selected = HashSet::new();
        for event in events {
            let Some(event) = PluginEvent::from_name(event) else {
                return Err((
                    ApiError::InvalidParams,
                    format!("unsupported event: {event}"),
                ));
            };
            selected.insert(event);
        }
        if selected.is_empty() {
            return Err((ApiError::InvalidParams, "event list is empty".to_owned()));
        }
        let subscriptions = self.by_plugin.entry(plugin_id.to_owned()).or_default();
        if subscriptions.len() >= MAX_SUBSCRIPTIONS {
            return Err((
                ApiError::LimitExceeded,
                format!("subscription limit reached for plugin {plugin_id}"),
            ));
        }
        let id = self.next_subscription;
        self.next_subscription = self.next_subscription.saturating_add(1);
        subscriptions.insert(
            id,
            Subscription {
                events: selected,
                sequence: 0,
            },
        );
        Ok(id)
    }

    pub(crate) fn unsubscribe(&mut self, plugin_id: &str, subscription_id: u64) -> bool {
        let removed = self
            .by_plugin
            .get_mut(plugin_id)
            .is_some_and(|subscriptions| subscriptions.remove(&subscription_id).is_some());
        if removed {
            self.pending
                .retain(|delivery| delivery.subscription_id != subscription_id);
            self.rebuild_pending_keys();
        }
        removed
    }

    pub(crate) fn remove_plugin(&mut self, plugin_id: &str) {
        self.by_plugin.remove(plugin_id);
        self.pending
            .retain(|delivery| delivery.plugin_id != plugin_id);
        self.rebuild_pending_keys();
        self.stats.plugins.remove(plugin_id);
        self.recompute_handler_errors();
    }

    pub(crate) fn enqueue(&mut self, event: PluginEvent, payload: &Value) -> usize {
        let mut targets = Vec::new();
        for (plugin_id, subscriptions) in &mut self.by_plugin {
            for (subscription_id, subscription) in subscriptions {
                if !subscription.events.contains(&event) {
                    continue;
                }
                subscription.sequence = subscription.sequence.saturating_add(1);
                targets.push((plugin_id.clone(), *subscription_id, subscription.sequence));
            }
        }
        let policy = event.policy();
        let key = event.coalesce_key(payload);
        let mut queued = 0;
        for (plugin_id, subscription_id, sequence) in targets {
            queued += 1;
            let delivery = Delivery {
                plugin_id,
                subscription_id,
                event,
                sequence,
                payload: payload.clone(),
            };
            if policy == EventPolicy::LatestByKey
                && let Some(key) = key.as_ref()
                && let Some(index) = self.pending_keys.get(&(subscription_id, key.clone()))
            {
                self.pending[*index] = delivery;
                continue;
            }
            let index = self.pending.len();
            self.pending.push_back(delivery);
            if policy == EventPolicy::LatestByKey
                && let Some(key) = key.as_ref()
            {
                self.pending_keys
                    .insert((subscription_id, key.clone()), index);
            }
            self.trim_pending();
        }
        queued
    }

    pub(crate) fn take_deliveries(&mut self) -> Vec<Delivery> {
        self.pending_keys.clear();
        self.pending.drain(..).collect()
    }

    pub(crate) fn record_delivery(&mut self, plugin_id: &str, delivered: bool) {
        let stats = self.plugin_stats(plugin_id);
        if delivered {
            stats.delivered = stats.delivered.saturating_add(1);
            self.stats.delivered = self.stats.delivered.saturating_add(1);
        } else {
            stats.dropped = stats.dropped.saturating_add(1);
            self.stats.dropped = self.stats.dropped.saturating_add(1);
        }
    }

    pub(crate) fn record_handler_errors(&mut self, plugin_id: &str, errors: u64) {
        self.plugin_stats(plugin_id).handler_errors = errors;
        self.recompute_handler_errors();
    }

    pub(crate) fn stats(&self) -> EventStats {
        self.stats.clone()
    }

    fn plugin_stats(&mut self, plugin_id: &str) -> &mut PluginEventStats {
        self.stats.plugins.entry(plugin_id.to_owned()).or_default()
    }

    fn recompute_handler_errors(&mut self) {
        self.stats.handler_errors = self
            .stats
            .plugins
            .values()
            .map(|stats| stats.handler_errors)
            .sum();
    }

    fn trim_pending(&mut self) {
        while self.pending.len() > MAX_PENDING_EVENTS {
            let Some(dropped) = self.pending.pop_front() else {
                return;
            };
            let stats = self.plugin_stats(&dropped.plugin_id);
            stats.dropped = stats.dropped.saturating_add(1);
            self.stats.dropped = self.stats.dropped.saturating_add(1);
            self.rebuild_pending_keys();
        }
    }

    fn rebuild_pending_keys(&mut self) {
        self.pending_keys.clear();
        for (index, delivery) in self.pending.iter().enumerate() {
            if delivery.event.policy() == EventPolicy::LatestByKey
                && let Some(key) = delivery.event.coalesce_key(&delivery.payload)
            {
                self.pending_keys
                    .insert((delivery.subscription_id, key), index);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_rejects_unknown_events() {
        assert!(PluginEvent::from_name("editor.mouse").is_none());
        assert_eq!(
            PluginEvent::from_name("diagnostics.changed"),
            Some(PluginEvent::DiagnosticsChanged)
        );
    }

    #[test]
    fn subscription_limit_is_typed_and_bounded() {
        let mut bus = PluginEventBus::default();
        for _ in 0..MAX_SUBSCRIPTIONS {
            bus.subscribe("demo", &["diagnostics.changed".to_owned()])
                .expect("subscribe");
        }
        let (kind, _) = bus
            .subscribe("demo", &["diagnostics.changed".to_owned()])
            .expect_err("limit");
        assert_eq!(kind, ApiError::LimitExceeded);
    }

    #[test]
    fn latest_value_replaces_same_path() {
        let mut bus = PluginEventBus::default();
        bus.subscribe("demo", &["diagnostics.changed".to_owned()])
            .expect("subscribe");
        bus.enqueue(
            PluginEvent::DiagnosticsChanged,
            &serde_json::json!({"path": "a.rs", "generation": 1}),
        );
        bus.enqueue(
            PluginEvent::DiagnosticsChanged,
            &serde_json::json!({"path": "a.rs", "generation": 2}),
        );
        bus.enqueue(
            PluginEvent::DiagnosticsChanged,
            &serde_json::json!({"path": "b.rs", "generation": 3}),
        );
        let deliveries = bus.take_deliveries();
        assert_eq!(deliveries.len(), 2);
        assert_eq!(deliveries[0].payload["generation"], 2);
        assert_eq!(deliveries[1].payload["path"], "b.rs");
    }

    #[test]
    fn pending_queue_is_bounded_and_counted() {
        let mut bus = PluginEventBus::default();
        bus.subscribe("demo", &["diagnostics.changed".to_owned()])
            .expect("subscribe");
        for generation in 0..(MAX_PENDING_EVENTS as u64 + 4) {
            bus.enqueue(
                PluginEvent::DiagnosticsChanged,
                &serde_json::json!({"path": format!("{generation}.rs"), "generation": generation}),
            );
        }
        assert_eq!(bus.take_deliveries().len(), MAX_PENDING_EVENTS);
        assert!(bus.stats().dropped >= 4);
    }

    #[test]
    fn handler_errors_clear_to_zero_and_reset_on_remove() {
        let mut bus = PluginEventBus::default();
        bus.record_handler_errors("demo", 3);
        assert_eq!(bus.stats().handler_errors, 3);
        bus.record_handler_errors("demo", 0);
        assert_eq!(bus.stats().handler_errors, 0);
        bus.record_handler_errors("demo", 2);
        bus.remove_plugin("demo");
        let stats = bus.stats();
        assert_eq!(stats.handler_errors, 0);
        assert!(!stats.plugins.contains_key("demo"));
    }
}

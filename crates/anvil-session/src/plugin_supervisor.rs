//! Single-flight plugin host supervision and bounded failure handling.

use crate::editor_service::{EditorRequest, EditorService};
use crate::host_session::{HostEvent, SendOutcome};
use crate::plugin_events::{Delivery, PluginEvent, PluginEventBus};
use crate::plugin_session::PluginSession;
use anvil_api::ApiError;
use anvil_ipc::GatewayClient;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(crate) const START_TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) const STOP_DRAIN_TIMEOUT: Duration = Duration::from_millis(500);
pub(crate) const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_RUNNING_HOSTS: usize = 8;
const MAX_QUEUED_STARTS: usize = 8;
const MAX_PENDING_CALLS: usize = 32;
const MAX_FAILURES: usize = 3;
const FAILURE_WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostState {
    Unloaded,
    Queued,
    Registering,
    Starting,
    Ready,
    Draining,
    Stopped,
    Failed,
    Disabled,
}

impl HostState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unloaded => "unloaded",
            Self::Queued => "queued",
            Self::Registering => "registering",
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Draining => "draining",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartMode {
    Normal,
    Oom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecOutcome {
    Sent,
    Queued,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartOutcome {
    Started,
    Queued,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SupervisorEvent {
    State {
        plugin_id: String,
        state: HostState,
        detail: Option<String>,
    },
    Message {
        plugin_id: String,
        message: String,
    },
    Provider {
        plugin_id: String,
        id: u64,
        result: Result<serde_json::Value, String>,
    },
    ProviderAction {
        plugin_id: String,
        id: u64,
        result: Result<serde_json::Value, String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingCall {
    Exec,
    Provider {
        id: u64,
        handler: String,
        query: String,
        limit: u32,
    },
    ProviderAction {
        id: u64,
        handler: String,
        value: String,
    },
}

struct StartRequest<'a> {
    plugin_root: Option<&'a Path>,
    mode: StartMode,
    permissions: &'a [String],
    command: Option<&'a str>,
}

struct PendingStart {
    plugin_id: String,
    plugin_root: Option<std::path::PathBuf>,
    mode: StartMode,
    permissions: Vec<String>,
    command: Option<String>,
    workspace: std::path::PathBuf,
}

struct PluginEntry {
    state: HostState,
    host: Option<Box<dyn ManagedHost>>,
    started_at: Option<Instant>,
    last_activity: Option<Instant>,
    retry_at: Option<Instant>,
    failures: Vec<Instant>,
    pending: VecDeque<PendingCall>,
}

impl PluginEntry {
    fn new() -> Self {
        Self {
            state: HostState::Unloaded,
            host: None,
            started_at: None,
            last_activity: None,
            retry_at: None,
            failures: Vec::new(),
            pending: VecDeque::new(),
        }
    }
}

pub(crate) trait ManagedHost {
    fn poll(&mut self) -> Vec<HostEvent>;
    fn exec(&mut self) -> SendOutcome;
    fn event(
        &mut self,
        subscription_id: u64,
        event: &str,
        sequence: u64,
        payload: serde_json::Value,
    ) -> bool;
    fn event_stats(&mut self) -> bool;
    fn provider(&mut self, id: u64, handler: &str, query: &str, limit: u32) -> SendOutcome;
    fn provider_action(&mut self, id: u64, handler: &str, value: &str) -> SendOutcome;
    fn has_pending(&self) -> bool;
    fn drain(&mut self, timeout: Duration);
    fn stop(&mut self);
}

impl ManagedHost for PluginSession {
    fn poll(&mut self) -> Vec<HostEvent> {
        PluginSession::poll(self)
    }

    fn exec(&mut self) -> SendOutcome {
        PluginSession::exec(self)
    }

    fn event(
        &mut self,
        subscription_id: u64,
        event: &str,
        sequence: u64,
        payload: serde_json::Value,
    ) -> bool {
        PluginSession::event(self, subscription_id, event, sequence, payload)
    }

    fn event_stats(&mut self) -> bool {
        PluginSession::event_stats(self)
    }

    fn provider(&mut self, id: u64, handler: &str, query: &str, limit: u32) -> SendOutcome {
        PluginSession::provider(self, id, handler, query, limit)
    }

    fn provider_action(&mut self, id: u64, handler: &str, value: &str) -> SendOutcome {
        PluginSession::provider_action(self, id, handler, value)
    }

    fn has_pending(&self) -> bool {
        PluginSession::has_pending(self)
    }

    fn drain(&mut self, timeout: Duration) {
        PluginSession::drain(self, timeout);
    }

    fn stop(&mut self) {
        PluginSession::stop(self);
    }
}

pub(crate) trait HostLauncher {
    #[allow(clippy::too_many_arguments)]
    fn start(
        &self,
        gateway: Option<&mut GatewayClient>,
        workspace: &Path,
        plugin_id: &str,
        plugin_root: Option<&Path>,
        mode: StartMode,
        permissions: &[String],
        editor_socket: Option<&Path>,
        command: Option<&str>,
        identities: &Arc<Mutex<HashMap<[u8; 32], String>>>,
    ) -> Result<Box<dyn ManagedHost>, String>;
}

struct SystemLauncher;

impl HostLauncher for SystemLauncher {
    #[allow(clippy::too_many_arguments)]
    fn start(
        &self,
        gateway: Option<&mut GatewayClient>,
        workspace: &Path,
        plugin_id: &str,
        plugin_root: Option<&Path>,
        mode: StartMode,
        permissions: &[String],
        editor_socket: Option<&Path>,
        command: Option<&str>,
        identities: &Arc<Mutex<HashMap<[u8; 32], String>>>,
    ) -> Result<Box<dyn ManagedHost>, String> {
        let gateway = gateway.ok_or_else(|| "gateway unavailable".to_owned())?;
        let host = match (mode, plugin_root) {
            (StartMode::Oom, _) => PluginSession::start_oom(
                gateway,
                workspace,
                plugin_id,
                permissions,
                editor_socket,
                identities,
            ),
            (StartMode::Normal, Some(root)) => PluginSession::start_plugin(
                gateway,
                workspace,
                plugin_id,
                root,
                permissions,
                editor_socket,
                command,
                identities,
            ),
            (StartMode::Normal, None) => PluginSession::start(
                gateway,
                workspace,
                plugin_id,
                permissions,
                editor_socket,
                identities,
            ),
        }?;
        Ok(Box::new(host))
    }
}

pub struct PluginSupervisor {
    launcher: Box<dyn HostLauncher>,
    entries: HashMap<String, PluginEntry>,
    pending_starts: VecDeque<PendingStart>,
    editor_tx: Sender<EditorRequest>,
    editor_service: Option<EditorService>,
    editor_requests: Receiver<EditorRequest>,
    editor_identities: Arc<Mutex<HashMap<[u8; 32], String>>>,
    events: PluginEventBus,
}

impl Default for PluginSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginSupervisor {
    pub fn new() -> Self {
        let (editor_tx, editor_requests) = mpsc::channel();
        Self {
            launcher: Box::new(SystemLauncher),
            editor_identities: Arc::new(Mutex::new(HashMap::new())),
            entries: HashMap::new(),
            pending_starts: VecDeque::new(),
            editor_service: None,
            editor_tx,
            editor_requests,
            events: PluginEventBus::default(),
        }
    }

    #[cfg(test)]
    fn with_launcher(launcher: Box<dyn HostLauncher>) -> Self {
        let (editor_tx, editor_requests) = mpsc::channel();
        Self {
            launcher,
            editor_identities: Arc::new(Mutex::new(HashMap::new())),
            entries: HashMap::new(),
            pending_starts: VecDeque::new(),
            editor_service: None,
            editor_tx,
            editor_requests,
            events: PluginEventBus::default(),
        }
    }

    pub fn editor_socket(&self) -> Option<&Path> {
        self.editor_service.as_ref().map(EditorService::path)
    }

    pub fn poll_editor_request(&self) -> Option<EditorRequest> {
        self.editor_requests.try_recv().ok()
    }

    pub fn subscribe(
        &mut self,
        plugin_id: &str,
        events: Vec<String>,
    ) -> Result<u64, (ApiError, String)> {
        let entry = self.entries.get(plugin_id).ok_or_else(|| {
            (
                ApiError::NotFound,
                format!("plugin {plugin_id} is not loaded"),
            )
        })?;
        if !matches!(entry.state, HostState::Starting | HostState::Ready) {
            return Err((
                ApiError::Unavailable,
                format!("plugin {plugin_id} is not running"),
            ));
        }
        self.events.subscribe(plugin_id, &events)
    }

    pub fn unsubscribe(&mut self, plugin_id: &str, subscription_id: u64) -> bool {
        self.events.unsubscribe(plugin_id, subscription_id)
    }

    pub fn publish_event(&mut self, event: PluginEvent, payload: &serde_json::Value) -> usize {
        self.events.enqueue(event, payload)
    }

    pub(crate) fn flush_events(&mut self) -> usize {
        let deliveries = self.events.take_deliveries();
        let mut delivered = 0;
        for Delivery {
            plugin_id,
            subscription_id,
            event,
            sequence,
            payload,
        } in deliveries
        {
            let ok = self
                .entries
                .get_mut(&plugin_id)
                .and_then(|entry| entry.host.as_mut())
                .is_some_and(|host| host.event(subscription_id, event.name(), sequence, payload));
            self.events.record_delivery(&plugin_id, ok);
            if ok {
                delivered += 1;
            }
        }
        delivered
    }

    pub fn event_stats(&self) -> crate::plugin_events::EventStats {
        self.events.stats()
    }

    pub fn request_event_stats(&mut self, plugin_id: &str) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(plugin_id)
            .ok_or_else(|| format!("plugin {plugin_id} is not loaded"))?;
        if entry.state != HostState::Ready {
            return Err(format!("plugin {plugin_id} is not ready"));
        }
        if entry.host.as_mut().is_some_and(|host| host.event_stats()) {
            Ok(())
        } else {
            Err(format!("plugin {plugin_id} host event stats unavailable"))
        }
    }

    #[doc(hidden)]
    pub fn register_editor_identity(&self, token_hash: [u8; 32], plugin_id: &str) {
        self.editor_identities
            .lock()
            .expect("identities")
            .insert(token_hash, plugin_id.to_owned());
    }

    pub fn ensure_editor_service(&mut self) {
        if self.editor_service.is_none() {
            self.editor_service =
                EditorService::start(self.editor_tx.clone(), Arc::clone(&self.editor_identities))
                    .ok();
        }
    }

    pub fn activate(
        &mut self,
        plugin_id: &str,
        plugin_root: &Path,
        gateway: &mut GatewayClient,
        workspace: &Path,
    ) -> Result<StartOutcome, String> {
        self.start_at(
            Instant::now(),
            plugin_id,
            StartRequest {
                plugin_root: Some(plugin_root),
                mode: StartMode::Normal,
                permissions: &[],
                command: None,
            },
            Some(gateway),
            workspace,
        )
    }

    pub fn activate_command(
        &mut self,
        plugin_id: &str,
        plugin_root: &Path,
        command: &str,
        gateway: &mut GatewayClient,
        workspace: &Path,
    ) -> Result<StartOutcome, String> {
        self.start_at(
            Instant::now(),
            plugin_id,
            StartRequest {
                plugin_root: Some(plugin_root),
                mode: StartMode::Normal,
                permissions: &[],
                command: Some(command),
            },
            Some(gateway),
            workspace,
        )
    }

    pub fn start_debug(
        &mut self,
        plugin_id: &str,
        mode: StartMode,
        permissions: &[String],
        gateway: &mut GatewayClient,
        workspace: &Path,
    ) -> Result<StartOutcome, String> {
        self.start_at(
            Instant::now(),
            plugin_id,
            StartRequest {
                plugin_root: None,
                mode,
                permissions,
                command: None,
            },
            Some(gateway),
            workspace,
        )
    }

    fn start_at(
        &mut self,
        now: Instant,
        plugin_id: &str,
        request: StartRequest<'_>,
        mut gateway: Option<&mut GatewayClient>,
        workspace: &Path,
    ) -> Result<StartOutcome, String> {
        self.ensure_editor_service();
        let editor_socket = self
            .editor_service
            .as_ref()
            .map(|service| service.path().to_path_buf());
        if let Some(entry) = self.entries.get(plugin_id) {
            match entry.state {
                HostState::Disabled => {
                    return Err(format!(
                        "plugin {plugin_id} disabled; run :plugin-enable {plugin_id}"
                    ));
                }
                HostState::Queued => return Ok(StartOutcome::Queued),
                HostState::Failed | HostState::Unloaded | HostState::Stopped => {}
                _ => return Err(format!("plugin {plugin_id} already active")),
            }
            if let Some(retry_at) = entry.retry_at
                && now < retry_at
            {
                return Err(format!(
                    "plugin {plugin_id} retry in {}ms",
                    retry_at.duration_since(now).as_millis()
                ));
            }
        }
        if self.running_hosts() >= MAX_RUNNING_HOSTS {
            if self
                .pending_starts
                .iter()
                .any(|pending| pending.plugin_id == plugin_id)
            {
                return Ok(StartOutcome::Queued);
            }
            if self.pending_starts.len() >= MAX_QUEUED_STARTS {
                return Err("plugin start queue full".to_owned());
            }
            let entry = self
                .entries
                .entry(plugin_id.to_owned())
                .or_insert_with(PluginEntry::new);
            entry.state = HostState::Queued;
            entry.started_at = None;
            entry.last_activity = None;
            entry.retry_at = None;
            entry.pending.clear();
            self.pending_starts.push_back(PendingStart {
                plugin_id: plugin_id.to_owned(),
                plugin_root: request.plugin_root.map(Path::to_path_buf),
                mode: request.mode,
                permissions: request.permissions.to_vec(),
                command: request.command.map(str::to_owned),
                workspace: workspace.to_path_buf(),
            });
            return Ok(StartOutcome::Queued);
        }
        let entry = self
            .entries
            .entry(plugin_id.to_owned())
            .or_insert_with(PluginEntry::new);
        entry.state = HostState::Registering;
        entry.started_at = None;
        entry.last_activity = None;
        entry.retry_at = None;
        entry.pending.clear();

        match self.launcher.start(
            gateway.as_deref_mut(),
            workspace,
            plugin_id,
            request.plugin_root,
            request.mode,
            request.permissions,
            editor_socket.as_deref(),
            request.command,
            &self.editor_identities,
        ) {
            Ok(host) => {
                let entry = self
                    .entries
                    .get_mut(plugin_id)
                    .expect("plugin entry exists after registration");
                entry.host = Some(host);
                entry.state = HostState::Starting;
                entry.started_at = Some(now);
                Ok(StartOutcome::Started)
            }
            Err(error) => {
                self.fail_at(plugin_id, now, error.clone(), gateway);
                Err(error)
            }
        }
    }

    fn running_hosts(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| {
                matches!(
                    entry.state,
                    HostState::Starting | HostState::Ready | HostState::Draining
                )
            })
            .count()
    }

    fn drain_pending(&mut self, now: Instant, mut gateway: Option<&mut GatewayClient>) {
        while self.running_hosts() < MAX_RUNNING_HOSTS {
            let Some(pending) = self.pending_starts.pop_front() else {
                return;
            };
            if let Some(entry) = self.entries.get_mut(&pending.plugin_id)
                && entry.state == HostState::Queued
            {
                entry.state = HostState::Stopped;
            }
            let request = StartRequest {
                plugin_root: pending.plugin_root.as_deref(),
                mode: pending.mode,
                permissions: &pending.permissions,
                command: pending.command.as_deref(),
            };
            let _ = self.start_at(
                now,
                &pending.plugin_id,
                request,
                gateway.as_deref_mut(),
                &pending.workspace,
            );
        }
    }

    fn reclaim_idle(
        &mut self,
        now: Instant,
        mut gateway: Option<&mut GatewayClient>,
        events: &mut Vec<SupervisorEvent>,
    ) {
        let ids = self.entries.keys().cloned().collect::<Vec<_>>();
        for plugin_id in ids {
            let idle = self.entries.get(&plugin_id).is_some_and(|entry| {
                entry.state == HostState::Ready
                    && entry.pending.is_empty()
                    && entry
                        .last_activity
                        .is_some_and(|activity| now.duration_since(activity) >= IDLE_TIMEOUT)
                    && entry.host.as_ref().is_some_and(|host| !host.has_pending())
            });
            if !idle {
                continue;
            }
            let entry = self.entries.get_mut(&plugin_id).expect("idle plugin entry");
            entry.state = HostState::Draining;
            if let Some(mut host) = entry.host.take() {
                host.drain(STOP_DRAIN_TIMEOUT);
                host.stop();
            }
            entry.state = HostState::Stopped;
            self.events.remove_plugin(&plugin_id);
            entry.last_activity = None;
            entry.pending.clear();
            if let Some(gateway) = gateway.as_deref_mut() {
                let _ = gateway.revoke_plugin(&plugin_id);
            }
            events.push(SupervisorEvent::State {
                plugin_id,
                state: HostState::Stopped,
                detail: Some("idle".to_owned()),
            });
        }
    }

    /// Removes a queued provider request. In-flight host work cannot be
    /// aborted because the host control call is synchronous.
    pub fn cancel_provider_request(&mut self, request_id: u64) {
        for entry in self.entries.values_mut() {
            entry.pending.retain(|call| {
                !matches!(
                    call,
                    PendingCall::Provider { id, .. }
                    | PendingCall::ProviderAction { id, .. }
                    if *id == request_id
                )
            });
        }
    }

    pub fn provider(
        &mut self,
        plugin_id: &str,
        id: u64,
        handler: &str,
        query: &str,
        limit: u32,
    ) -> Result<ExecOutcome, String> {
        let entry = self
            .entries
            .get_mut(plugin_id)
            .ok_or_else(|| format!("plugin {plugin_id} is not loaded"))?;
        match entry.state {
            HostState::Ready => {
                let outcome = entry.host.as_mut().map_or(SendOutcome::Closed, |host| {
                    host.provider(id, handler, query, limit)
                });
                match outcome {
                    SendOutcome::Sent => {
                        entry.last_activity = Some(Instant::now());
                        Ok(ExecOutcome::Sent)
                    }
                    SendOutcome::Full => {
                        if entry.pending.len() >= MAX_PENDING_CALLS {
                            return Err(format!("plugin {plugin_id} request queue full"));
                        }
                        entry.pending.push_back(PendingCall::Provider {
                            id,
                            handler: handler.to_owned(),
                            query: query.to_owned(),
                            limit,
                        });
                        Ok(ExecOutcome::Queued)
                    }
                    SendOutcome::Closed => Err(format!("plugin {plugin_id} host channel closed")),
                }
            }
            HostState::Registering | HostState::Starting => {
                if entry.pending.len() >= MAX_PENDING_CALLS {
                    return Err(format!("plugin {plugin_id} request queue full"));
                }
                entry.pending.push_back(PendingCall::Provider {
                    id,
                    handler: handler.to_owned(),
                    query: query.to_owned(),
                    limit,
                });
                Ok(ExecOutcome::Queued)
            }
            HostState::Disabled => Err(format!(
                "plugin {plugin_id} disabled; run :plugin-enable {plugin_id}"
            )),
            _ => Err(format!("plugin {plugin_id} is not running")),
        }
    }

    pub fn provider_action(
        &mut self,
        plugin_id: &str,
        id: u64,
        handler: &str,
        value: &str,
    ) -> Result<ExecOutcome, String> {
        let entry = self
            .entries
            .get_mut(plugin_id)
            .ok_or_else(|| format!("plugin {plugin_id} is not loaded"))?;
        match entry.state {
            HostState::Ready => {
                let outcome = entry.host.as_mut().map_or(SendOutcome::Closed, |host| {
                    host.provider_action(id, handler, value)
                });
                match outcome {
                    SendOutcome::Sent => {
                        entry.last_activity = Some(Instant::now());
                        Ok(ExecOutcome::Sent)
                    }
                    SendOutcome::Full => {
                        if entry.pending.len() >= MAX_PENDING_CALLS {
                            return Err(format!("plugin {plugin_id} request queue full"));
                        }
                        entry.pending.push_back(PendingCall::ProviderAction {
                            id,
                            handler: handler.to_owned(),
                            value: value.to_owned(),
                        });
                        Ok(ExecOutcome::Queued)
                    }
                    SendOutcome::Closed => Err(format!("plugin {plugin_id} host channel closed")),
                }
            }
            HostState::Registering | HostState::Starting => {
                if entry.pending.len() >= MAX_PENDING_CALLS {
                    return Err(format!("plugin {plugin_id} request queue full"));
                }
                entry.pending.push_back(PendingCall::ProviderAction {
                    id,
                    handler: handler.to_owned(),
                    value: value.to_owned(),
                });
                Ok(ExecOutcome::Queued)
            }
            HostState::Disabled => Err(format!(
                "plugin {plugin_id} disabled; run :plugin-enable {plugin_id}"
            )),
            _ => Err(format!("plugin {plugin_id} is not running")),
        }
    }

    pub fn exec(&mut self, plugin_id: &str) -> Result<ExecOutcome, String> {
        let entry = self
            .entries
            .get_mut(plugin_id)
            .ok_or_else(|| format!("plugin {plugin_id} is not loaded"))?;
        match entry.state {
            HostState::Ready => {
                let outcome = entry
                    .host
                    .as_mut()
                    .map_or(SendOutcome::Closed, |host| host.exec());
                match outcome {
                    SendOutcome::Sent => {
                        entry.last_activity = Some(Instant::now());
                        Ok(ExecOutcome::Sent)
                    }
                    SendOutcome::Full => {
                        if entry.pending.len() >= MAX_PENDING_CALLS {
                            return Err(format!("plugin {plugin_id} request queue full"));
                        }
                        entry.pending.push_back(PendingCall::Exec);
                        Ok(ExecOutcome::Queued)
                    }
                    SendOutcome::Closed => Err(format!("plugin {plugin_id} host channel closed")),
                }
            }
            HostState::Registering | HostState::Starting => {
                if entry.pending.len() >= MAX_PENDING_CALLS {
                    return Err(format!("plugin {plugin_id} request queue full"));
                }
                entry.pending.push_back(PendingCall::Exec);
                Ok(ExecOutcome::Queued)
            }
            HostState::Disabled => Err(format!(
                "plugin {plugin_id} disabled; run :plugin-enable {plugin_id}"
            )),
            _ => Err(format!("plugin {plugin_id} is not running")),
        }
    }

    pub fn stop(
        &mut self,
        plugin_id: &str,
        gateway: Option<&mut GatewayClient>,
    ) -> Result<(), String> {
        if self
            .entries
            .get(plugin_id)
            .is_some_and(|entry| entry.state == HostState::Queued)
        {
            self.pending_starts
                .retain(|pending| pending.plugin_id != plugin_id);
            let entry = self
                .entries
                .get_mut(plugin_id)
                .expect("queued plugin entry");
            entry.state = HostState::Stopped;
            entry.last_activity = None;
            entry.pending.clear();
            return Ok(());
        }
        let Some(entry) = self.entries.get_mut(plugin_id) else {
            return Err(format!("plugin {plugin_id} is not loaded"));
        };
        if matches!(entry.state, HostState::Unloaded | HostState::Stopped) {
            return Err(format!("plugin {plugin_id} is not running"));
        }
        if entry.state == HostState::Disabled {
            return Err(format!("plugin {plugin_id} is disabled"));
        }
        entry.state = HostState::Draining;
        if let Some(mut host) = entry.host.take() {
            host.drain(STOP_DRAIN_TIMEOUT);
            host.stop();
        }
        if let Some(gateway) = gateway {
            let _ = gateway.revoke_plugin(plugin_id);
        }
        entry.state = HostState::Stopped;
        self.events.remove_plugin(plugin_id);
        entry.started_at = None;
        entry.last_activity = None;
        entry.retry_at = None;
        entry.pending.clear();
        Ok(())
    }

    pub fn enable(&mut self, plugin_id: &str) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(plugin_id)
            .ok_or_else(|| format!("plugin {plugin_id} is not loaded"))?;
        if entry.state != HostState::Disabled {
            return Err(format!("plugin {plugin_id} is not disabled"));
        }
        entry.state = HostState::Stopped;
        entry.last_activity = None;
        entry.failures.clear();
        entry.retry_at = None;
        Ok(())
    }

    pub fn state(&self, plugin_id: &str) -> HostState {
        self.entries
            .get(plugin_id)
            .map(|entry| entry.state)
            .unwrap_or(HostState::Unloaded)
    }

    pub fn poll(&mut self, gateway: Option<&mut GatewayClient>) -> Vec<SupervisorEvent> {
        self.poll_at(Instant::now(), gateway)
    }

    fn poll_at(
        &mut self,
        now: Instant,
        mut gateway: Option<&mut GatewayClient>,
    ) -> Vec<SupervisorEvent> {
        let ids = self.entries.keys().cloned().collect::<Vec<_>>();
        let mut events = Vec::new();
        self.flush_events();
        for plugin_id in ids {
            if let Some(entry) = self.entries.get(&plugin_id)
                && entry.state == HostState::Starting
                && entry
                    .started_at
                    .is_some_and(|started| now >= started + START_TIMEOUT)
            {
                let detail = self.fail_at(
                    &plugin_id,
                    now,
                    "host start timeout".to_owned(),
                    gateway.as_deref_mut(),
                );
                events.push(SupervisorEvent::State {
                    plugin_id: plugin_id.clone(),
                    state: self.state(&plugin_id),
                    detail: Some(detail),
                });
                continue;
            }

            let host_events = self
                .entries
                .get_mut(&plugin_id)
                .and_then(|entry| entry.host.as_mut())
                .map(|host| host.poll())
                .unwrap_or_default();
            for event in host_events {
                match event {
                    HostEvent::Ready => {
                        let Some(entry) = self.entries.get_mut(&plugin_id) else {
                            continue;
                        };
                        if entry.state != HostState::Starting {
                            continue;
                        }
                        entry.state = HostState::Ready;
                        entry.started_at = None;
                        entry.last_activity = Some(now);
                        events.push(SupervisorEvent::State {
                            plugin_id: plugin_id.clone(),
                            state: HostState::Ready,
                            detail: None,
                        });
                        self.flush_pending(&plugin_id, now, gateway.as_deref_mut(), &mut events);
                    }
                    HostEvent::Pong => {
                        if let Some(entry) = self.entries.get_mut(&plugin_id) {
                            entry.last_activity = Some(now);
                        }
                        events.push(SupervisorEvent::Message {
                            plugin_id: plugin_id.clone(),
                            message: "ready".to_owned(),
                        });
                    }
                    HostEvent::ExecUnsupported => events.push(SupervisorEvent::Message {
                        plugin_id: plugin_id.clone(),
                        message: "sys.exec unsupported".to_owned(),
                    }),
                    HostEvent::ProviderAction { id, result } => {
                        if let Some(entry) = self.entries.get_mut(&plugin_id) {
                            entry.last_activity = Some(now);
                        }
                        events.push(SupervisorEvent::ProviderAction {
                            plugin_id: plugin_id.clone(),
                            id,
                            result,
                        });
                    }
                    HostEvent::Provider { id, result } => {
                        if let Some(entry) = self.entries.get_mut(&plugin_id) {
                            entry.last_activity = Some(now);
                        }
                        events.push(SupervisorEvent::Provider {
                            plugin_id: plugin_id.clone(),
                            id,
                            result,
                        });
                    }
                    HostEvent::Error(error) => {
                        let detail = self.fail_at(&plugin_id, now, error, gateway.as_deref_mut());
                        events.push(SupervisorEvent::State {
                            plugin_id: plugin_id.clone(),
                            state: self.state(&plugin_id),
                            detail: Some(detail),
                        });
                        break;
                    }
                    HostEvent::EventStats(errors) => {
                        self.events.record_handler_errors(&plugin_id, errors);
                        events.push(SupervisorEvent::Message {
                            plugin_id: plugin_id.clone(),
                            message: format!("event handler errors: {errors}"),
                        });
                    }
                    HostEvent::Exited(exit) => {
                        let detail = self.fail_at(
                            &plugin_id,
                            now,
                            format!("{exit:?}"),
                            gateway.as_deref_mut(),
                        );
                        events.push(SupervisorEvent::State {
                            plugin_id: plugin_id.clone(),
                            state: self.state(&plugin_id),
                            detail: Some(detail),
                        });
                        break;
                    }
                }
            }
        }
        self.reclaim_idle(now, gateway.as_deref_mut(), &mut events);
        self.drain_pending(now, gateway);
        events
    }

    fn flush_pending(
        &mut self,
        plugin_id: &str,
        now: Instant,
        gateway: Option<&mut GatewayClient>,
        events: &mut Vec<SupervisorEvent>,
    ) {
        loop {
            let Some(call) = self
                .entries
                .get_mut(plugin_id)
                .and_then(|entry| entry.pending.pop_front())
            else {
                return;
            };
            let outcome = self
                .entries
                .get_mut(plugin_id)
                .and_then(|entry| entry.host.as_mut())
                .map_or(SendOutcome::Closed, |host| match &call {
                    PendingCall::Exec => host.exec(),
                    PendingCall::Provider {
                        id,
                        handler,
                        query,
                        limit,
                    } => host.provider(*id, handler, query, *limit),
                    PendingCall::ProviderAction { id, handler, value } => {
                        host.provider_action(*id, handler, value)
                    }
                });
            match outcome {
                SendOutcome::Sent => {
                    if let Some(entry) = self.entries.get_mut(plugin_id) {
                        entry.last_activity = Some(now);
                    }
                    continue;
                }
                SendOutcome::Full => {
                    if let Some(entry) = self.entries.get_mut(plugin_id) {
                        entry.pending.push_front(call);
                    }
                    return;
                }
                SendOutcome::Closed => {
                    let detail =
                        self.fail_at(plugin_id, now, "host channel closed".to_owned(), gateway);
                    events.push(SupervisorEvent::State {
                        plugin_id: plugin_id.to_owned(),
                        state: self.state(plugin_id),
                        detail: Some(detail),
                    });
                    return;
                }
            }
        }
    }

    fn fail_at(
        &mut self,
        plugin_id: &str,
        now: Instant,
        detail: String,
        gateway: Option<&mut GatewayClient>,
    ) -> String {
        let Some(entry) = self.entries.get_mut(plugin_id) else {
            return detail;
        };
        if let Some(mut host) = entry.host.take() {
            host.drain(STOP_DRAIN_TIMEOUT);
            host.stop();
        }
        if let Some(gateway) = gateway {
            let _ = gateway.revoke_plugin(plugin_id);
        }
        entry.pending.clear();
        self.events.remove_plugin(plugin_id);
        entry.started_at = None;
        entry.last_activity = None;
        entry
            .failures
            .retain(|failure| now.duration_since(*failure) <= FAILURE_WINDOW);
        entry.failures.push(now);
        if entry.failures.len() >= MAX_FAILURES {
            entry.state = HostState::Disabled;
            entry.retry_at = None;
            format!("{detail}; disabled after {} failures", entry.failures.len())
        } else {
            let delay = Duration::from_millis(250)
                .saturating_mul(1 << (entry.failures.len().saturating_sub(1)) as u32)
                .min(Duration::from_secs(2));
            entry.retry_at = Some(now + delay);
            entry.state = HostState::Failed;
            format!("{detail}; retry in {}ms", delay.as_millis())
        }
    }

    pub fn shutdown(&mut self, mut gateway: Option<&mut GatewayClient>) {
        let ids = self.entries.keys().cloned().collect::<Vec<_>>();
        for plugin_id in ids {
            let _ = self.stop(&plugin_id, gateway.as_deref_mut());
        }
    }
}

impl Drop for PluginSupervisor {
    fn drop(&mut self) {
        for entry in self.entries.values_mut() {
            if let Some(host) = entry.host.as_mut() {
                host.stop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_events::MAX_SUBSCRIPTIONS;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct FakeHostState {
        events: VecDeque<HostEvent>,
        execs: usize,
        providers: usize,
        pending: usize,
        drains: usize,
        stopped: bool,
        send_full: bool,
        deliveries: Vec<(u64, String, u64, serde_json::Value)>,
    }

    struct FakeHost {
        state: Arc<Mutex<FakeHostState>>,
    }

    impl ManagedHost for FakeHost {
        fn poll(&mut self) -> Vec<HostEvent> {
            self.state
                .lock()
                .expect("fake host")
                .events
                .drain(..)
                .collect()
        }

        fn exec(&mut self) -> SendOutcome {
            let mut state = self.state.lock().expect("fake host");
            if state.send_full {
                return SendOutcome::Full;
            }
            state.execs += 1;
            state.pending += 1;
            SendOutcome::Sent
        }

        fn event(
            &mut self,
            subscription_id: u64,
            event: &str,
            sequence: u64,
            payload: serde_json::Value,
        ) -> bool {
            self.state.lock().expect("fake host").deliveries.push((
                subscription_id,
                event.to_owned(),
                sequence,
                payload,
            ));
            true
        }

        fn event_stats(&mut self) -> bool {
            true
        }

        fn provider(&mut self, _id: u64, _handler: &str, _query: &str, _limit: u32) -> SendOutcome {
            let mut state = self.state.lock().expect("fake host");
            if state.send_full {
                return SendOutcome::Full;
            }
            state.providers += 1;
            state.pending += 1;
            SendOutcome::Sent
        }

        fn provider_action(&mut self, _id: u64, _handler: &str, _value: &str) -> SendOutcome {
            let mut state = self.state.lock().expect("fake host");
            if state.send_full {
                return SendOutcome::Full;
            }
            state.pending += 1;
            SendOutcome::Sent
        }

        fn has_pending(&self) -> bool {
            self.state.lock().expect("fake host").pending > 0
        }

        fn drain(&mut self, _timeout: Duration) {
            self.state.lock().expect("fake host").drains += 1;
        }

        fn stop(&mut self) {
            self.state.lock().expect("fake host").stopped = true;
        }
    }

    struct FakeLauncher {
        starts: AtomicUsize,
        failure: Mutex<Option<String>>,
        state: Arc<Mutex<FakeHostState>>,
    }

    impl FakeLauncher {
        fn start_impl(
            &self,
            _gateway: Option<&mut GatewayClient>,
            _workspace: &Path,
            _plugin_id: &str,
            _plugin_root: Option<&Path>,
            _mode: StartMode,
            _permissions: &[String],
        ) -> Result<Box<dyn ManagedHost>, String> {
            self.starts.fetch_add(1, Ordering::Relaxed);
            if let Some(error) = self.failure.lock().expect("fake launcher").clone() {
                return Err(error);
            }
            Ok(Box::new(FakeHost {
                state: Arc::clone(&self.state),
            }))
        }
    }

    impl HostLauncher for FakeLauncher {
        #[allow(clippy::too_many_arguments)]
        fn start(
            &self,
            gateway: Option<&mut GatewayClient>,
            workspace: &Path,
            plugin_id: &str,
            plugin_root: Option<&Path>,
            mode: StartMode,
            permissions: &[String],
            _editor_socket: Option<&Path>,
            _command: Option<&str>,
            _identities: &Arc<Mutex<HashMap<[u8; 32], String>>>,
        ) -> Result<Box<dyn ManagedHost>, String> {
            self.start_impl(
                gateway,
                workspace,
                plugin_id,
                plugin_root,
                mode,
                permissions,
            )
        }
    }

    struct SharedLauncher(Arc<FakeLauncher>);

    impl HostLauncher for SharedLauncher {
        #[allow(clippy::too_many_arguments)]
        fn start(
            &self,
            gateway: Option<&mut GatewayClient>,
            workspace: &Path,
            plugin_id: &str,
            plugin_root: Option<&Path>,
            mode: StartMode,
            permissions: &[String],
            _editor_socket: Option<&Path>,
            _command: Option<&str>,
            _identities: &Arc<Mutex<HashMap<[u8; 32], String>>>,
        ) -> Result<Box<dyn ManagedHost>, String> {
            self.0.start_impl(
                gateway,
                workspace,
                plugin_id,
                plugin_root,
                mode,
                permissions,
            )
        }
    }

    fn supervisor() -> (PluginSupervisor, Arc<FakeLauncher>) {
        let state = Arc::new(Mutex::new(FakeHostState::default()));
        let launcher = Arc::new(FakeLauncher {
            starts: AtomicUsize::new(0),
            failure: Mutex::new(None),
            state,
        });
        (
            PluginSupervisor::with_launcher(Box::new(SharedLauncher(Arc::clone(&launcher)))),
            launcher,
        )
    }

    fn start(
        supervisor: &mut PluginSupervisor,
        now: Instant,
        plugin_id: &str,
    ) -> Result<StartOutcome, String> {
        supervisor.start_at(
            now,
            plugin_id,
            StartRequest {
                plugin_root: Some(Path::new(".")),
                mode: StartMode::Normal,
                permissions: &[],
                command: None,
            },
            None,
            Path::new("."),
        )
    }

    #[test]
    fn start_is_single_flight_and_queues_until_ready() {
        let (mut supervisor, launcher) = supervisor();
        let now = Instant::now();
        start(&mut supervisor, now, "demo").expect("start");
        start(&mut supervisor, now, "demo").expect_err("duplicate start");
        assert_eq!(launcher.starts.load(Ordering::Relaxed), 1);
        assert_eq!(supervisor.exec("demo").expect("queue"), ExecOutcome::Queued);

        launcher
            .state
            .lock()
            .expect("fake host")
            .events
            .push_back(HostEvent::Ready);
        let events = supervisor.poll_at(now, None);
        assert!(events.iter().any(|event| matches!(
            event,
            SupervisorEvent::State {
                state: HostState::Ready,
                ..
            }
        )));
        assert_eq!(supervisor.state("demo"), HostState::Ready);
        assert_eq!(launcher.state.lock().expect("fake host").execs, 1);
    }

    #[test]
    fn full_command_queue_stays_ready_and_requeues() {
        let (mut supervisor, launcher) = supervisor();
        let now = Instant::now();
        start(&mut supervisor, now, "demo").expect("start");
        launcher
            .state
            .lock()
            .expect("fake host")
            .events
            .push_back(HostEvent::Ready);
        supervisor.poll_at(now, None);
        launcher.state.lock().expect("fake host").send_full = true;
        assert_eq!(
            supervisor.exec("demo").expect("queue full is transient"),
            ExecOutcome::Queued
        );
        assert_eq!(supervisor.state("demo"), HostState::Ready);
        supervisor.poll_at(now, None);
        assert_eq!(supervisor.state("demo"), HostState::Ready);
        assert_eq!(
            supervisor.entries.get("demo").expect("entry").pending.len(),
            1
        );
    }

    #[test]
    fn event_subscription_delivers_sequences_and_unsubscribes() {
        let (mut supervisor, launcher) = supervisor();
        let now = Instant::now();
        start(&mut supervisor, now, "demo").expect("start");
        assert!(
            supervisor
                .subscribe("demo", vec!["editor.mouse".to_owned()])
                .is_err()
        );
        let subscription = supervisor
            .subscribe("demo", vec!["diagnostics.changed".to_owned()])
            .expect("subscribe");
        let first = serde_json::json!({ "path": "src/main.rs", "generation": 3 });
        let second = serde_json::json!({ "path": "src/lib.rs", "generation": 4 });
        assert_eq!(
            supervisor.publish_event(PluginEvent::DiagnosticsChanged, &first),
            1
        );
        assert_eq!(
            supervisor.publish_event(PluginEvent::DiagnosticsChanged, &second),
            1
        );
        assert_eq!(supervisor.flush_events(), 2);
        {
            let state = launcher.state.lock().expect("fake host");
            assert_eq!(state.deliveries.len(), 2);
            assert_eq!(state.deliveries[0].0, subscription);
            assert_eq!(state.deliveries[0].1, "diagnostics.changed");
            assert_eq!(state.deliveries[0].2, 1);
            assert_eq!(state.deliveries[1].2, 2);
            assert_eq!(state.deliveries[1].3, second);
        }
        let stats = supervisor.event_stats();
        assert_eq!(stats.delivered, 2);
        assert_eq!(stats.dropped, 0);
        assert!(supervisor.unsubscribe("demo", subscription));
        assert_eq!(
            supervisor.publish_event(PluginEvent::DiagnosticsChanged, &first),
            0
        );
    }

    #[test]
    fn event_handler_errors_are_recorded_per_plugin() {
        let (mut supervisor, launcher) = supervisor();
        let now = Instant::now();
        start(&mut supervisor, now, "demo").expect("start");
        launcher
            .state
            .lock()
            .expect("fake host")
            .events
            .push_back(HostEvent::EventStats(3));
        supervisor.poll_at(now, None);
        let stats = supervisor.event_stats();
        assert_eq!(stats.handler_errors, 3);
        assert_eq!(
            stats.plugins.get("demo").map(|stats| stats.handler_errors),
            Some(3)
        );
    }

    #[test]
    fn subscription_limit_is_bounded() {
        let (mut supervisor, _) = supervisor();
        let now = Instant::now();
        start(&mut supervisor, now, "demo").expect("start");
        for _ in 0..MAX_SUBSCRIPTIONS {
            supervisor
                .subscribe("demo", vec!["diagnostics.changed".to_owned()])
                .expect("subscribe");
        }
        let (kind, _) = supervisor
            .subscribe("demo", vec!["diagnostics.changed".to_owned()])
            .expect_err("subscription limit");
        assert_eq!(kind, ApiError::LimitExceeded);
    }

    #[test]
    fn cancelling_queued_provider_prevents_send() {
        let (mut supervisor, launcher) = supervisor();
        let now = Instant::now();
        start(&mut supervisor, now, "demo").expect("start");
        assert_eq!(
            supervisor
                .provider("demo", 7, "provide", "buf", 10)
                .expect("queue"),
            ExecOutcome::Queued
        );
        supervisor.cancel_provider_request(7);
        launcher
            .state
            .lock()
            .expect("fake host")
            .events
            .push_back(HostEvent::Ready);
        let _ = supervisor.poll_at(now, None);
        assert_eq!(launcher.state.lock().expect("fake host").providers, 0);
    }

    #[test]
    fn queued_provider_is_sent_after_ready() {
        let (mut supervisor, launcher) = supervisor();
        let now = Instant::now();
        start(&mut supervisor, now, "demo").expect("start");
        assert_eq!(
            supervisor
                .provider("demo", 7, "provide", "buf", 10)
                .expect("queue"),
            ExecOutcome::Queued
        );
        launcher
            .state
            .lock()
            .expect("fake host")
            .events
            .push_back(HostEvent::Ready);
        let _ = supervisor.poll_at(now, None);
        assert_eq!(launcher.state.lock().expect("fake host").providers, 1);
    }

    #[test]
    fn start_timeout_stops_the_host() {
        let (mut supervisor, launcher) = supervisor();
        let now = Instant::now();
        start(&mut supervisor, now, "demo").expect("start");
        let events = supervisor.poll_at(now + START_TIMEOUT, None);
        assert!(events.iter().any(|event| matches!(
            event,
            SupervisorEvent::State {
                state: HostState::Failed,
                ..
            }
        )));
        assert!(launcher.state.lock().expect("fake host").stopped);
        assert_eq!(launcher.starts.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn repeated_failures_disable_until_enabled() {
        let (mut supervisor, launcher) = supervisor();
        let mut now = Instant::now();
        for _ in 0..MAX_FAILURES {
            start(&mut supervisor, now, "demo").expect("start");
            launcher
                .state
                .lock()
                .expect("fake host")
                .events
                .push_back(HostEvent::Error("boom".to_owned()));
            supervisor.poll_at(now, None);
            now += Duration::from_secs(3);
        }
        assert_eq!(supervisor.state("demo"), HostState::Disabled);
        start(&mut supervisor, now, "demo").expect_err("disabled");
        supervisor.enable("demo").expect("enable");
        start(&mut supervisor, now, "demo").expect("restart");
        assert_eq!(launcher.starts.load(Ordering::Relaxed), MAX_FAILURES + 1);
    }

    #[test]
    fn start_failure_enters_backoff() {
        let (mut supervisor, launcher) = supervisor();
        *launcher.failure.lock().expect("fake launcher") = Some("spawn failed".to_owned());
        let now = Instant::now();
        let error = start(&mut supervisor, now, "demo").expect_err("start failure");
        assert!(error.contains("spawn failed"), "{error}");
        assert_eq!(supervisor.state("demo"), HostState::Failed);
        let error = start(&mut supervisor, now, "demo").expect_err("backoff");
        assert!(error.contains("retry in"), "{error}");
        *launcher.failure.lock().expect("fake launcher") = None;
        start(&mut supervisor, now + Duration::from_millis(251), "demo")
            .expect("retry after backoff");
    }
    #[test]
    fn start_admission_and_request_queue_are_bounded() {
        let (mut supervisor, launcher) = supervisor();
        let now = Instant::now();
        for index in 0..MAX_RUNNING_HOSTS {
            assert_eq!(
                start(&mut supervisor, now, &format!("plugin-{index}")).expect("start"),
                StartOutcome::Started
            );
        }
        for index in 0..MAX_QUEUED_STARTS {
            assert_eq!(
                start(&mut supervisor, now, &format!("queued-{index}")).expect("queue"),
                StartOutcome::Queued
            );
        }
        start(&mut supervisor, now, "overflow").expect_err("start queue cap");
        assert_eq!(launcher.starts.load(Ordering::Relaxed), MAX_RUNNING_HOSTS);
        supervisor.stop("plugin-0", None).expect("stop");
        supervisor.poll_at(now, None);
        assert_eq!(
            launcher.starts.load(Ordering::Relaxed),
            MAX_RUNNING_HOSTS + 1
        );

        for _ in 0..MAX_PENDING_CALLS {
            assert_eq!(
                supervisor.exec("plugin-1").expect("queue"),
                ExecOutcome::Queued
            );
        }
        supervisor.exec("plugin-1").expect_err("request cap");
    }

    #[test]
    fn idle_ready_host_is_reclaimed() {
        let (mut supervisor, launcher) = supervisor();
        let now = Instant::now();
        start(&mut supervisor, now, "demo").expect("start");
        launcher
            .state
            .lock()
            .expect("fake host")
            .events
            .push_back(HostEvent::Ready);
        supervisor.poll_at(now, None);
        assert_eq!(supervisor.state("demo"), HostState::Ready);

        let events = supervisor.poll_at(now + IDLE_TIMEOUT, None);
        assert_eq!(supervisor.state("demo"), HostState::Stopped);
        assert!(events.iter().any(|event| matches!(
            event,
            SupervisorEvent::State {
                state: HostState::Stopped,
                detail: Some(detail),
                ..
            } if detail == "idle"
        )));
        let state = launcher.state.lock().expect("fake host");
        assert_eq!(state.drains, 1);
        assert!(state.stopped);
    }

    #[test]
    fn pending_request_blocks_idle_reclaim() {
        let (mut supervisor, launcher) = supervisor();
        let now = Instant::now();
        start(&mut supervisor, now, "demo").expect("start");
        launcher
            .state
            .lock()
            .expect("fake host")
            .events
            .push_back(HostEvent::Ready);
        supervisor.poll_at(now, None);
        supervisor.exec("demo").expect("exec");
        supervisor.poll_at(now + IDLE_TIMEOUT + Duration::from_secs(1), None);
        assert_eq!(supervisor.state("demo"), HostState::Ready);

        launcher.state.lock().expect("fake host").pending = 0;
        supervisor.poll_at(now + IDLE_TIMEOUT + Duration::from_secs(1), None);
        assert_eq!(supervisor.state("demo"), HostState::Stopped);
    }

    #[test]
    fn stop_drains_before_terminating() {
        let (mut supervisor, launcher) = supervisor();
        let now = Instant::now();
        start(&mut supervisor, now, "demo").expect("start");
        launcher
            .state
            .lock()
            .expect("fake host")
            .events
            .push_back(HostEvent::Ready);
        supervisor.poll_at(now, None);
        supervisor.stop("demo", None).expect("stop");
        assert_eq!(supervisor.state("demo"), HostState::Stopped);
        let state = launcher.state.lock().expect("fake host");
        assert_eq!(state.drains, 1);
        assert!(state.stopped);
    }
}

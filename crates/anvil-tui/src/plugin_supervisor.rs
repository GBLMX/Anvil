//! Single-flight plugin host supervision and bounded failure handling.

use crate::host_session::HostEvent;
use crate::plugin_session::PluginSession;
use anvil_ipc::GatewayClient;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) const START_TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) const STOP_DRAIN_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_ACTIVE_STARTS: usize = 8;
const MAX_PENDING_CALLS: usize = 32;
const MAX_FAILURES: usize = 3;
const FAILURE_WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostState {
    Unloaded,
    Registering,
    Starting,
    Ready,
    Draining,
    Stopped,
    Failed,
    Disabled,
}

impl HostState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Unloaded => "unloaded",
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
pub(crate) enum StartMode {
    Normal,
    Oom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExecOutcome {
    Sent,
    Queued,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SupervisorEvent {
    State {
        plugin_id: String,
        state: HostState,
        detail: Option<String>,
    },
    Message {
        plugin_id: String,
        message: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingCall {
    Exec,
}

struct StartRequest<'a> {
    plugin_root: Option<&'a Path>,
    mode: StartMode,
    permissions: &'a [String],
}

struct PluginEntry {
    state: HostState,
    host: Option<Box<dyn ManagedHost>>,
    started_at: Option<Instant>,
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
            retry_at: None,
            failures: Vec::new(),
            pending: VecDeque::new(),
        }
    }
}

pub(crate) trait ManagedHost {
    fn poll(&mut self) -> Vec<HostEvent>;
    fn exec(&mut self) -> bool;
    fn drain(&mut self, timeout: Duration);
    fn stop(&mut self);
}

impl ManagedHost for PluginSession {
    fn poll(&mut self) -> Vec<HostEvent> {
        PluginSession::poll(self)
    }

    fn exec(&mut self) -> bool {
        PluginSession::exec(self)
    }

    fn drain(&mut self, timeout: Duration) {
        PluginSession::drain(self, timeout);
    }

    fn stop(&mut self) {
        PluginSession::stop(self);
    }
}

pub(crate) trait HostLauncher {
    fn start(
        &self,
        gateway: Option<&mut GatewayClient>,
        workspace: &Path,
        plugin_id: &str,
        plugin_root: Option<&Path>,
        mode: StartMode,
        permissions: &[String],
    ) -> Result<Box<dyn ManagedHost>, String>;
}

struct SystemLauncher;

impl HostLauncher for SystemLauncher {
    fn start(
        &self,
        gateway: Option<&mut GatewayClient>,
        workspace: &Path,
        plugin_id: &str,
        plugin_root: Option<&Path>,
        mode: StartMode,
        permissions: &[String],
    ) -> Result<Box<dyn ManagedHost>, String> {
        let gateway = gateway.ok_or_else(|| "gateway unavailable".to_owned())?;
        let host = match (mode, plugin_root) {
            (StartMode::Oom, _) => {
                PluginSession::start_oom(gateway, workspace, plugin_id, permissions)
            }
            (StartMode::Normal, Some(root)) => {
                PluginSession::start_plugin(gateway, workspace, plugin_id, root, permissions)
            }
            (StartMode::Normal, None) => {
                PluginSession::start(gateway, workspace, plugin_id, permissions)
            }
        }?;
        Ok(Box::new(host))
    }
}

pub(crate) struct PluginSupervisor {
    launcher: Box<dyn HostLauncher>,
    entries: HashMap<String, PluginEntry>,
}

impl PluginSupervisor {
    pub(crate) fn new() -> Self {
        Self {
            launcher: Box::new(SystemLauncher),
            entries: HashMap::new(),
        }
    }

    #[cfg(test)]
    fn with_launcher(launcher: Box<dyn HostLauncher>) -> Self {
        Self {
            launcher,
            entries: HashMap::new(),
        }
    }

    pub(crate) fn activate(
        &mut self,
        plugin_id: &str,
        plugin_root: &Path,
        gateway: &mut GatewayClient,
        workspace: &Path,
    ) -> Result<(), String> {
        self.start_at(
            Instant::now(),
            plugin_id,
            StartRequest {
                plugin_root: Some(plugin_root),
                mode: StartMode::Normal,
                permissions: &[],
            },
            Some(gateway),
            workspace,
        )
    }

    pub(crate) fn start_debug(
        &mut self,
        plugin_id: &str,
        mode: StartMode,
        permissions: &[String],
        gateway: &mut GatewayClient,
        workspace: &Path,
    ) -> Result<(), String> {
        self.start_at(
            Instant::now(),
            plugin_id,
            StartRequest {
                plugin_root: None,
                mode,
                permissions,
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
    ) -> Result<(), String> {
        if let Some(entry) = self.entries.get(plugin_id) {
            match entry.state {
                HostState::Disabled => {
                    return Err(format!(
                        "plugin {plugin_id} disabled; run :plugin-enable {plugin_id}"
                    ));
                }
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
        if self
            .entries
            .values()
            .filter(|entry| matches!(entry.state, HostState::Registering | HostState::Starting))
            .count()
            >= MAX_ACTIVE_STARTS
        {
            return Err("plugin start queue full".to_owned());
        }
        let entry = self
            .entries
            .entry(plugin_id.to_owned())
            .or_insert_with(PluginEntry::new);
        entry.state = HostState::Registering;
        entry.started_at = None;
        entry.retry_at = None;
        entry.pending.clear();

        match self.launcher.start(
            gateway.as_deref_mut(),
            workspace,
            plugin_id,
            request.plugin_root,
            request.mode,
            request.permissions,
        ) {
            Ok(host) => {
                let entry = self
                    .entries
                    .get_mut(plugin_id)
                    .expect("plugin entry exists after registration");
                entry.host = Some(host);
                entry.state = HostState::Starting;
                entry.started_at = Some(now);
                Ok(())
            }
            Err(error) => {
                self.fail_at(plugin_id, now, error.clone(), gateway);
                Err(error)
            }
        }
    }

    pub(crate) fn exec(&mut self, plugin_id: &str) -> Result<ExecOutcome, String> {
        let entry = self
            .entries
            .get_mut(plugin_id)
            .ok_or_else(|| format!("plugin {plugin_id} is not loaded"))?;
        match entry.state {
            HostState::Ready => {
                if entry.host.as_mut().is_some_and(|host| host.exec()) {
                    Ok(ExecOutcome::Sent)
                } else {
                    Err(format!("plugin {plugin_id} host channel closed"))
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

    pub(crate) fn stop(
        &mut self,
        plugin_id: &str,
        gateway: Option<&mut GatewayClient>,
    ) -> Result<(), String> {
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
        entry.started_at = None;
        entry.retry_at = None;
        entry.pending.clear();
        Ok(())
    }

    pub(crate) fn enable(&mut self, plugin_id: &str) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(plugin_id)
            .ok_or_else(|| format!("plugin {plugin_id} is not loaded"))?;
        if entry.state != HostState::Disabled {
            return Err(format!("plugin {plugin_id} is not disabled"));
        }
        entry.state = HostState::Stopped;
        entry.failures.clear();
        entry.retry_at = None;
        Ok(())
    }

    pub(crate) fn state(&self, plugin_id: &str) -> HostState {
        self.entries
            .get(plugin_id)
            .map(|entry| entry.state)
            .unwrap_or(HostState::Unloaded)
    }

    pub(crate) fn poll(&mut self, gateway: Option<&mut GatewayClient>) -> Vec<SupervisorEvent> {
        self.poll_at(Instant::now(), gateway)
    }

    fn poll_at(
        &mut self,
        now: Instant,
        mut gateway: Option<&mut GatewayClient>,
    ) -> Vec<SupervisorEvent> {
        let ids = self.entries.keys().cloned().collect::<Vec<_>>();
        let mut events = Vec::new();
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
                        events.push(SupervisorEvent::State {
                            plugin_id: plugin_id.clone(),
                            state: HostState::Ready,
                            detail: None,
                        });
                        self.flush_pending(&plugin_id, now, gateway.as_deref_mut(), &mut events);
                    }
                    HostEvent::Pong => events.push(SupervisorEvent::Message {
                        plugin_id: plugin_id.clone(),
                        message: "ready".to_owned(),
                    }),
                    HostEvent::ExecUnsupported => events.push(SupervisorEvent::Message {
                        plugin_id: plugin_id.clone(),
                        message: "sys.exec unsupported".to_owned(),
                    }),
                    HostEvent::Error(error) => {
                        let detail = self.fail_at(&plugin_id, now, error, gateway.as_deref_mut());
                        events.push(SupervisorEvent::State {
                            plugin_id: plugin_id.clone(),
                            state: self.state(&plugin_id),
                            detail: Some(detail),
                        });
                        break;
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
            let sent = self
                .entries
                .get_mut(plugin_id)
                .and_then(|entry| entry.host.as_mut())
                .is_some_and(|host| match call {
                    PendingCall::Exec => host.exec(),
                });
            if sent {
                continue;
            }
            let detail = self.fail_at(plugin_id, now, "host channel closed".to_owned(), gateway);
            events.push(SupervisorEvent::State {
                plugin_id: plugin_id.to_owned(),
                state: self.state(plugin_id),
                detail: Some(detail),
            });
            return;
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
        entry.started_at = None;
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

    pub(crate) fn shutdown(&mut self, mut gateway: Option<&mut GatewayClient>) {
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct FakeHostState {
        events: VecDeque<HostEvent>,
        execs: usize,
        drains: usize,
        stopped: bool,
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

        fn exec(&mut self) -> bool {
            self.state.lock().expect("fake host").execs += 1;
            true
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
        fn start(
            &self,
            gateway: Option<&mut GatewayClient>,
            workspace: &Path,
            plugin_id: &str,
            plugin_root: Option<&Path>,
            mode: StartMode,
            permissions: &[String],
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
        fn start(
            &self,
            gateway: Option<&mut GatewayClient>,
            workspace: &Path,
            plugin_id: &str,
            plugin_root: Option<&Path>,
            mode: StartMode,
            permissions: &[String],
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
    ) -> Result<(), String> {
        supervisor.start_at(
            now,
            plugin_id,
            StartRequest {
                plugin_root: Some(Path::new(".")),
                mode: StartMode::Normal,
                permissions: &[],
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
        for index in 0..MAX_ACTIVE_STARTS {
            start(&mut supervisor, now, &format!("plugin-{index}")).expect("start");
        }
        start(&mut supervisor, now, "overflow").expect_err("start cap");
        assert_eq!(launcher.starts.load(Ordering::Relaxed), MAX_ACTIVE_STARTS);

        for _ in 0..MAX_PENDING_CALLS {
            assert_eq!(
                supervisor.exec("plugin-0").expect("queue"),
                ExecOutcome::Queued
            );
        }
        supervisor.exec("plugin-0").expect_err("request cap");
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

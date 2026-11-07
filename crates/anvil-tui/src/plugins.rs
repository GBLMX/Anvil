//! Plugin discovery, activation, and host lifecycle commands.

use super::*;
impl Model {
    #[cfg(target_os = "linux")]
    pub(crate) fn list_plugins(&mut self) {
        self.open_plugin_manager();
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn list_plugins(&mut self) {
        self.status = "plugins are Linux-only".to_owned();
        self.dirty = true;
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn activate_plugins(&mut self, event: ActivationEvent) {
        let candidates = self
            .plugins
            .iter()
            .filter(|plugin| {
                plugin.enabled
                    && plugin_activation::matches(&plugin.manifest.activation_events, &event)
            })
            .cloned()
            .collect::<Vec<_>>();
        let Some(gateway) = self.gateway.as_mut() else {
            return;
        };
        for plugin in candidates {
            let plugin_id = plugin.manifest.id.clone();
            if matches!(
                self.plugin_supervisor.state(&plugin_id),
                HostState::Queued
                    | HostState::Registering
                    | HostState::Starting
                    | HostState::Ready
                    | HostState::Draining
            ) {
                continue;
            }
            match self.plugin_supervisor.activate(
                &plugin_id,
                &plugin.root,
                gateway,
                &self.workspace,
            ) {
                Ok(StartOutcome::Started) => {
                    self.status = format!("plugin {plugin_id} starting");
                    self.dirty = true;
                }
                Ok(StartOutcome::Queued) => {
                    self.status = format!("plugin {plugin_id} queued");
                    self.dirty = true;
                }
                Err(error) => {
                    self.status = format!("E: plugin {plugin_id}: {error}");
                    self.dirty = true;
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn run_plugin_command(&mut self, plugin_id: &str, command_id: &str) {
        let Some(plugin) = self
            .plugins
            .iter()
            .find(|plugin| plugin.manifest.id == plugin_id)
            .cloned()
        else {
            self.status = format!("E: unknown plugin: {plugin_id}");
            self.dirty = true;
            return;
        };
        if !plugin.enabled {
            self.status = format!("E: plugin {plugin_id} disabled; run :plugin-enable {plugin_id}");
            self.dirty = true;
            return;
        }
        if !plugin
            .manifest
            .commands
            .iter()
            .any(|command| command.id == command_id)
        {
            self.status = format!("E: plugin {plugin_id} does not declare {command_id}");
            self.dirty = true;
            return;
        }
        if !plugin_activation::matches(
            &plugin.manifest.activation_events,
            &ActivationEvent::Command(command_id.to_owned()),
        ) {
            self.status = format!("E: plugin {plugin_id} does not activate on {command_id}");
            self.dirty = true;
            return;
        }
        let Some(gateway) = self.gateway.as_mut() else {
            self.status = "E: gateway unavailable".to_owned();
            self.dirty = true;
            return;
        };
        if matches!(
            self.plugin_supervisor.state(plugin_id),
            HostState::Queued
                | HostState::Registering
                | HostState::Starting
                | HostState::Ready
                | HostState::Draining
        ) && let Err(error) = self.plugin_supervisor.stop(plugin_id, Some(gateway))
        {
            self.status = format!("E: plugin {plugin_id}: {error}");
            self.dirty = true;
            return;
        }
        match self.plugin_supervisor.activate_command(
            plugin_id,
            &plugin.root,
            command_id,
            gateway,
            &self.workspace,
        ) {
            Ok(StartOutcome::Started) => {
                self.status = format!("plugin {plugin_id} running {command_id}")
            }
            Ok(StartOutcome::Queued) => self.status = format!("plugin {plugin_id} command queued"),
            Err(error) => self.status = format!("E: plugin {plugin_id}: {error}"),
        }
        self.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn run_plugin_command(&mut self, _plugin_id: &str, _command_id: &str) {
        self.status = "plugins are Linux-only".to_owned();
        self.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn activate_plugins(&mut self, _event: ActivationEvent) {}

    #[cfg(target_os = "linux")]
    pub(crate) fn activate_workspace_plugins(&mut self) {
        if !workspace_activation_enabled() {
            return;
        }
        let mut markers = Vec::new();
        for plugin in &self.plugins {
            for value in &plugin.manifest.activation_events {
                let Some(ActivationEvent::WorkspaceContains(marker)) =
                    plugin_activation::parse_event(value)
                else {
                    continue;
                };
                if !markers.contains(&marker) {
                    markers.push(marker);
                }
            }
        }
        if markers.is_empty() {
            return;
        }
        let Some(gateway) = self.gateway.as_mut() else {
            return;
        };
        let entries = match gateway.read_dir(".") {
            Ok(entries) => entries,
            Err(error) => {
                self.status = format!("E: workspace activation: {error}");
                self.dirty = true;
                return;
            }
        };
        let names = entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        for marker in markers {
            if names.contains(&marker.as_str()) {
                self.activate_plugins(ActivationEvent::WorkspaceContains(marker));
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn activate_workspace_plugins(&mut self) {}

    #[cfg(target_os = "linux")]
    pub(crate) fn activate_current_language(&mut self) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        let Some(settings) = lsp_settings(path) else {
            return;
        };
        self.activate_plugins(ActivationEvent::Language(settings.language_id.clone()));
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn activate_current_language(&mut self) {}

    #[cfg(target_os = "linux")]
    pub(crate) fn start_oom_host(&mut self) {
        let Some(gateway) = self.gateway.as_mut() else {
            self.status = "E: gateway unavailable".to_owned();
            self.dirty = true;
            return;
        };
        let permissions = DEBUG_PERMISSIONS
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect::<Vec<_>>();
        match self.plugin_supervisor.start_debug(
            DEBUG_PLUGIN_ID,
            StartMode::Oom,
            &permissions,
            gateway,
            &self.workspace,
        ) {
            Ok(StartOutcome::Started) => self.status = "plugin host OOM probe starting".to_owned(),
            Ok(StartOutcome::Queued) => self.status = "plugin host OOM probe queued".to_owned(),
            Err(error) => self.status = format!("E: {error}"),
        }
        self.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn start_oom_host(&mut self) {
        self.status = "E: plugin host is Linux-only".to_owned();
        self.dirty = true;
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn start_host(&mut self) {
        let Some(gateway) = self.gateway.as_mut() else {
            self.status = "E: gateway unavailable".to_owned();
            self.dirty = true;
            return;
        };
        let permissions = DEBUG_PERMISSIONS
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect::<Vec<_>>();
        match self.plugin_supervisor.start_debug(
            DEBUG_PLUGIN_ID,
            StartMode::Normal,
            &permissions,
            gateway,
            &self.workspace,
        ) {
            Ok(StartOutcome::Started) => self.status = "plugin host starting".to_owned(),
            Ok(StartOutcome::Queued) => self.status = "plugin host queued".to_owned(),
            Err(error) => self.status = format!("E: {error}"),
        }
        self.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn start_host(&mut self) {
        self.status = "E: plugin host is Linux-only".to_owned();
        self.dirty = true;
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn exec_host(&mut self) {
        match self.plugin_supervisor.exec(DEBUG_PLUGIN_ID) {
            Ok(ExecOutcome::Sent) => self.status = "plugin host request sent".to_owned(),
            Ok(ExecOutcome::Queued) => self.status = "plugin host request queued".to_owned(),
            Err(error) => self.status = format!("E: {error}"),
        }
        self.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn exec_host(&mut self) {}

    #[cfg(target_os = "linux")]
    pub(crate) fn stop_host(&mut self) {
        match self
            .plugin_supervisor
            .stop(DEBUG_PLUGIN_ID, self.gateway.as_mut())
        {
            Ok(()) => self.status = "plugin host stopped".to_owned(),
            Err(error) => self.status = format!("E: {error}"),
        }
        self.dirty = true;
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn set_managed_enabled(
        &mut self,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        {
            let gateway = self
                .gateway
                .as_mut()
                .ok_or_else(|| "gateway unavailable".to_owned())?;
            gateway
                .call(
                    "pkg.set_enabled",
                    serde_json::json!({"plugin_id": plugin_id, "enabled": enabled}),
                )
                .map_err(|error| error.to_string())?;
        }
        if let Some(plugin) = self
            .plugins
            .iter_mut()
            .find(|plugin| plugin.manifest.id == plugin_id)
        {
            plugin.enabled = enabled;
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn stop_host(&mut self) {}

    #[cfg(target_os = "linux")]
    pub(crate) fn enable_plugin(&mut self, plugin_id: &str) {
        let Some(plugin) = self
            .plugins
            .iter()
            .find(|plugin| plugin.manifest.id == plugin_id)
            .cloned()
        else {
            self.status = format!("E: unknown plugin: {plugin_id}");
            self.dirty = true;
            return;
        };
        if !plugin.enabled {
            if let Err(error) = self.set_managed_enabled(plugin_id, true) {
                self.status = format!("E: plugin {plugin_id}: {error}");
                self.dirty = true;
                return;
            }
        }
        let Some(gateway) = self.gateway.as_mut() else {
            self.status = "E: gateway unavailable".to_owned();
            self.dirty = true;
            return;
        };
        if let Err(error) = self.plugin_supervisor.enable(plugin_id) {
            self.status = format!("E: plugin {plugin_id}: {error}");
            self.dirty = true;
            return;
        }
        match self
            .plugin_supervisor
            .activate(plugin_id, &plugin.root, gateway, &self.workspace)
        {
            Ok(StartOutcome::Started) => self.status = format!("plugin {plugin_id} starting"),
            Ok(StartOutcome::Queued) => self.status = format!("plugin {plugin_id} queued"),
            Err(error) => self.status = format!("E: plugin {plugin_id}: {error}"),
        }
        self.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn enable_plugin(&mut self, _plugin_id: &str) {}

    #[cfg(target_os = "linux")]
    pub(crate) fn poll_host(&mut self) {
        let events = self.plugin_supervisor.poll(self.gateway.as_mut());
        for event in events {
            match event {
                SupervisorEvent::State {
                    plugin_id,
                    state,
                    detail,
                } => {
                    self.status = match state {
                        HostState::Starting => format!("plugin {plugin_id} starting"),
                        HostState::Ready => format!("plugin {plugin_id} ready"),
                        HostState::Failed => format!(
                            "E: plugin {plugin_id}: {}",
                            detail.unwrap_or_else(|| "start failed".to_owned())
                        ),
                        HostState::Disabled => format!(
                            "E: plugin {plugin_id} disabled ({}); run :plugin-enable {plugin_id}",
                            detail.unwrap_or_else(|| "repeated failures".to_owned())
                        ),
                        HostState::Stopped => match detail {
                            Some(detail) => format!("plugin {plugin_id} stopped ({detail})"),
                            None => format!("plugin {plugin_id} stopped"),
                        },
                        _ => format!("plugin {plugin_id} {}", state.as_str()),
                    };
                    self.dirty = true;
                }
                SupervisorEvent::Message { plugin_id, message } => {
                    self.status = format!("plugin {plugin_id}: {message}");
                    self.dirty = true;
                }
            }
        }
        self.poll_editor_requests();
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn poll_host(&mut self) {}
}

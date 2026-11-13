//! Plugin discovery, activation, and host lifecycle commands.

use super::*;
impl Model {
    #[cfg(target_os = "linux")]
    pub(crate) fn list_plugins(&mut self) {
        self.open_plugin_manager();
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn list_plugins(&mut self) {
        self.status.text = "plugins are Linux-only".to_owned();
        self.file.dirty = true;
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn activate_plugins(&mut self, event: ActivationEvent) {
        let candidates = self
            .plugins
            .entries
            .iter()
            .filter(|plugin| {
                plugin.enabled
                    && plugin_activation::matches(&plugin.manifest.activation_events, &event)
            })
            .cloned()
            .collect::<Vec<_>>();
        let Some(gateway) = self.workspace.gateway.as_mut() else {
            return;
        };
        for plugin in candidates {
            let plugin_id = plugin.manifest.id.clone();
            if matches!(
                self.plugins.supervisor.state(&plugin_id),
                HostState::Queued
                    | HostState::Registering
                    | HostState::Starting
                    | HostState::Ready
                    | HostState::Draining
            ) {
                continue;
            }
            match self.plugins.supervisor.activate(
                &plugin_id,
                &plugin.root,
                gateway,
                &self.workspace.path,
            ) {
                Ok(StartOutcome::Started) => {
                    self.status.text = format!("plugin {plugin_id} starting");
                    self.file.dirty = true;
                }
                Ok(StartOutcome::Queued) => {
                    self.status.text = format!("plugin {plugin_id} queued");
                    self.file.dirty = true;
                }
                Err(error) => {
                    self.status.text = format!("E: plugin {plugin_id}: {error}");
                    self.file.dirty = true;
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn run_plugin_command(&mut self, plugin_id: &str, command_id: &str) {
        let Some(plugin) = self
            .plugins
            .entries
            .iter()
            .find(|plugin| plugin.manifest.id == plugin_id)
            .cloned()
        else {
            self.status.text = format!("E: unknown plugin: {plugin_id}");
            self.file.dirty = true;
            return;
        };
        if !plugin.enabled {
            self.status.text =
                format!("E: plugin {plugin_id} disabled; run :plugin-enable {plugin_id}");
            self.file.dirty = true;
            return;
        }
        if !plugin
            .manifest
            .commands
            .iter()
            .any(|command| command.id == command_id)
        {
            self.status.text = format!("E: plugin {plugin_id} does not declare {command_id}");
            self.file.dirty = true;
            return;
        }
        if !plugin_activation::matches(
            &plugin.manifest.activation_events,
            &ActivationEvent::Command(command_id.to_owned()),
        ) {
            self.status.text = format!("E: plugin {plugin_id} does not activate on {command_id}");
            self.file.dirty = true;
            return;
        }
        let Some(gateway) = self.workspace.gateway.as_mut() else {
            self.status.text = "E: gateway unavailable".to_owned();
            self.file.dirty = true;
            return;
        };
        if matches!(
            self.plugins.supervisor.state(plugin_id),
            HostState::Queued
                | HostState::Registering
                | HostState::Starting
                | HostState::Ready
                | HostState::Draining
        ) && let Err(error) = self.plugins.supervisor.stop(plugin_id, Some(gateway))
        {
            self.status.text = format!("E: plugin {plugin_id}: {error}");
            self.file.dirty = true;
            return;
        }
        match self.plugins.supervisor.activate_command(
            plugin_id,
            &plugin.root,
            command_id,
            gateway,
            &self.workspace.path,
        ) {
            Ok(StartOutcome::Started) => {
                self.status.text = format!("plugin {plugin_id} running {command_id}")
            }
            Ok(StartOutcome::Queued) => {
                self.status.text = format!("plugin {plugin_id} command queued")
            }
            Err(error) => self.status.text = format!("E: plugin {plugin_id}: {error}"),
        }
        self.file.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn run_plugin_command(&mut self, _plugin_id: &str, _command_id: &str) {
        self.status.text = "plugins are Linux-only".to_owned();
        self.file.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn activate_plugins(&mut self, _event: ActivationEvent) {}

    #[cfg(target_os = "linux")]
    pub(crate) fn activate_workspace_plugins(&mut self) {
        if !workspace_activation_enabled() {
            return;
        }
        let mut markers = Vec::new();
        for plugin in &self.plugins.entries {
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
        let Some(gateway) = self.workspace.gateway.as_mut() else {
            return;
        };
        let entries = match gateway.read_dir(".") {
            Ok(entries) => entries,
            Err(error) => {
                self.status.text = format!("E: workspace activation: {error}");
                self.file.dirty = true;
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
        let Some(path) = self.session.path.as_ref() else {
            return;
        };
        let Some(settings) =
            lsp_settings_with_service(path, self.workspace.language_tooling_service.as_deref())
        else {
            return;
        };
        self.activate_plugins(ActivationEvent::Language(settings.language_id.clone()));
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn activate_current_language(&mut self) {}

    #[cfg(target_os = "linux")]
    pub(crate) fn start_oom_host(&mut self) {
        let Some(gateway) = self.workspace.gateway.as_mut() else {
            self.status.text = "E: gateway unavailable".to_owned();
            self.file.dirty = true;
            return;
        };
        let permissions = DEBUG_PERMISSIONS
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect::<Vec<_>>();
        match self.plugins.supervisor.start_debug(
            DEBUG_PLUGIN_ID,
            StartMode::Oom,
            &permissions,
            gateway,
            &self.workspace.path,
        ) {
            Ok(StartOutcome::Started) => {
                self.status.text = "plugin host OOM probe starting".to_owned()
            }
            Ok(StartOutcome::Queued) => {
                self.status.text = "plugin host OOM probe queued".to_owned()
            }
            Err(error) => self.status.text = format!("E: {error}"),
        }
        self.file.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn start_oom_host(&mut self) {
        self.status.text = "E: plugin host is Linux-only".to_owned();
        self.file.dirty = true;
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn start_host(&mut self) {
        let Some(gateway) = self.workspace.gateway.as_mut() else {
            self.status.text = "E: gateway unavailable".to_owned();
            self.file.dirty = true;
            return;
        };
        let permissions = DEBUG_PERMISSIONS
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect::<Vec<_>>();
        match self.plugins.supervisor.start_debug(
            DEBUG_PLUGIN_ID,
            StartMode::Normal,
            &permissions,
            gateway,
            &self.workspace.path,
        ) {
            Ok(StartOutcome::Started) => self.status.text = "plugin host starting".to_owned(),
            Ok(StartOutcome::Queued) => self.status.text = "plugin host queued".to_owned(),
            Err(error) => self.status.text = format!("E: {error}"),
        }
        self.file.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn start_host(&mut self) {
        self.status.text = "E: plugin host is Linux-only".to_owned();
        self.file.dirty = true;
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn exec_host(&mut self) {
        match self.plugins.supervisor.exec(DEBUG_PLUGIN_ID) {
            Ok(ExecOutcome::Sent) => self.status.text = "plugin host request sent".to_owned(),
            Ok(ExecOutcome::Queued) => self.status.text = "plugin host request queued".to_owned(),
            Err(error) => self.status.text = format!("E: {error}"),
        }
        self.file.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn exec_host(&mut self) {}

    #[cfg(target_os = "linux")]
    pub(crate) fn stop_host(&mut self) {
        match self
            .plugins
            .supervisor
            .stop(DEBUG_PLUGIN_ID, self.workspace.gateway.as_mut())
        {
            Ok(()) => self.status.text = "plugin host stopped".to_owned(),
            Err(error) => self.status.text = format!("E: {error}"),
        }
        self.file.dirty = true;
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn set_managed_enabled(
        &mut self,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        {
            let gateway = self
                .workspace
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
            .entries
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
            .entries
            .iter()
            .find(|plugin| plugin.manifest.id == plugin_id)
            .cloned()
        else {
            self.status.text = format!("E: unknown plugin: {plugin_id}");
            self.file.dirty = true;
            return;
        };
        if !plugin.enabled {
            if let Err(error) = self.set_managed_enabled(plugin_id, true) {
                self.status.text = format!("E: plugin {plugin_id}: {error}");
                self.file.dirty = true;
                return;
            }
        }
        let Some(gateway) = self.workspace.gateway.as_mut() else {
            self.status.text = "E: gateway unavailable".to_owned();
            self.file.dirty = true;
            return;
        };
        if let Err(error) = self.plugins.supervisor.enable(plugin_id) {
            self.status.text = format!("E: plugin {plugin_id}: {error}");
            self.file.dirty = true;
            return;
        }
        match self.plugins.supervisor.activate(
            plugin_id,
            &plugin.root,
            gateway,
            &self.workspace.path,
        ) {
            Ok(StartOutcome::Started) => self.status.text = format!("plugin {plugin_id} starting"),
            Ok(StartOutcome::Queued) => self.status.text = format!("plugin {plugin_id} queued"),
            Err(error) => self.status.text = format!("E: plugin {plugin_id}: {error}"),
        }
        self.file.dirty = true;
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn enable_plugin(&mut self, _plugin_id: &str) {}

    #[cfg(target_os = "linux")]
    pub(crate) fn poll_host(&mut self) {
        let events = self
            .plugins
            .supervisor
            .poll(self.workspace.gateway.as_mut());
        for event in events {
            match event {
                SupervisorEvent::State {
                    plugin_id,
                    state,
                    detail,
                } => {
                    self.status.text = match state {
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
                    self.file.dirty = true;
                }
                SupervisorEvent::Message { plugin_id, message } => {
                    self.status.text = format!("plugin {plugin_id}: {message}");
                    self.file.dirty = true;
                }
            }
        }
        self.poll_editor_requests();
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn poll_host(&mut self) {}
}

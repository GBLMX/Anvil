//! Installed-plugin manager overlay and lifecycle actions.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use anvil_session::plugin_supervisor::HostState;

use crate::{Model, plugin_discovery, theme};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PluginManagerState {
    pub(crate) selected: usize,
    pub(crate) confirm_uninstall: bool,
}

impl Model {
    pub(crate) fn open_plugin_manager(&mut self) {
        if self.plugins.entries.is_empty() {
            self.status.text = "no plugins (set ANVIL_PLUGIN_DIR or install a package)".to_owned();
            self.dirty = true;
            return;
        }
        self.plugins.manager = Some(PluginManagerState::default());
        self.dirty = true;
    }

    pub(crate) fn close_plugin_manager(&mut self) {
        self.plugins.manager = None;
        self.dirty = true;
    }

    pub(crate) fn move_plugin_manager(&mut self, delta: isize) {
        let Some(manager) = self.plugins.manager.as_mut() else {
            return;
        };
        let last = self.plugins.entries.len().saturating_sub(1);
        manager.selected = if delta < 0 {
            manager.selected.saturating_sub(delta.unsigned_abs())
        } else {
            (manager.selected + delta.unsigned_abs()).min(last)
        };
        manager.confirm_uninstall = false;
        self.dirty = true;
    }

    fn selected_plugin(&self) -> Option<plugin_discovery::PluginDescriptor> {
        let manager = self.plugins.manager.as_ref()?;
        self.plugins.entries.get(manager.selected).cloned()
    }

    pub(crate) fn toggle_selected_plugin(&mut self) {
        let Some(plugin) = self.selected_plugin() else {
            return;
        };
        if plugin.digest.is_none() {
            let kind = if plugin.source.as_deref() == Some("official") {
                "bundled official plugin; install a signed package to manage it"
            } else {
                "local plugin"
            };
            self.status.text = format!("E: plugin {} is a {kind}", plugin.manifest.id);
            self.dirty = true;
            return;
        }
        let plugin_id = plugin.manifest.id.clone();
        if plugin.enabled {
            let active = matches!(
                self.plugins.supervisor.state(&plugin_id),
                HostState::Queued
                    | HostState::Registering
                    | HostState::Starting
                    | HostState::Ready
                    | HostState::Draining
            );
            if active {
                let Some(gateway) = self.workspace.gateway.as_mut() else {
                    self.status.text = "E: gateway unavailable".to_owned();
                    self.dirty = true;
                    return;
                };
                if let Err(error) = self.plugins.supervisor.stop(&plugin_id, Some(gateway)) {
                    self.status.text = format!("E: stop plugin {plugin_id}: {error}");
                    self.dirty = true;
                    return;
                }
            }
            match self.set_managed_enabled(&plugin_id, false) {
                Ok(()) => self.status.text = format!("plugin {plugin_id} disabled"),
                Err(error) => self.status.text = format!("E: plugin {plugin_id}: {error}"),
            }
        } else {
            self.enable_plugin(&plugin_id);
        }
        self.dirty = true;
    }

    pub(crate) fn request_uninstall(&mut self) {
        let Some(manager) = self.plugins.manager.as_mut() else {
            return;
        };
        manager.confirm_uninstall = true;
        self.status.text = "uninstall plugin? [y/N]".to_owned();
        self.dirty = true;
    }

    pub(crate) fn cancel_uninstall(&mut self) {
        if let Some(manager) = self.plugins.manager.as_mut() {
            manager.confirm_uninstall = false;
        }
        self.status.text.clear();
        self.dirty = true;
    }

    pub(crate) fn uninstall_selected_plugin(&mut self) {
        let Some(manager) = self.plugins.manager.as_ref() else {
            return;
        };
        if !manager.confirm_uninstall {
            return;
        }
        let Some(plugin) = self.selected_plugin() else {
            return;
        };
        if plugin.digest.is_none() {
            self.status.text = "E: local plugins are not uninstalled here".to_owned();
            self.dirty = true;
            return;
        }
        let plugin_id = plugin.manifest.id.clone();
        if matches!(
            self.plugins.supervisor.state(&plugin_id),
            HostState::Queued
                | HostState::Registering
                | HostState::Starting
                | HostState::Ready
                | HostState::Draining
        ) {
            self.status.text = format!("E: disable plugin {plugin_id} before uninstall");
            self.dirty = true;
            return;
        }
        let result = self
            .workspace
            .gateway
            .as_mut()
            .ok_or_else(|| "gateway unavailable".to_owned())
            .and_then(|gateway| {
                gateway
                    .call("pkg.uninstall", serde_json::json!({"plugin_id": plugin_id}))
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok(()) => {
                self.status.text = format!("plugin {plugin_id} uninstalled");
                let _ = self.load_plugins();
                if self.plugins.entries.is_empty() {
                    self.plugins.manager = None;
                    self.dirty = true;
                    return;
                }
                if let Some(manager) = self.plugins.manager.as_mut() {
                    manager.selected = manager
                        .selected
                        .min(self.plugins.entries.len().saturating_sub(1));
                    manager.confirm_uninstall = false;
                }
            }
            Err(error) => self.status.text = format!("E: uninstall {plugin_id}: {error}"),
        }
        self.dirty = true;
    }

    pub(crate) fn rollback_selected_plugin(&mut self) {
        let Some(plugin) = self.selected_plugin() else {
            return;
        };
        if plugin.previous_digest.is_none() {
            self.status.text = "E: no previous version to roll back to".to_owned();
            self.dirty = true;
            return;
        }
        let plugin_id = plugin.manifest.id.clone();
        if matches!(
            self.plugins.supervisor.state(&plugin_id),
            HostState::Queued
                | HostState::Registering
                | HostState::Starting
                | HostState::Ready
                | HostState::Draining
        ) {
            let Some(gateway) = self.workspace.gateway.as_mut() else {
                self.status.text = "E: gateway unavailable".to_owned();
                self.dirty = true;
                return;
            };
            if let Err(error) = self.plugins.supervisor.stop(&plugin_id, Some(gateway)) {
                self.status.text = format!("E: stop plugin {plugin_id}: {error}");
                self.dirty = true;
                return;
            }
        }
        if let Err(error) = self.set_managed_enabled(&plugin_id, false) {
            self.status.text = format!("E: plugin {plugin_id}: {error}");
            self.dirty = true;
            return;
        }
        let result = self
            .workspace
            .gateway
            .as_mut()
            .ok_or_else(|| "gateway unavailable".to_owned())
            .and_then(|gateway| {
                gateway
                    .call("pkg.rollback", serde_json::json!({"plugin_id": plugin_id}))
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok(()) => {
                self.status.text = format!("plugin {plugin_id} rolled back; enable to start");
                let _ = self.load_plugins();
            }
            Err(error) => self.status.text = format!("E: rollback {plugin_id}: {error}"),
        }
        self.dirty = true;
    }
}

pub(crate) fn render(model: &Model, frame: &mut Frame, area: Rect) {
    let Some(manager) = &model.plugins.manager else {
        return;
    };
    let width = (area.width * 9 / 10)
        .max(60)
        .min(area.width.saturating_sub(2));
    let height = (model.plugins.entries.len() as u16 + 7)
        .max(10)
        .min(area.height.saturating_sub(2));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);
    let mut lines = vec![Line::from(vec![Span::styled(
        "Installed plugins",
        Style::default()
            .fg(theme::accent())
            .add_modifier(Modifier::BOLD),
    )])];
    let safe = |value: &str| value.escape_default().to_string();
    for (index, plugin) in model.plugins.entries.iter().enumerate() {
        let marker = if index == manager.selected { ">" } else { " " };
        let source = safe(plugin.source.as_deref().unwrap_or("Local"));
        let digest = plugin
            .digest
            .as_deref()
            .map(|value| safe(&value.chars().take(8).collect::<String>()))
            .unwrap_or_else(|| "-".to_owned());
        let state = if plugin.enabled {
            "Enabled"
        } else {
            "Disabled"
        };
        let line = format!(
            "{marker} {:<28} {:>8} {} {:<12} {:<8}",
            safe(&plugin.manifest.id),
            safe(&plugin.manifest.version),
            digest,
            source,
            state
        );
        lines.push(Line::from(if index == manager.selected {
            Span::styled(line, Style::default().fg(theme::accent()))
        } else {
            Span::raw(line)
        }));
    }
    lines.push(Line::raw(""));
    if manager.confirm_uninstall {
        lines.push(Line::from("Uninstall selected plugin? [y/N]"));
    } else {
        lines.push(Line::from(
            "[Space] enable/disable  [x] uninstall  [r] rollback  [Esc] close",
        ));
    }
    lines.push(Line::from(format!(
        "Store: {}",
        format_bytes(model.plugins.store_size)
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::panel())
            .block(crate::render::panel_block("Plugin Manager")),
        popup,
    );
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_plugin_host::manifest::PluginManifest;
    use ratatui::{Terminal, backend::TestBackend};

    fn model() -> Model {
        let mut model = Model::new(crate::EditorState::from_text(""), None);
        model
            .plugins
            .entries
            .push(plugin_discovery::PluginDescriptor {
                root: std::path::PathBuf::from("/store/root"),
                manifest: PluginManifest {
                    id: "demo.plugin".to_owned(),
                    version: "1.2.3".to_owned(),
                    entry: "main.luau".to_owned(),
                    api: "1.0".to_owned(),
                    activation_events: Vec::new(),
                    commands: Vec::new(),
                },
                enabled: true,
                digest: Some("0123456789abcdef".to_owned()),
                source: Some("local-file".to_owned()),
                previous_digest: None,
            });
        model.plugins.store_size = 2 * 1024 * 1024;
        model.plugins.manager = Some(PluginManagerState::default());
        model
    }

    #[test]
    fn manager_renders_identity_state_and_store_size() {
        let model = model();
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render(&model, frame, frame.area()))
            .expect("draw");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("demo.plugin"), "{rendered}");
        assert!(rendered.contains("1.2.3"), "{rendered}");
        assert!(rendered.contains("Enabled"), "{rendered}");
        assert!(rendered.contains("2.0 MiB"), "{rendered}");
    }

    #[test]
    fn uninstall_requires_confirmation() {
        let mut model = model();
        model.request_uninstall();
        assert!(model.plugins.manager.expect("manager").confirm_uninstall);
        model.cancel_uninstall();
        assert!(!model.plugins.manager.expect("manager").confirm_uninstall);
    }

    #[test]
    fn manager_escapes_control_sequences() {
        let mut model = model();
        model.plugins.entries[0].manifest.version = "1.2.3\n\x1b[31m".to_owned();
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render(&model, frame, frame.area()))
            .expect("draw");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(!rendered.contains('\x1b'), "{rendered}");
    }
}

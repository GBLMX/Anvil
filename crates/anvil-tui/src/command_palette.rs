//! Command palette state built from the shared command registry.

use super::*;
use anvil_session::command::{self, CommandContext};

fn command_title(id: &str) -> String {
    id.split(['.', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => {
                    let mut title = first.to_uppercase().collect::<String>();
                    title.push_str(characters.as_str());
                    title
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Model {
    pub(crate) fn open_command_palette(&mut self) {
        let mut items = command::COMMANDS
            .iter()
            .filter(|spec| crate::keymap::executable_command(spec.id.as_str()))
            .map(|spec| {
                let mut contexts = Vec::new();
                if spec.contexts.contains(&CommandContext::Normal) {
                    contexts.push("normal");
                }
                if spec.contexts.contains(&CommandContext::Pending) {
                    contexts.push("pending");
                }
                let id = spec.id.as_str();
                let title = command_title(id);
                let detail = format!("{id} · {}", contexts.join("+"));
                CommandPaletteItem {
                    id: id.to_owned(),
                    label: format!("{title} {id}"),
                    title,
                    detail,
                    plugin_id: None,
                    theme: None,
                }
            })
            .collect::<Vec<_>>();
        for name in theme::names() {
            let current = if name == theme::name() {
                " [current]"
            } else {
                ""
            };
            items.push(CommandPaletteItem {
                id: format!("theme.{name}"),
                label: format!("Theme {name}{current}"),
                title: format!("Theme: {name}"),
                detail: current.trim().to_owned(),
                plugin_id: None,
                theme: Some(name),
            });
        }
        #[cfg(target_os = "linux")]
        for plugin in self.plugins.entries.iter().filter(|plugin| plugin.enabled) {
            for command in &plugin.manifest.commands {
                if items.len() >= 512 {
                    break;
                }
                if items.iter().any(|item| item.id == command.id) {
                    continue;
                }
                items.push(CommandPaletteItem {
                    id: command.id.clone(),
                    label: format!(
                        "{} {}",
                        command.title.escape_default(),
                        command.id.escape_default()
                    ),
                    title: command.title.escape_default().to_string(),
                    detail: format!(
                        "{} · plugin {}",
                        command.id.escape_default(),
                        plugin.manifest.id.escape_default()
                    ),
                    plugin_id: Some(plugin.manifest.id.clone()),
                    theme: None,
                });
            }
        }
        self.overlays.command_palette = Some(CommandPaletteState {
            table: SelectionTable::new(items),
            theme_picker: theme_picker::ThemePicker::new(),
        });
        self.refresh_command_palette();
        self.input.pending = None;
        self.input.leader_deadline = None;
        self.file.dirty = true;
    }

    pub(crate) fn close_command_palette(&mut self) {
        if let Some(palette) = self.overlays.command_palette.take() {
            palette.theme_picker.cancel();
        }
        self.file.dirty = true;
    }

    pub(crate) fn commit_theme(&mut self, name: &str) {
        if let Some(palette) = self.overlays.command_palette.take() {
            self.status.text = match palette.theme_picker.commit(name) {
                Ok(()) => {
                    let status = format!("theme: {}", theme::name());
                    self.persist_theme_status(status)
                }
                Err(error) => format!("E: {error}"),
            };
        }
        self.file.dirty = true;
    }

    pub(crate) fn refresh_command_palette(&mut self) {
        let recents = self.overlays.command_palette_recents.clone();
        let Some(palette) = self.overlays.command_palette.as_mut() else {
            return;
        };
        let empty_query = palette.table.query().is_empty();
        palette.table.refresh();
        if empty_query {
            palette.table.sort_filtered_by(|item| {
                recents
                    .iter()
                    .position(|id| id == &item.id)
                    .unwrap_or(usize::MAX)
            });
        }
        self.preview_selected_theme();
        self.file.dirty = true;
    }

    pub(crate) fn move_command_palette(&mut self, delta: isize) {
        let Some(palette) = self.overlays.command_palette.as_mut() else {
            return;
        };
        palette.table.move_by(delta);
        self.preview_selected_theme();
        self.file.dirty = true;
    }

    pub(crate) fn selected_command(&self) -> Option<(String, Option<String>)> {
        let palette = self.overlays.command_palette.as_ref()?;
        palette
            .table
            .selected()
            .filter(|item| item.theme.is_none())
            .map(|item| (item.id.clone(), item.plugin_id.clone()))
    }

    pub(crate) fn selected_theme(&self) -> Option<&'static str> {
        self.overlays
            .command_palette
            .as_ref()?
            .table
            .selected()
            .and_then(|item| item.theme)
    }

    fn preview_selected_theme(&mut self) {
        let selected = self.selected_theme();
        if let Some(palette) = self.overlays.command_palette.as_ref() {
            palette.theme_picker.preview(selected);
        }
        self.file.dirty = true;
    }

    pub(crate) fn remember_command(&mut self, id: &str) {
        self.overlays
            .command_palette_recents
            .retain(|recent| recent != id);
        self.overlays
            .command_palette_recents
            .insert(0, id.to_owned());
        self.overlays.command_palette_recents.truncate(10);
    }
}

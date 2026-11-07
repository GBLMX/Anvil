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
                CommandPaletteItem {
                    id: id.to_owned(),
                    label: format!(
                        "{:<28} {:<36} [{}]",
                        command_title(id),
                        id,
                        contexts.join("+")
                    ),
                    plugin_id: None,
                }
            })
            .collect::<Vec<_>>();
        #[cfg(target_os = "linux")]
        for plugin in self.plugins.iter().filter(|plugin| plugin.enabled) {
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
                        "{:<28} {:<36} [plugin {}]",
                        command.title.escape_default(),
                        command.id.escape_default(),
                        plugin.manifest.id.escape_default()
                    ),
                    plugin_id: Some(plugin.manifest.id.clone()),
                });
            }
        }
        let filtered = (0..items.len()).collect();
        self.command_palette = Some(CommandPaletteState {
            query: String::new(),
            items,
            filtered,
            selected: 0,
        });
        self.refresh_command_palette();
        self.pending = None;
        self.leader_deadline = None;
        self.dirty = true;
    }

    pub(crate) fn close_command_palette(&mut self) {
        self.command_palette = None;
        self.dirty = true;
    }

    pub(crate) fn refresh_command_palette(&mut self) {
        let recents = self.command_palette_recents.clone();
        let Some(palette) = self.command_palette.as_mut() else {
            return;
        };
        let query = palette.query.to_ascii_lowercase();
        palette.filtered = palette
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                query.is_empty() || item.label.to_ascii_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect();
        if query.is_empty() {
            palette.filtered.sort_by_key(|index| {
                recents
                    .iter()
                    .position(|id| id == &palette.items[*index].id)
                    .unwrap_or(usize::MAX)
            });
        }
        palette.selected = palette
            .selected
            .min(palette.filtered.len().saturating_sub(1));
        self.dirty = true;
    }

    pub(crate) fn move_command_palette(&mut self, delta: isize) {
        let Some(palette) = self.command_palette.as_mut() else {
            return;
        };
        if palette.filtered.is_empty() {
            return;
        }
        let last = palette.filtered.len() - 1;
        palette.selected = if delta < 0 {
            palette.selected.saturating_sub(delta.unsigned_abs())
        } else {
            (palette.selected + delta.unsigned_abs()).min(last)
        };
        self.dirty = true;
    }

    pub(crate) fn selected_command(&self) -> Option<(String, Option<String>)> {
        let palette = self.command_palette.as_ref()?;
        palette
            .items
            .get(*palette.filtered.get(palette.selected)?)
            .map(|item| (item.id.clone(), item.plugin_id.clone()))
    }

    pub(crate) fn remember_command(&mut self, id: &str) {
        self.command_palette_recents.retain(|recent| recent != id);
        self.command_palette_recents.insert(0, id.to_owned());
        self.command_palette_recents.truncate(10);
    }
}

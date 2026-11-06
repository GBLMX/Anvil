//! Command palette state built from the shared command registry.

use super::*;
use anvil_session::command::{self, CommandContext};

impl Model {
    pub(crate) fn open_command_palette(&mut self) {
        let items = command::COMMANDS
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
                format!("{:<36} [{}]", spec.id.as_str(), contexts.join("+"))
            })
            .collect::<Vec<_>>();
        let filtered = (0..items.len()).collect();
        self.command_palette = Some(CommandPaletteState {
            query: String::new(),
            items,
            filtered,
            selected: 0,
        });
        self.pending = None;
        self.leader_deadline = None;
        self.dirty = true;
    }

    pub(crate) fn close_command_palette(&mut self) {
        self.command_palette = None;
        self.dirty = true;
    }

    pub(crate) fn refresh_command_palette(&mut self) {
        let Some(palette) = self.command_palette.as_mut() else {
            return;
        };
        let query = palette.query.to_ascii_lowercase();
        palette.filtered = palette
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| query.is_empty() || item.to_ascii_lowercase().contains(&query))
            .map(|(index, _)| index)
            .collect();
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

    pub(crate) fn selected_command_id(&self) -> Option<String> {
        let palette = self.command_palette.as_ref()?;
        let item = palette
            .items
            .get(*palette.filtered.get(palette.selected)?)?;
        item.split_whitespace().next().map(str::to_owned)
    }
}

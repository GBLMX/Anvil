//! Command palette state built from the shared command registry.

use super::*;
use anvil_session::command::{self, CommandContext};

const RECENTS_LIMIT: usize = 10;

pub(crate) fn command_title(id: &str) -> String {
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

impl CommandPaletteState {
    /// Selected non-theme row as the command id and owning plugin.
    pub(crate) fn selected_command(&self) -> Option<(String, Option<String>)> {
        self.table
            .selected()
            .filter(|item| item.theme.is_none())
            .map(|item| (item.id.clone(), item.plugin_id.clone()))
    }

    /// Theme carried by the selected row, if it is a theme entry.
    pub(crate) fn selected_theme(&self) -> Option<&'static str> {
        self.table.selected().and_then(|item| item.theme)
    }

    /// Select a visible palette row without executing it.
    pub(crate) fn select(&mut self, index: usize) {
        self.table.select(index);
    }

    /// Move the selected row `delta` places, clamped to the filtered view.
    pub(crate) fn move_selection(&mut self, delta: isize) {
        self.table.move_by(delta);
    }

    /// Recompute the filtered rows; an empty query keeps recents first.
    pub(crate) fn refresh(&mut self, recents: &[String]) {
        let empty_query = self.table.query().is_empty();
        self.table.refresh();
        if empty_query {
            self.table.sort_filtered_by(|item| {
                recents
                    .iter()
                    .position(|id| id == &item.id)
                    .unwrap_or(usize::MAX)
            });
        }
    }

    /// Record `id` as the most recent command, deduplicated and capped.
    pub(crate) fn remember_command(recents: &mut Vec<String>, id: &str) {
        recents.retain(|recent| recent != id);
        recents.insert(0, id.to_owned());
        recents.truncate(RECENTS_LIMIT);
    }
}

impl Model {
    pub(crate) fn open_command_palette(&mut self) {
        self.open_command_palette_with(CommandPaletteMode::All);
    }

    pub(crate) fn open_command_history(&mut self) {
        if !self.workspace.recent.commands.is_empty() {
            self.open_ex_command_history();
            return;
        }
        self.open_command_palette_with(CommandPaletteMode::History);
    }

    fn open_ex_command_history(&mut self) {
        let items = self
            .workspace
            .recent
            .commands
            .iter()
            .map(|command| CommandPaletteItem {
                id: format!("ex:{command}"),
                label: format!("ex {command}"),
                title: command.clone(),
                detail: "Ex command".to_owned(),
                group: "ex".to_owned(),
                key: None,
                recent: true,
                plugin_id: None,
                theme: None,
            })
            .collect();
        self.overlays.command_palette = Some(CommandPaletteState {
            table: SelectionTable::new(items),
            theme_picker: theme_picker::ThemePicker::new(),
            mode: CommandPaletteMode::History,
        });
        self.push_overlay(OverlayKind::CommandPalette);
        self.refresh_command_palette();
        self.input.pending = None;
        self.input.leader_deadline = None;
        self.file.dirty = true;
    }

    pub(crate) fn open_keybinds(&mut self) {
        self.open_command_palette_with(CommandPaletteMode::Keybinds);
    }

    fn open_command_palette_with(&mut self, mode: CommandPaletteMode) {
        let recents = self.overlays.command_palette_recents.clone();
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
                let group = id.split('.').next().unwrap_or("other").to_owned();
                CommandPaletteItem {
                    id: id.to_owned(),
                    label: format!("{group} {title} {id}"),
                    title,
                    detail,
                    group,
                    key: self.config.keymap.key_for_command(id),
                    recent: recents.iter().any(|recent| recent == id),
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
                label: format!("theme Theme {name}{current}"),
                title: format!("Theme: {name}"),
                detail: current.trim().to_owned(),
                group: "theme".to_owned(),
                key: None,
                recent: recents
                    .iter()
                    .any(|recent| recent == &format!("theme.{name}")),
                plugin_id: None,
                theme: Some(name),
            });
        }
        #[cfg(target_os = "linux")]
        for plugin in self
            .runtime
            .plugins
            .entries
            .iter()
            .filter(|plugin| plugin.enabled)
        {
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
                    group: "plugin".to_owned(),
                    key: None,
                    recent: false,
                    plugin_id: Some(plugin.manifest.id.clone()),
                    theme: None,
                });
            }
        }
        #[cfg(target_os = "linux")]
        for (command_id, command) in &self.runtime.plugins.dynamic_commands {
            if items.len() >= 512 {
                break;
            }
            if items.iter().any(|item| item.id == *command_id) {
                continue;
            }
            if !self
                .runtime
                .plugins
                .entries
                .iter()
                .any(|plugin| plugin.enabled && plugin.manifest.id == command.plugin_id)
            {
                continue;
            }
            items.push(CommandPaletteItem {
                id: command_id.clone(),
                label: format!(
                    "{} {}",
                    command.title.escape_default(),
                    command_id.escape_default()
                ),
                title: command.title.escape_default().to_string(),
                detail: format!(
                    "{} · plugin {}",
                    command_id.escape_default(),
                    command.plugin_id.escape_default()
                ),
                group: "plugin".to_owned(),
                key: None,
                recent: false,
                plugin_id: Some(command.plugin_id.clone()),
                theme: None,
            });
        }
        match mode {
            CommandPaletteMode::All => {}
            CommandPaletteMode::History => items.retain(|item| item.recent),
            CommandPaletteMode::Keybinds => items.retain(|item| item.key.is_some()),
        }
        if items.is_empty() {
            self.status.text = match mode {
                CommandPaletteMode::History => "no command history",
                CommandPaletteMode::Keybinds => "no keymapped commands",
                CommandPaletteMode::All => "no commands",
            }
            .to_owned();
            self.file.dirty = true;
            return;
        }
        self.overlays.command_palette = Some(CommandPaletteState {
            table: SelectionTable::new(items),
            theme_picker: theme_picker::ThemePicker::new(),
            mode,
        });
        self.push_overlay(OverlayKind::CommandPalette);
        self.refresh_command_palette();
        self.input.pending = None;
        self.input.leader_deadline = None;
        self.file.dirty = true;
    }

    pub(crate) fn close_command_palette(&mut self) {
        if let Some(palette) = self.overlays.command_palette.take() {
            palette.theme_picker.cancel();
        }
        self.close_overlay(OverlayKind::CommandPalette);
        self.file.dirty = true;
    }

    pub(crate) fn commit_theme(&mut self, name: &str) {
        self.close_overlay(OverlayKind::CommandPalette);
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
        palette.refresh(&recents);
        self.preview_selected_theme();
        self.file.dirty = true;
    }

    pub(crate) fn move_command_palette(&mut self, delta: isize) {
        let Some(palette) = self.overlays.command_palette.as_mut() else {
            return;
        };
        palette.move_selection(delta);
        self.preview_selected_theme();
        self.file.dirty = true;
    }

    pub(crate) fn selected_command(&self) -> Option<(String, Option<String>)> {
        self.overlays.command_palette.as_ref()?.selected_command()
    }

    pub(crate) fn selected_theme(&self) -> Option<&'static str> {
        self.overlays.command_palette.as_ref()?.selected_theme()
    }

    fn preview_selected_theme(&mut self) {
        let selected = self.selected_theme();
        if let Some(palette) = self.overlays.command_palette.as_ref() {
            palette.theme_picker.preview(selected);
        }
        self.file.dirty = true;
    }

    pub(crate) fn remember_command(&mut self, id: &str) {
        CommandPaletteState::remember_command(&mut self.overlays.command_palette_recents, id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, theme: Option<&'static str>) -> CommandPaletteItem {
        CommandPaletteItem {
            id: id.to_owned(),
            label: id.to_owned(),
            title: id.to_owned(),
            detail: String::new(),
            group: "test".to_owned(),
            key: None,
            recent: false,
            plugin_id: None,
            theme,
        }
    }

    fn command(id: &str) -> CommandPaletteItem {
        item(id, None)
    }

    fn palette(items: Vec<CommandPaletteItem>) -> CommandPaletteState {
        CommandPaletteState {
            table: SelectionTable::new(items),
            theme_picker: theme_picker::ThemePicker::new(),
            mode: CommandPaletteMode::All,
        }
    }

    #[test]
    fn selected_access_separates_commands_and_themes() {
        let mut palette = palette(vec![
            item("theme.nord", Some("nord")),
            command("editor.undo"),
        ]);
        assert_eq!(palette.selected_theme(), Some("nord"));
        assert_eq!(palette.selected_command(), None);

        palette.move_selection(1);
        assert_eq!(palette.selected_theme(), None);
        assert_eq!(
            palette.selected_command(),
            Some(("editor.undo".to_owned(), None))
        );
    }

    #[test]
    fn selected_command_carries_the_plugin_id() {
        let mut plugin = command("demo.hello");
        plugin.plugin_id = Some("demo".to_owned());
        let palette = palette(vec![plugin]);
        assert_eq!(
            palette.selected_command(),
            Some(("demo.hello".to_owned(), Some("demo".to_owned())))
        );
    }

    #[test]
    fn move_selection_clamps_to_the_filtered_view() {
        let mut palette = palette(vec![
            command("editor.a"),
            command("editor.b"),
            command("editor.c"),
        ]);
        palette.move_selection(9);
        assert_eq!(
            palette.selected_command().map(|(id, _)| id),
            Some("editor.c".to_owned())
        );
        palette.move_selection(-9);
        assert_eq!(
            palette.selected_command().map(|(id, _)| id),
            Some("editor.a".to_owned())
        );
    }

    #[test]
    fn move_selection_ignores_an_empty_filtered_view() {
        let mut palette = palette(vec![command("editor.undo")]);
        palette.table.query_mut().push_str("missing");
        palette.refresh(&[]);
        assert!(palette.table.is_empty());

        palette.move_selection(1);
        assert_eq!(palette.selected_command(), None);
        assert_eq!(palette.selected_theme(), None);
    }

    #[test]
    fn refresh_orders_recents_first_for_an_empty_query() {
        let mut palette = palette(vec![
            command("editor.undo"),
            command("editor.redo"),
            command("editor.save"),
        ]);
        palette.refresh(&[
            "editor.save".to_owned(),
            "editor.undo".to_owned(),
            "editor.missing".to_owned(),
        ]);
        assert_eq!(palette.table.filtered(), &[2, 0, 1]);
        assert_eq!(
            palette.selected_command().map(|(id, _)| id),
            Some("editor.save".to_owned())
        );
    }

    #[test]
    fn refresh_keeps_source_order_for_a_non_empty_query() {
        let mut palette = palette(vec![
            command("editor.undo"),
            command("editor.redo"),
            command("editor.save"),
        ]);
        palette.table.query_mut().push_str("editor");
        palette.refresh(&["editor.save".to_owned()]);
        assert_eq!(palette.table.filtered(), &[0, 1, 2]);
    }

    #[test]
    fn empty_query_refresh_without_recents_keeps_source_order() {
        let mut palette = palette(vec![command("editor.b"), command("editor.a")]);
        palette.refresh(&[]);
        assert_eq!(palette.table.filtered(), &[0, 1]);
        assert_eq!(
            palette.selected_command().map(|(id, _)| id),
            Some("editor.b".to_owned())
        );
    }

    #[test]
    fn remember_command_dedupes_to_the_front() {
        let mut recents = Vec::new();
        for id in ["editor.a", "editor.b", "editor.c"] {
            CommandPaletteState::remember_command(&mut recents, id);
        }
        assert_eq!(recents, ["editor.c", "editor.b", "editor.a"]);

        CommandPaletteState::remember_command(&mut recents, "editor.a");
        assert_eq!(recents, ["editor.a", "editor.c", "editor.b"]);
    }

    #[test]
    fn remember_command_caps_the_recency_list() {
        let mut recents = Vec::new();
        for index in 0..RECENTS_LIMIT + 5 {
            CommandPaletteState::remember_command(&mut recents, &format!("command.{index}"));
        }
        assert_eq!(recents.len(), RECENTS_LIMIT);
        assert_eq!(recents.first().map(String::as_str), Some("command.14"));
        assert_eq!(recents.last().map(String::as_str), Some("command.5"));
    }

    #[test]
    fn history_and_keybind_modes_filter_the_palette() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.overlays.command_palette_recents = vec!["editor.undo".to_owned()];

        model.open_command_history();
        let palette = model.overlays.command_palette.as_ref().expect("history");
        assert_eq!(palette.mode, CommandPaletteMode::History);
        assert!(palette.table.items().iter().all(|item| item.recent));
        assert!(
            palette
                .table
                .items()
                .iter()
                .any(|item| item.id == "editor.undo")
        );

        model.close_command_palette();
        model.open_keybinds();
        let palette = model.overlays.command_palette.as_ref().expect("keybinds");
        assert_eq!(palette.mode, CommandPaletteMode::Keybinds);
        assert!(palette.table.items().iter().all(|item| item.key.is_some()));
    }

    #[test]
    fn ex_command_history_prefers_persisted_commands() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.record_ex_command("w");
        model.record_ex_command("s/foo/bar/g");

        model.open_command_history();

        let palette = model.overlays.command_palette.as_ref().expect("history");
        assert_eq!(palette.mode, CommandPaletteMode::History);
        assert_eq!(
            palette
                .table
                .items()
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["ex:s/foo/bar/g", "ex:w"]
        );
    }
}

//! Ex command-name and argument completion.

use super::*;
use crate::selection_table::{SelectionItem, SelectionTable};

#[derive(Clone, Debug)]
pub(crate) struct CompletionItem {
    pub(crate) label: String,
    insert: String,
}

impl SelectionItem for CompletionItem {
    fn label(&self) -> &str {
        &self.label
    }
}

pub(crate) struct CommandCompletion {
    table: SelectionTable<CompletionItem>,
}

impl CommandCompletion {
    fn new(buffer: &str, plugin_ids: &[&str]) -> Self {
        let buffer = buffer.trim_start();
        let (command, argument, has_argument) = match buffer.split_once(char::is_whitespace) {
            Some((command, argument)) => (command, argument.trim_start(), true),
            None => (buffer, "", false),
        };
        let (query, items) = if has_argument {
            match command {
                "theme" => argument_items(command, argument, theme::names().into_iter(), 1)
                    .unwrap_or_default(),
                "theme-auto" => argument_items(command, argument, theme::names().into_iter(), 2)
                    .unwrap_or_default(),
                "plugin-enable" => argument_items(command, argument, plugin_ids.iter().copied(), 1)
                    .unwrap_or_default(),
                _ => (String::new(), Vec::new()),
            }
        } else {
            (
                buffer.to_owned(),
                anvil_control::COMMANDS
                    .iter()
                    .map(|command| CompletionItem {
                        label: command.name.to_owned(),
                        insert: format!("{} ", command.name),
                    })
                    .collect(),
            )
        };
        let mut table = SelectionTable::new(items);
        table.query_mut().push_str(&query);
        table.refresh_with(|item, query| {
            query.is_empty() || item.label.to_ascii_lowercase().starts_with(query)
        });
        Self { table }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    pub(crate) fn selected(&self) -> Option<&str> {
        self.table.selected().map(|item| item.insert.as_str())
    }

    pub(crate) fn move_by(&mut self, delta: isize) {
        self.table.move_by(delta);
    }

    pub(crate) fn items(&self) -> &[CompletionItem] {
        self.table.items()
    }

    pub(crate) fn filtered(&self) -> &[usize] {
        self.table.filtered()
    }

    pub(crate) fn selected_index(&self) -> usize {
        self.table.selected_index()
    }
}

fn argument_items(
    command: &str,
    argument: &str,
    names: impl Iterator<Item = impl AsRef<str>>,
    max_arguments: usize,
) -> Option<(String, Vec<CompletionItem>)> {
    let (before, query) = if argument.ends_with(char::is_whitespace) {
        (argument, "")
    } else if let Some((before, query)) = argument.rsplit_once(char::is_whitespace) {
        (before, query)
    } else {
        ("", argument)
    };
    if before.split_whitespace().count() >= max_arguments {
        return None;
    }
    let prefix = if before.is_empty() {
        format!("{command} ")
    } else {
        format!("{command} {before} ")
    };
    let items = names
        .map(|name| {
            let name = name.as_ref().to_owned();
            CompletionItem {
                label: name.clone(),
                insert: format!("{prefix}{name} "),
            }
        })
        .collect();
    Some((query.to_owned(), items))
}

impl Model {
    pub(crate) fn refresh_command_completion(&mut self) {
        let InputMode::CommandLine { buffer } = &self.ui.input_mode else {
            self.overlays.command_completion = None;
            return;
        };
        #[cfg(target_os = "linux")]
        let plugin_ids = self
            .plugins
            .entries
            .iter()
            .map(|plugin| plugin.manifest.id.as_str())
            .collect::<Vec<_>>();
        #[cfg(not(target_os = "linux"))]
        let plugin_ids: Vec<&str> = Vec::new();
        let completion = CommandCompletion::new(buffer.text(), &plugin_ids);
        self.overlays.command_completion = (!completion.is_empty()).then_some(completion);
    }

    pub(crate) fn complete_command_line(&mut self) {
        if let Some(completion) = self.overlays.command_completion.take() {
            if let Some(insert) = completion.selected() {
                self.set_command_line(insert);
            }
            self.file.dirty = true;
            return;
        }
        self.refresh_command_completion();
        let single = self
            .overlays
            .command_completion
            .as_ref()
            .filter(|completion| completion.filtered().len() == 1)
            .and_then(CommandCompletion::selected)
            .map(str::to_owned);
        if let Some(insert) = single {
            self.overlays.command_completion = None;
            self.set_command_line(&insert);
        }
        self.file.dirty = true;
    }

    pub(crate) fn move_command_completion(&mut self, delta: isize) {
        if let Some(completion) = self.overlays.command_completion.as_mut() {
            completion.move_by(delta);
            self.file.dirty = true;
        }
    }

    pub(crate) fn close_command_completion(&mut self) -> bool {
        if self.overlays.command_completion.take().is_some() {
            self.file.dirty = true;
            true
        } else {
            false
        }
    }

    fn set_command_line(&mut self, command: &str) {
        self.ui.input_mode = InputMode::CommandLine {
            buffer: InputBuffer::new(command),
        };
        self.file.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(completion: &CommandCompletion) -> Vec<&str> {
        completion
            .filtered()
            .iter()
            .map(|index| completion.items()[*index].label.as_str())
            .collect()
    }

    #[test]
    fn completes_exact_command_prefix() {
        let completion = CommandCompletion::new("theme", &[]);
        assert_eq!(
            labels(&completion),
            vec!["theme", "theme-auto", "themes", "theme-save"]
        );
        assert_eq!(
            labels(&CommandCompletion::new("theme ", &[])),
            theme::names()
        );
    }
    #[test]
    fn completes_command_prefixes() {
        let completion = CommandCompletion::new("the", &[]);
        assert_eq!(
            labels(&completion),
            vec!["theme", "theme-auto", "themes", "theme-save"]
        );
        assert_eq!(
            labels(&CommandCompletion::new("set", &[])),
            vec!["settings"]
        );
    }

    #[test]
    fn completes_theme_arguments() {
        let completion = CommandCompletion::new("theme so", &[]);
        assert_eq!(labels(&completion), vec!["solarized-light"]);
        assert_eq!(completion.selected(), Some("theme solarized-light "));
    }

    #[test]
    fn completes_second_auto_theme_argument() {
        let completion = CommandCompletion::new("theme-auto nord so", &[]);
        assert_eq!(labels(&completion), vec!["solarized-light"]);
        assert_eq!(
            completion.selected(),
            Some("theme-auto nord solarized-light ")
        );
    }

    #[test]
    fn completes_plugin_ids() {
        let completion = CommandCompletion::new("plugin-enable de", &["demo"]);
        assert_eq!(labels(&completion), vec!["demo"]);
        assert_eq!(completion.selected(), Some("plugin-enable demo "));
    }

    #[test]
    fn completion_closes_after_unknown_argument() {
        let completion = CommandCompletion::new("w file", &[]);
        assert!(completion.is_empty());
    }

    #[test]
    fn completion_covers_every_manifest_command() {
        let completion = CommandCompletion::new("", &[]);
        let labels = labels(&completion);
        for command in anvil_control::COMMANDS {
            assert!(
                labels.contains(&command.name),
                "completion missing {}",
                command.name
            );
        }
    }
}

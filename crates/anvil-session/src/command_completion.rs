//! UI-neutral command-line completion policy.

use anvil_api::{
    CompletionContext, CompletionItem, CompletionKind, CompletionRequest, CompletionResponse,
};

const MAX_CANDIDATES: usize = 256;

/// Inputs a command-line completer may use without owning UI state.
pub struct CommandCompletionData<'a> {
    pub commands: &'a [&'a str],
    pub path_entries: &'a [anvil_ipc::DirEntry],
    pub plugin_ids: &'a [String],
    pub themes: &'a [String],
    pub buffers: &'a [String],
    pub git: bool,
    pub desktop: bool,
}

/// Kernel completion interface used by command-line frontends.
pub trait Completer: Send + Sync {
    fn complete(
        &self,
        request: &CompletionRequest,
        data: &CommandCompletionData<'_>,
    ) -> CompletionResponse;
}

/// Built-in command, path, and buffer completion.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltinCompleter;

impl Completer for BuiltinCompleter {
    fn complete(
        &self,
        request: &CompletionRequest,
        data: &CommandCompletionData<'_>,
    ) -> CompletionResponse {
        let query = command_query(request);
        let trimmed = query.trim_start();
        let (command, argument, has_argument) = match trimmed.split_once(char::is_whitespace) {
            Some((command, argument)) => (command, argument.trim_start(), true),
            None => (trimmed, "", false),
        };

        let mut items = match request.context {
            CompletionContext::Command => {
                if has_argument {
                    argument_items(command, argument, data)
                } else {
                    command_items(trimmed, data)
                }
            }
            CompletionContext::Path => path_items(command, argument, data),
            CompletionContext::Buffer => buffer_items(argument, data),
        };
        let truncated = items.len() > MAX_CANDIDATES;
        items.truncate(MAX_CANDIDATES);
        CompletionResponse {
            request_id: 0,
            revision: request.workspace_revision,
            items,
            truncated,
        }
    }
}

fn command_query(request: &CompletionRequest) -> &str {
    let mut cursor = (request.cursor as usize).min(request.query.len());
    while cursor > 0 && !request.query.is_char_boundary(cursor) {
        cursor -= 1;
    }
    &request.query[..cursor]
}

fn command_items(query: &str, data: &CommandCompletionData<'_>) -> Vec<CompletionItem> {
    data.commands
        .iter()
        .filter(|command| {
            (data.git || !command.name.starts_with("git-"))
                && (data.desktop || command.name != "open-url")
        })
        .filter(|command| command.to_ascii_lowercase().starts_with(query))
        .map(|command| CompletionItem {
            label: (*command).to_owned(),
            replacement: format!("{command} "),
            detail: None,
            kind: CompletionKind::Command,
            source: "builtin".to_owned(),
        })
        .collect()
}

fn argument_items(
    command: &str,
    argument: &str,
    data: &CommandCompletionData<'_>,
) -> Vec<CompletionItem> {
    let (names, max_arguments): (Vec<&str>, usize) = match command {
        "theme" => (data.themes.iter().map(String::as_str).collect(), 1),
        "theme-auto" => (data.themes.iter().map(String::as_str).collect(), 2),
        "plugin-enable" => (data.plugin_ids.iter().map(String::as_str).collect(), 1),
        _ => return Vec::new(),
    };
    let (before, query) = if argument.ends_with(char::is_whitespace) {
        (argument, "")
    } else if let Some((before, query)) = argument.rsplit_once(char::is_whitespace) {
        (before, query)
    } else {
        ("", argument)
    };
    if before.split_whitespace().count() >= max_arguments {
        return Vec::new();
    }
    let prefix = if before.is_empty() {
        format!("{command} ")
    } else {
        format!("{command} {before} ")
    };
    names
        .into_iter()
        .filter(|name| {
            name.to_ascii_lowercase()
                .starts_with(&query.to_ascii_lowercase())
        })
        .map(|name| CompletionItem {
            label: name.to_owned(),
            replacement: format!("{prefix}{name} "),
            detail: None,
            kind: CompletionKind::Command,
            source: "builtin".to_owned(),
        })
        .collect()
}

fn path_items(
    command: &str,
    argument: &str,
    data: &CommandCompletionData<'_>,
) -> Vec<CompletionItem> {
    if command.is_empty() || !is_path_command(command) {
        return Vec::new();
    }
    let (directory, query) = match argument.rsplit_once('/') {
        Some((directory, query)) => (format!("{directory}/"), query),
        None => (String::new(), argument),
    };
    let mut entries = data.path_entries.iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries
        .into_iter()
        .filter_map(|entry| {
            let name = sanitize_name(&entry.name)?;
            if !name
                .to_ascii_lowercase()
                .starts_with(&query.to_ascii_lowercase())
            {
                return None;
            }
            let label = if entry.is_dir {
                format!("{directory}{name}/")
            } else {
                format!("{directory}{name}")
            };
            let suffix = if entry.is_dir { "" } else { " " };
            Some(CompletionItem {
                label: label.clone(),
                replacement: format!("{command} {label}{suffix}"),
                detail: None,
                kind: if entry.is_dir {
                    CompletionKind::Directory
                } else {
                    CompletionKind::Path
                },
                source: "builtin".to_owned(),
            })
        })
        .collect()
}

fn buffer_items(argument: &str, data: &CommandCompletionData<'_>) -> Vec<CompletionItem> {
    let query = argument.to_ascii_lowercase();
    data.buffers
        .iter()
        .filter(|buffer| buffer.to_ascii_lowercase().starts_with(&query))
        .map(|buffer| CompletionItem {
            label: buffer.clone(),
            replacement: format!("buffer {buffer} "),
            detail: None,
            kind: CompletionKind::Buffer,
            source: "builtin".to_owned(),
        })
        .collect()
}

const PATH_COMMANDS: &[&str] = &[
    "e",
    "w",
    "workspace-add",
    "workspace-remove",
    "workspace-trust",
    "workspace-untrust",
];

/// Whether command completion should request workspace path entries.
pub fn is_path_command(command: &str) -> bool {
    PATH_COMMANDS.contains(&command)
}

fn sanitize_name(name: &str) -> Option<String> {
    let sanitized = name
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    (!sanitized.is_empty() && sanitized == name).then_some(sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_COMMANDS: &[&str] = &["e", "theme"];

    fn data<'a>(
        path_entries: &'a [anvil_ipc::DirEntry],
        themes: &'a [String],
        buffers: &'a [String],
    ) -> CommandCompletionData<'a> {
        CommandCompletionData {
            commands: TEST_COMMANDS,
            path_entries,
            plugin_ids: &[],
            themes,
            buffers,
            git: false,
            desktop: false,
        }
    }

    fn request(query: &str, context: CompletionContext) -> CompletionRequest {
        CompletionRequest {
            query: query.to_owned(),
            cursor: query.len() as u64,
            context,
            workspace_revision: 7,
        }
    }

    #[test]
    fn command_names_are_derived_from_input() {
        let themes = Vec::new();
        let buffers = Vec::new();
        let response = BuiltinCompleter.complete(
            &request("e", CompletionContext::Command),
            &data(&[], &themes, &buffers),
        );
        assert!(response.items.iter().any(|item| item.label == "e"));
        assert_eq!(response.revision, 7);
    }

    #[test]
    fn path_completion_returns_full_replacements() {
        let entries = vec![
            anvil_ipc::DirEntry {
                ino: 0,
                name: "main.rs".to_owned(),
                is_dir: false,
            },
            anvil_ipc::DirEntry {
                ino: 0,
                name: "main".to_owned(),
                is_dir: true,
            },
        ];
        let themes = Vec::new();
        let buffers = Vec::new();
        let response = BuiltinCompleter.complete(
            &request("e mai", CompletionContext::Path),
            &data(&entries, &themes, &buffers),
        );
        assert_eq!(response.items.len(), 2);
        assert!(response.items.iter().any(|item| item.label == "main/"));
        assert!(
            response
                .items
                .iter()
                .any(|item| item.replacement == "e main.rs ")
        );
    }

    #[test]
    fn buffer_completion_is_available_to_the_contract() {
        let themes = Vec::new();
        let buffers = vec!["src/main.rs".to_owned()];
        let response = BuiltinCompleter.complete(
            &request("ma", CompletionContext::Buffer),
            &data(&[], &themes, &buffers),
        );
        assert_eq!(response.items[0].kind, CompletionKind::Buffer);
        assert_eq!(response.items[0].replacement, "buffer src/main.rs ");
    }

    #[test]
    fn command_completion_keeps_theme_and_plugin_arguments() {
        let themes = vec!["anvil-dark".to_owned()];
        let buffers = Vec::new();
        let response = BuiltinCompleter.complete(
            &request("theme an", CompletionContext::Command),
            &data(&[], &themes, &buffers),
        );
        assert_eq!(response.items[0].replacement, "theme anvil-dark ");
    }

    #[test]
    fn path_completion_rejects_control_characters() {
        let entries = vec![anvil_ipc::DirEntry {
            ino: 0,
            name: "bad\nname".to_owned(),
            is_dir: false,
        }];
        let themes = Vec::new();
        let buffers = Vec::new();
        let response = BuiltinCompleter.complete(
            &request("e b", CompletionContext::Path),
            &data(&entries, &themes, &buffers),
        );
        assert!(response.items.is_empty());
    }
}

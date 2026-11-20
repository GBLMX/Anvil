//! Ex command-name and argument completion.

use super::*;
use anvil_api::{CompletionContext, CompletionRequest};
use anvil_session::command_completion::{BuiltinCompleter, CommandCompletionData, Completer};
use anvil_tui_kit::{SelectionItem, SelectionTable};
use anvil_workspace::FileRequest;

const PATH_COMMANDS: &[&str] = &[
    "e",
    "w",
    "workspace-add",
    "workspace-remove",
    "workspace-trust",
    "workspace-untrust",
];

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
    path: Option<PathCompletion>,
}

struct PathCompletion {
    generation: u64,
    command: String,
    directory: String,
    query: String,
    request_path: PathBuf,
}

impl CommandCompletion {
    fn new(buffer: &str, plugin_ids: &[&str], git: bool, desktop: bool) -> Self {
        let themes = theme::names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let plugins = plugin_ids
            .iter()
            .map(|id| (*id).to_owned())
            .collect::<Vec<_>>();
        let request = CompletionRequest {
            query: buffer.to_owned(),
            cursor: buffer.len() as u64,
            context: CompletionContext::Command,
            workspace_revision: 0,
        };
        let data = CommandCompletionData {
            path_entries: &[],
            plugin_ids: &plugins,
            themes: &themes,
            buffers: &[],
            git,
            desktop,
        };
        let response = BuiltinCompleter.complete(&request, &data);
        let items = response
            .items
            .into_iter()
            .map(|item| CompletionItem {
                label: item.label,
                insert: item.replacement,
            })
            .collect();
        Self {
            table: SelectionTable::new(items),
            path: None,
        }
    }
    fn pending(path: PathCompletion) -> Self {
        Self {
            table: SelectionTable::new(Vec::new()),
            path: Some(path),
        }
    }

    fn is_pending(&self) -> bool {
        self.path.is_some()
    }

    fn pending_generation(&self) -> Option<u64> {
        self.path.as_ref().map(|path| path.generation)
    }

    fn apply_path_listing(&mut self, entries: Vec<anvil_ipc::DirEntry>) {
        let Some(path) = self.path.take() else {
            return;
        };
        let query = format!("{} {}{}", path.command, path.directory, path.query);
        let request = CompletionRequest {
            query,
            cursor: (path.command.len() + 1 + path.directory.len() + path.query.len()) as u64,
            context: CompletionContext::Path,
            workspace_revision: 0,
        };
        let data = CommandCompletionData {
            path_entries: &entries,
            plugin_ids: &[],
            themes: &[],
            buffers: &[],
            git: true,
            desktop: true,
        };
        let response = BuiltinCompleter.complete(&request, &data);
        self.table = SelectionTable::new(
            response
                .items
                .into_iter()
                .map(|item| CompletionItem {
                    label: item.label,
                    insert: item.replacement,
                })
                .collect(),
        );
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

impl Model {
    pub(crate) fn refresh_command_completion(&mut self) {
        let InputMode::CommandLine { buffer } = &self.ui.input_mode else {
            self.overlays.command_completion = None;
            return;
        };
        let buffer = buffer.text().to_owned();
        if let Some(mut path) = self.path_completion_request(&buffer) {
            self.ensure_file_worker();
            let generation = self.overlays.completion_generation.wrapping_add(1);
            self.overlays.completion_generation = generation;
            path.generation = generation;
            let request = FileRequest::ListCompletion {
                path: path.request_path.clone(),
                generation,
            };
            if self
                .workspace
                .file_worker
                .as_ref()
                .is_some_and(|worker| worker.request(request).is_ok())
            {
                self.overlays.command_completion = Some(CommandCompletion::pending(path));
            } else {
                self.overlays.command_completion = None;
            }
            return;
        }
        #[cfg(target_os = "linux")]
        let plugin_ids = self
            .runtime
            .plugins
            .entries
            .iter()
            .map(|plugin| plugin.manifest.id.as_str())
            .collect::<Vec<_>>();
        #[cfg(not(target_os = "linux"))]
        let plugin_ids: Vec<&str> = Vec::new();
        let completion = CommandCompletion::new(
            &buffer,
            &plugin_ids,
            self.workspace.git_service.is_some(),
            self.workspace.desktop_service.is_some(),
        );
        self.overlays.command_completion = (!completion.is_empty()).then_some(completion);
    }

    fn path_completion_request(&self, buffer: &str) -> Option<PathCompletion> {
        let buffer = buffer.trim_start();
        let (command, argument, has_argument) = match buffer.split_once(char::is_whitespace) {
            Some((command, argument)) => (command, argument.trim(), true),
            None => (buffer, "", false),
        };
        if !has_argument || !PATH_COMMANDS.contains(&command) {
            return None;
        }
        let argument = argument.trim();
        if argument.starts_with('/')
            || argument.split('/').any(|part| part == "..")
            || argument.chars().any(char::is_control)
        {
            return None;
        }
        let (directory, query) = if argument.is_empty() {
            (String::new(), "")
        } else if argument.ends_with('/') {
            (argument.to_owned(), "")
        } else if let Some((directory, query)) = argument.rsplit_once('/') {
            (format!("{directory}/"), query)
        } else {
            (String::new(), argument)
        };
        let relative = directory.trim_end_matches('/');
        let request_path = if relative.is_empty() {
            self.workspace.path.clone()
        } else {
            self.workspace.path.join(relative)
        };
        Some(PathCompletion {
            generation: 0,
            command: command.to_owned(),
            directory,
            query: query.to_owned(),
            request_path,
        })
    }

    pub(crate) fn handle_path_completion(
        &mut self,
        generation: u64,
        result: Result<Vec<anvil_ipc::DirEntry>, String>,
    ) {
        let Some(mut completion) = self.overlays.command_completion.take() else {
            return;
        };
        if completion.pending_generation() != Some(generation) {
            self.overlays.command_completion = Some(completion);
            return;
        }
        let keep = match result {
            Ok(entries) => {
                completion.apply_path_listing(entries);
                !completion.is_empty()
            }
            Err(error) => {
                self.status.text = format!("E: completion: {error}");
                false
            }
        };
        self.overlays.command_completion = keep.then_some(completion);
        self.file.dirty = true;
    }

    pub(crate) fn complete_command_line(&mut self) {
        if self
            .overlays
            .command_completion
            .as_ref()
            .is_some_and(CommandCompletion::is_pending)
        {
            return;
        }
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
        let completion = CommandCompletion::new("theme", &[], true, true);
        assert_eq!(
            labels(&completion),
            vec!["theme", "theme-auto", "themes", "theme-save"]
        );
        assert_eq!(
            labels(&CommandCompletion::new("theme ", &[], true, true)),
            theme::names()
        );
    }
    #[test]
    fn completes_command_prefixes() {
        let completion = CommandCompletion::new("the", &[], true, true);
        assert_eq!(
            labels(&completion),
            vec!["theme", "theme-auto", "themes", "theme-save"]
        );
        assert_eq!(
            labels(&CommandCompletion::new("set", &[], true, true)),
            vec!["settings", "settings-export", "settings-import"]
        );
    }

    #[test]
    fn completes_theme_arguments() {
        let completion = CommandCompletion::new("theme so", &[], true, true);
        assert_eq!(labels(&completion), vec!["solarized-light", "solarized"]);
        assert_eq!(completion.selected(), Some("theme solarized-light "));
    }

    #[test]
    fn completes_second_auto_theme_argument() {
        let completion = CommandCompletion::new("theme-auto nord so", &[], true, true);
        assert_eq!(labels(&completion), vec!["solarized-light", "solarized"]);
        assert_eq!(
            completion.selected(),
            Some("theme-auto nord solarized-light ")
        );
    }

    #[test]
    fn completes_plugin_ids() {
        let completion = CommandCompletion::new("plugin-enable de", &["demo"], true, true);
        assert_eq!(labels(&completion), vec!["demo"]);
        assert_eq!(completion.selected(), Some("plugin-enable demo "));
    }

    #[test]
    fn completion_closes_after_unknown_argument() {
        let completion = CommandCompletion::new("w file", &[], true, true);
        assert!(completion.is_empty());
    }

    #[test]
    fn completion_covers_every_manifest_command() {
        let completion = CommandCompletion::new("", &[], true, true);
        let labels = labels(&completion);
        for command in anvil_control::COMMANDS {
            assert!(
                labels.contains(&command.name),
                "completion missing {}",
                command.name
            );
        }
    }
    #[test]
    fn desktop_completion_requires_the_desktop_module() {
        let with_desktop = CommandCompletion::new("", &[], true, true);
        assert!(labels(&with_desktop).contains(&"open-url"));

        let without_desktop = CommandCompletion::new("", &[], true, false);
        assert!(!labels(&without_desktop).contains(&"open-url"));
    }

    #[test]
    fn git_completions_require_the_git_module() {
        let with_git = CommandCompletion::new("", &[], true, true);
        assert!(labels(&with_git).contains(&"git-diff"));

        let without_git = CommandCompletion::new("", &[], false, true);
        assert!(
            labels(&without_git)
                .iter()
                .all(|label| !label.starts_with("git-"))
        );
    }

    #[test]
    fn path_completion_filters_directory_entries() {
        let mut completion = CommandCompletion::pending(PathCompletion {
            generation: 1,
            command: "e".to_owned(),
            directory: "src/".to_owned(),
            query: "ma".to_owned(),
            request_path: PathBuf::from("/workspace/src"),
        });
        completion.apply_path_listing(vec![
            anvil_ipc::DirEntry {
                name: "main".to_owned(),
                is_dir: true,
                ino: 1,
            },
            anvil_ipc::DirEntry {
                name: "main.rs".to_owned(),
                is_dir: false,
                ino: 2,
            },
            anvil_ipc::DirEntry {
                name: "mod.rs".to_owned(),
                is_dir: false,
                ino: 3,
            },
        ]);
        assert_eq!(labels(&completion), vec!["src/main/", "src/main.rs"]);
        assert_eq!(completion.selected(), Some("e src/main/"));
    }

    #[test]
    fn path_completion_filters_control_characters() {
        let mut completion = CommandCompletion::pending(PathCompletion {
            generation: 1,
            command: "e".to_owned(),
            directory: "src/".to_owned(),
            query: String::new(),
            request_path: PathBuf::from("/workspace/src"),
        });
        completion.apply_path_listing(vec![
            anvil_ipc::DirEntry {
                name: "safe.rs".to_owned(),
                is_dir: false,
                ino: 1,
            },
            anvil_ipc::DirEntry {
                name: "evil\x1b[31m.rs".to_owned(),
                is_dir: false,
                ino: 2,
            },
            anvil_ipc::DirEntry {
                name: "bad\nname".to_owned(),
                is_dir: false,
                ino: 3,
            },
        ]);
        assert_eq!(labels(&completion), vec!["src/safe.rs"]);
        assert!(
            completion
                .selected()
                .is_some_and(|value| { !value.chars().any(char::is_control) })
        );
    }

    #[test]
    fn path_completion_rejects_parent_traversal() {
        let model = Model::new(EditorState::from_text("alpha"), None);
        assert!(model.path_completion_request("e ../secret").is_none());
        assert!(model.path_completion_request("e /etc/passwd").is_none());
    }

    #[test]
    fn accepting_a_completion_does_not_execute_a_command() {
        let mut model = Model::new(EditorState::from_text("alpha"), None);
        model.ui.input_mode = InputMode::CommandLine {
            buffer: InputBuffer::new("q"),
        };
        model.refresh_command_completion();
        model.complete_command_line();
        assert!(!model.file.quit);
        assert!(matches!(
            &model.ui.input_mode,
            InputMode::CommandLine { buffer } if buffer.text() == "q "
        ));
    }
}

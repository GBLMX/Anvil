//! Search, substitution, and Ex command dispatch.

use super::*;

impl Model {
    fn substitute(&mut self, whole: bool, spec: &str) {
        if self.reject_read_only() {
            return;
        }
        let mut parts = spec.splitn(3, '/');
        let (Some(old), Some(new)) = (parts.next(), parts.next()) else {
            self.status.text = "E: usage: :s/old/new/[g]".to_owned();
            self.file.dirty = true;
            return;
        };
        let flags = parts.next().unwrap_or_default();
        if old.is_empty() {
            self.status.text = "E: empty search pattern".to_owned();
            self.file.dirty = true;
            return;
        }
        if !flags.is_empty() && flags != "g" {
            self.status.text = format!("E: unknown substitute flags: {flags}");
            self.file.dirty = true;
            return;
        }
        let range = if whole {
            0..self.session.editor.text().len_bytes()
        } else {
            let Some(line) = self
                .session
                .editor
                .index()
                .offset_to_line(self.session.cursor)
            else {
                self.status.text = "E: no line".to_owned();
                self.file.dirty = true;
                return;
            };
            line.start..line.end
        };
        let search_text = self.session.editor.text().slice(range.clone());
        let (matches, truncated) = match crate::search::find_matches_limited(
            &search_text,
            old,
            self.search.state.options,
            crate::search::MAX_SEARCH_MATCHES,
        ) {
            Ok(result) => result,
            Err(error) => {
                self.status.text = format!("E: {error}");
                self.file.dirty = true;
                return;
            }
        };
        if truncated {
            self.status.text = "E: too many matches; narrow the range".to_owned();
            self.file.dirty = true;
            return;
        }
        let mut matches = matches
            .into_iter()
            .map(|found| range.start + found.start..range.start + found.end)
            .collect::<Vec<_>>();
        if !flags.contains('g') {
            matches.truncate(1);
        }
        if matches.is_empty() {
            self.status.text = "E: pattern not found".to_owned();
            self.file.dirty = true;
            return;
        }
        let first = matches[0].start;
        let count = matches.len();
        let edits = matches
            .into_iter()
            .map(|range| (range, new.to_owned()))
            .collect();
        if !self.session.editor.apply_transaction(edits) {
            self.status.text = "E: substitution failed".to_owned();
            self.file.dirty = true;
            return;
        }
        self.session.cursor = first.min(self.session.editor.text().len_bytes());
        self.session
            .editor
            .set_selection_bytes(self.session.cursor, self.session.cursor);
        self.file.modified = true;
        self.file.dirty = true;
        self.status.text = format!("{count} replacements applied");
    }

    pub(crate) fn execute_command(&mut self, command: &str) {
        if let Some(rest) = command.strip_prefix("%s/") {
            self.substitute(true, rest);
            return;
        }
        if let Some(rest) = command.strip_prefix("s/") {
            self.substitute(false, rest);
            return;
        }
        if anvil_control::capability_for_command(command).is_none() {
            self.status.text = format!("E: unknown command: {command}");
            self.file.dirty = true;
            return;
        }
        let mut parts = command.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or_default();
        let argument = parts.next().unwrap_or_default().trim();
        self.queue_effect(Effect::ExCommand {
            name: name.to_owned(),
            argument: argument.to_owned(),
        });
        self.drain_effects();
    }

    fn dispatch_ex(&mut self, name: &str, argument: &str) {
        if name.starts_with("git-") && self.workspace.git_service.is_none() {
            self.status.text = "E: git module is not installed".to_owned();
            self.file.dirty = true;
            return;
        }
        if name == "open-url" && self.workspace.desktop_service.is_none() {
            self.status.text = "E: desktop module is not installed".to_owned();
            self.file.dirty = true;
            return;
        }
        match (name, argument) {
            ("w", "") => self.queue_effect(Effect::Save),
            ("conflict-recover", "") => self.recover_conflict(),
            ("conflict-discard", "") => self.discard_conflict(),
            ("git-diff", "") => self.request_git_diff(false),
            ("git-diff", "--staged") => self.request_git_diff(true),
            ("git-hunk-next", "") => self.move_git_hunk(true),
            ("git-hunk-prev", "") => self.move_git_hunk(false),
            ("git-stage-hunk", "") => self.apply_current_hunk(false),
            ("git-unstage-hunk", "") => self.apply_current_hunk(true),
            ("git-blame", "") => self.request_git_blame(),
            ("config-reload", "") => self.reload_config(),
            ("undo", "") => self.undo(),
            ("redo", "") => self.redo(),
            ("comment", "") => self.toggle_comment(),
            ("git-conflicts", "") => self.request_git_conflicts(),
            ("git-conflict-base", "") => self.request_git_conflict_stage(1),
            ("git-conflict-ours", "") => self.request_git_conflict_stage(2),
            ("git-conflict-theirs", "") => self.request_git_conflict_stage(3),
            ("search-project", query) => self.request_project_search(query),
            ("search-cancel", "") => self.cancel_project_search(),
            ("replace-project", spec) => self.preview_project_replace(spec),
            ("replace-project-apply", "") => self.apply_project_replace(),
            ("replace-project-cancel", "") => self.cancel_project_replace(),
            ("git-resolve-ours", "") => self.resolve_git_conflict(2, false),
            ("git-resolve-ours!", "") => self.resolve_git_conflict(2, true),
            ("git-resolve-theirs", "") => self.resolve_git_conflict(3, false),
            ("git-resolve-theirs!", "") => self.resolve_git_conflict(3, true),
            ("w", path) => self.queue_effect(Effect::SaveAs(path.into())),
            ("wa", "") => self.queue_effect(Effect::SaveAll),
            ("e", path) => self.queue_effect(Effect::Open(path.into())),
            ("problems", "") => self.toggle_problems(),
            ("replace", value) => {
                self.replace_current(value);
            }
            ("replace-all", value) => {
                self.replace_all(value);
            }
            ("bn", "") => self.switch_buffer(1),
            ("bp", "") => self.switch_buffer(-1),
            ("buffers", "") => self.open_buffer_picker(),
            ("new", "") => self.new_buffer(),
            ("bd", "") => self.close_buffer(self.buffers.active, false),
            ("bd!", "") => self.close_buffer(self.buffers.active, true),
            ("bd", index) => self.close_buffer_number(index, false),
            ("bd!", index) => self.close_buffer_number(index, true),
            ("q", "") => self.request_quit(),
            ("q!", "") => self.force_quit(),
            ("cq", "") => self.quit_with_code(1),
            ("help", "") => self.show_help(),
            ("settings", "") => self.open_settings(),
            ("themes", "") => self.open_theme_browser(),
            ("theme", argument) => {
                if argument.is_empty() || argument == "list" {
                    self.status.text = format!(
                        "theme: {} ({:?}); available: {}",
                        theme::name(),
                        theme::choice(),
                        theme::names().join(", ")
                    );
                } else {
                    match theme::set_theme(argument) {
                        Ok(()) => {
                            self.config.settings.theme = theme::choice();
                            let status = format!("theme: {}", theme::name());
                            self.status.text = self.persist_theme_status(status);
                        }
                        Err(error) => self.status.text = format!("E: {error}"),
                    }
                }
                self.file.dirty = true;
            }
            ("theme-auto", argument) => {
                let (dark, light) = if argument.is_empty() {
                    theme::auto_mapping()
                } else {
                    let parts = argument.split_whitespace().collect::<Vec<_>>();
                    if let [dark, light] = parts.as_slice() {
                        (*dark, *light)
                    } else {
                        self.status.text = "E: usage: :theme-auto [dark] [light]".to_owned();
                        self.file.dirty = true;
                        return;
                    }
                };
                match theme::set_auto(dark, light) {
                    Ok(()) => {
                        self.config.settings.theme = theme::choice();
                        let status = format!(
                            "theme: auto (dark {}, light {})",
                            theme::auto_mapping().0,
                            theme::auto_mapping().1
                        );
                        self.status.text = self.persist_theme_status(status);
                    }
                    Err(error) => self.status.text = format!("E: {error}"),
                }
                self.file.dirty = true;
            }
            ("theme-save", "") => {
                self.status.text = match settings::save(self.config.settings) {
                    Ok(path) => format!("theme saved: {}", path.display()),
                    Err(error) => format!("E: {error}"),
                };
                self.file.dirty = true;
            }
            ("pkg", argument) => {
                if let Some(url) = argument.strip_prefix("install ") {
                    self.start_pkg_install(url.trim());
                } else if let Some(rest) = argument.strip_prefix("install-file ") {
                    let parts = rest.split_whitespace().collect::<Vec<_>>();
                    if let [package, signature, public_key] = parts.as_slice() {
                        self.start_pkg_install_file(package, signature, public_key);
                    } else {
                        self.status.text =
                            "E: usage: :pkg install-file <package> <signature> <public-key>"
                                .to_owned();
                        self.file.dirty = true;
                    }
                } else {
                    self.status.text = "E: usage: :pkg install <url> | :pkg install-file <package> <signature> <public-key>".to_owned();
                    self.file.dirty = true;
                }
            }
            ("reconnect", "") => self.queue_effect(Effect::ReconnectGateway),
            ("lsp-start", "") => self.queue_effect(Effect::LspStart),
            ("tree", "") => self.open_tree(),
            ("lsp-stop", "") => self.queue_effect(Effect::LspStop),
            ("format", "") => self.queue_effect(Effect::LspFormat),
            ("open-url", url) => {
                if url.is_empty() {
                    self.status.text = "E: usage: :open-url <http(s)-url>".to_owned();
                    self.file.dirty = true;
                    return;
                }
                match self
                    .workspace
                    .desktop_service
                    .as_ref()
                    .expect("desktop service checked before dispatch")
                    .open_url(url)
                {
                    Ok(()) => self.status.text = "opened external URL".to_owned(),
                    Err(error) => self.status.text = format!("E: {error}"),
                }
                self.file.dirty = true;
            }
            ("breakpoint", "") => self.toggle_breakpoint(),
            ("dap-attach", argument) => {
                let parts = argument.split_whitespace().collect::<Vec<_>>();
                let Some(program) = parts.first() else {
                    self.start_dap_auto();
                    self.file.dirty = true;
                    return;
                };
                let args = parts[1..]
                    .iter()
                    .map(|arg| (*arg).to_owned())
                    .collect::<Vec<_>>();
                self.start_dap_attach(std::path::PathBuf::from(program), args);
                self.file.dirty = true;
            }
            ("dap-detach", "") => {
                self.language.dap.client = None;
                self.language.dap.attach = None;
                self.language.dap.attach_started = None;
                self.status.text = "debug adapter detached".to_owned();
                self.file.dirty = true;
            }
            ("debug", "") => self.toggle_debug_panel(),
            ("debug-refresh", "") => self.request_debug_refresh(),
            ("debug-panel", panel) => self.set_debug_panel(panel),
            ("debug-watch", expression) => self.add_debug_watch(expression),
            ("debug-watch-clear", "") => self.clear_debug_watches(),
            ("symbols", "") => self.queue_effect(Effect::LspDocumentSymbols),
            ("plugin-start", "") => self.queue_effect(Effect::PluginStart),
            ("plugin-list", "") => self.queue_effect(Effect::PluginList),
            ("plugin-oom", "") => self.queue_effect(Effect::PluginOom),
            ("plugin-exec", "") => self.queue_effect(Effect::PluginExec),
            ("plugin-stop", "") => self.queue_effect(Effect::PluginStop),
            ("plugin-enable", plugin_id) => {
                self.queue_effect(Effect::PluginEnable(plugin_id.to_owned()))
            }
            ("task", command) => self.start_task(command),
            ("task-output", "") => self.show_task_output(),
            ("tests", command) => self.start_tests(command),
            #[cfg(unix)]
            ("terminal", shell) => self.start_terminal(shell),
            #[cfg(unix)]
            ("terminal-send", text) => self.send_terminal(text),
            #[cfg(unix)]
            ("terminal-stop", "") => self.stop_terminal(),
            #[cfg(unix)]
            ("terminal-read", "") => self.show_terminal_output(),
            #[cfg(not(unix))]
            ("terminal", _)
            | ("terminal-send", _)
            | ("terminal-stop", "")
            | ("terminal-read", "") => {
                self.status.text = "E: terminal is unsupported on this platform".to_owned();
                self.file.dirty = true;
            }
            ("workspace-add", path) => self.add_workspace_root(path),
            ("workspace-remove", path) => self.remove_workspace_root(path),
            ("workspace-trust", path) => self.trust_workspace_root(path, true),
            ("workspace-untrust", path) => self.trust_workspace_root(path, false),
            ("workspace-list", "") => self.list_workspace_roots(),
            ("recent-files", "") => self.open_recent_files(),
            ("recent-workspaces", "") => self.show_recent_workspaces(),
            ("wq", "") => self.queue_effect(Effect::SaveAndQuit),
            ("wq", path) => self.queue_effect(Effect::SaveAsAndQuit(path.into())),
            _ => {
                self.status.text = format!("E: unknown command: {name}");
                self.file.dirty = true;
            }
        }
    }

    pub(crate) fn queue_effect(&mut self, effect: Effect) {
        if self.config.effects.push(effect).is_err() {
            self.status.text = "E: effect queue full".to_owned();
            self.file.dirty = true;
        }
    }

    pub(crate) fn drain_effects(&mut self) {
        while let Some(queued) = self.config.effects.pop() {
            match queued.effect {
                Effect::ExCommand { name, argument } => {
                    self.dispatch_ex(&name, &argument);
                }
                Effect::Save => {
                    self.save();
                }
                Effect::SaveAs(path) => {
                    let path = path.to_string_lossy().into_owned();
                    self.save_as(&path);
                }
                Effect::SaveAll => {
                    self.save_all();
                }
                Effect::Open(path) => {
                    let path = path.to_string_lossy().into_owned();
                    self.open_file(&path);
                }
                Effect::LspCompletion => self.request_completion(),
                Effect::LspSignatureHelp(trigger) => self.request_signature_help(trigger),
                Effect::LspDefinition => self.request_definition(),
                Effect::LspDeclaration => self.request_declaration(),
                Effect::LspTypeDefinition => self.request_type_definition(),
                Effect::LspImplementation => self.request_implementation(),
                Effect::LspReferences => self.request_references(),
                Effect::LspDocumentHighlights => self.request_document_highlights(),
                Effect::LspIncomingCalls => {
                    self.request_call_hierarchy(CallHierarchyDirection::Incoming);
                }
                Effect::LspOutgoingCalls => {
                    self.request_call_hierarchy(CallHierarchyDirection::Outgoing);
                }
                Effect::LspHover => self.request_hover(),
                Effect::LspCodeActions => self.request_code_actions(),
                Effect::LspDocumentSymbols => self.request_document_symbols(),
                Effect::LspWorkspaceSymbols => self.request_workspace_symbols(),
                Effect::LspRename(name) => self.request_rename(&name),
                Effect::LspFormat => {
                    let _ = self.request_format();
                }
                Effect::TreeCreate {
                    directory,
                    name,
                    kind,
                } => self.finish_tree_create(directory, name, kind),
                Effect::TreeRename { path, name } => self.finish_tree_rename(path, name),
                Effect::TreeTransfer {
                    path,
                    copy,
                    directory,
                } => self.finish_tree_transfer(path, copy, directory),
                Effect::TreeDelete(path) => self.finish_tree_delete(path),
                Effect::SaveBuffer { index, close } => {
                    if self.save_buffer(index) && close {
                        self.ui.input_mode = InputMode::Normal;
                        self.close_buffer(index, true);
                    }
                }
                Effect::SaveAndQuit => {
                    if self.save() {
                        self.finish_save_quit();
                    }
                }
                Effect::SaveAsAndQuit(path) => {
                    let path = path.to_string_lossy().into_owned();
                    if self.save_as(&path) {
                        self.finish_save_quit();
                    }
                }
                Effect::Reload(path) => {
                    if let Err(error) = self.reload_external(&path) {
                        self.status.text = format!("E: {error}");
                        self.file.dirty = true;
                    }
                }
                Effect::ReconnectGateway => self.reconnect_gateway(),
                Effect::LspStart => self.start_lsp(),
                Effect::LspStop => self.stop_lsp(),
                Effect::PluginStart => self.start_host(),
                Effect::PluginList => self.list_plugins(),
                Effect::PluginOom => self.start_oom_host(),
                Effect::PluginExec => self.exec_host(),
                Effect::PluginStop => self.stop_host(),
                Effect::PluginEnable(plugin_id) => self.enable_plugin(&plugin_id),
            }
        }
    }

    fn finish_save_quit(&mut self) {
        if self.any_modified() {
            self.status.text = "E: other buffers have unsaved changes".to_owned();
            self.file.dirty = true;
        } else {
            self.file.quit = true;
        }
    }
    fn close_buffer_number(&mut self, argument: &str, force: bool) {
        let Ok(number) = argument.parse::<usize>() else {
            self.status.text = "E: invalid buffer index".to_owned();
            self.file.dirty = true;
            return;
        };
        if number == 0 {
            self.status.text = "E: buffer indexes start at 1".to_owned();
            self.file.dirty = true;
            return;
        }
        self.close_buffer(number - 1, force);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_module::desktop::{DesktopError, DesktopService};
    use std::sync::{Arc, Mutex};

    struct FakeDesktop(Arc<Mutex<Vec<String>>>);

    impl DesktopService for FakeDesktop {
        fn open_url(&self, url: &str) -> Result<(), DesktopError> {
            self.0.lock().expect("urls").push(url.to_owned());
            Ok(())
        }
    }

    #[test]
    fn open_url_requires_the_desktop_module() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.execute_command("open-url https://example.com");
        assert_eq!(model.status.text, "E: desktop module is not installed");
    }

    #[test]
    fn open_url_forwards_the_url_to_the_service() {
        let urls = Arc::new(Mutex::new(Vec::new()));
        let mut model = Model::new(EditorState::from_text(""), None);
        model.workspace.desktop_service = Some(Arc::new(FakeDesktop(Arc::clone(&urls))));
        model.execute_command("open-url https://example.com/a?x=1");
        assert_eq!(
            urls.lock().expect("urls").as_slice(),
            ["https://example.com/a?x=1"]
        );
        assert_eq!(model.status.text, "opened external URL");
    }

    #[test]
    fn open_url_requires_an_argument() {
        let urls = Arc::new(Mutex::new(Vec::new()));
        let mut model = Model::new(EditorState::from_text(""), None);
        model.workspace.desktop_service = Some(Arc::new(FakeDesktop(Arc::clone(&urls))));
        model.execute_command("open-url");
        assert!(urls.lock().expect("urls").is_empty());
        assert_eq!(model.status.text, "E: usage: :open-url <http(s)-url>");
    }
}

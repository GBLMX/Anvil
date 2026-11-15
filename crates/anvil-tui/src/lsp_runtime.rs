//! LSP worker lifecycle, event polling, and diagnostic helpers.

use super::*;
use crate::state::LspPollEffect;
use anvil_session::plugin_events::{DiagnosticsChanged, PluginEvent};

fn truncate_event_text(text: &str, max: usize) -> String {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn inactive_lsp_keys(
    keys: Vec<(String, String)>,
    desired: Option<&(String, String)>,
) -> Vec<(String, String)> {
    keys.into_iter()
        .filter(|key| desired != Some(key))
        .collect()
}

fn diagnostic_code(diagnostic: &anvil_lsp_client::Diagnostic) -> Option<String> {
    diagnostic.extra.get("code").and_then(|code| match code {
        serde_json::Value::String(code) if !code.trim().is_empty() => Some(code.clone()),
        serde_json::Value::Number(code) => Some(code.to_string()),
        _ => None,
    })
}

impl Model {
    pub(crate) fn clear_lsp_ui(&mut self) {
        self.clear_lsp_requests();
        self.cancel_completion();
        self.close_hover();
        self.close_diagnostic_peek();
        self.overlays.lsp_results.code_actions = None;
        self.close_overlay(OverlayKind::CodeActions);
        self.language.code_lens = None;
        self.overlays.lsp_results.document_symbols = None;
        self.close_overlay(OverlayKind::DocumentSymbols);
        self.overlays.lsp_results.references = None;
        self.close_overlay(OverlayKind::References);
        self.overlays.lsp_results.document_highlights = None;
        self.overlays.lsp_results.call_hierarchy = None;
        self.close_overlay(OverlayKind::CallHierarchy);
        self.language.semantic_tokens.legend.clear();
    }

    fn retain_lsp_worker(&mut self, desired: Option<&(String, String)>) {
        self.language.lsp.desired = desired.cloned();
        let keys = self.language.lsp.workers.keys().cloned().collect();
        for key in inactive_lsp_keys(keys, desired) {
            self.language.lsp.workers.remove(&key);
            self.language.lsp.retries.remove(&key);
            self.language.lsp.status.remove(&key);
            self.language.inlay_hints.support.remove(&key);
        }
        let status_keys = self.language.lsp.status.keys().cloned().collect();
        for key in inactive_lsp_keys(status_keys, desired) {
            self.language.lsp.status.remove(&key);
            self.language.lsp.retries.remove(&key);
        }
        let label_keys = self.language.lsp.labels.keys().cloned().collect();
        for key in inactive_lsp_keys(label_keys, desired) {
            self.language.lsp.labels.remove(&key);
        }
        if self
            .language
            .lsp
            .config
            .as_ref()
            .is_some_and(|key| desired != Some(key))
        {
            self.language.lsp.config = None;
        }
        if self
            .language
            .lsp
            .retry
            .as_ref()
            .is_some_and(|(key, _)| desired != Some(key))
        {
            self.language.lsp.retry = None;
        }
    }

    pub(crate) fn active_lsp_worker(&self) -> Option<&LspWorker> {
        self.language
            .lsp
            .config
            .as_ref()
            .and_then(|key| self.language.lsp.workers.get(key))
    }

    pub(crate) fn lsp_key(&self, path: &Path) -> Option<(String, String)> {
        lsp_settings_with_service(path, self.workspace.language_tooling_service.as_deref())
            .map(|settings| (settings.tool, settings.language_id))
    }

    fn lsp_status_root(&self, key: &(String, String)) -> PathBuf {
        self.language
            .lsp
            .status
            .get(key)
            .map(|status| status.root().to_path_buf())
            .or_else(|| {
                self.language
                    .lsp
                    .workers
                    .get(key)
                    .map(|worker| worker.root().to_path_buf())
            })
            .unwrap_or_else(|| self.workspace.path.clone())
    }

    pub(crate) fn show_lsp_status(&mut self) {
        let Some(path) = self.session.path.clone() else {
            self.status.text = "E: lsp requires an open file".to_owned();
            self.file.dirty = true;
            return;
        };
        let Some(settings) =
            lsp_settings_with_service(&path, self.workspace.language_tooling_service.as_deref())
        else {
            self.status.text = format!("E: no language server for {}", path.display());
            self.file.dirty = true;
            return;
        };
        let key = (settings.tool, settings.language_id);
        let status = self.language.lsp.status.get(&key);
        let root = self.lsp_status_root(&key);
        let state = status.map(LspStatus::label).unwrap_or_else(|| {
            if self.language.lsp.workers.contains_key(&key) {
                "starting"
            } else {
                "pending"
            }
        });
        let retries = self.language.lsp.retries.get(&key).copied().unwrap_or(0);
        let markers = if settings.root_markers.is_empty() {
            "-".to_owned()
        } else {
            settings.root_markers.join(",")
        };
        let mut detail = format!(
            "lsp {} ({}) {state} · root {} (primary)",
            settings.label,
            key.1,
            root.display()
        );
        if let Some(LspStatus::Running { server_info, .. }) = status
            && let Some(server_info) = server_info
        {
            detail.push_str(&format!(" · server {server_info}"));
        }
        if let Some(error) = match status {
            Some(LspStatus::Failed { error, .. } | LspStatus::Restarting { error, .. }) => {
                Some(error.as_str())
            }
            _ => None,
        } {
            detail.push_str(&format!(" · {error}"));
        }
        detail.push_str(&format!(" · retries {retries} · markers {markers}"));
        self.status.text = detail;
        self.file.dirty = true;
    }

    pub(crate) fn sync_lsp_active(&mut self) {
        let Some(path) = self.session.path.clone() else {
            self.language.lsp.config = None;
            self.retain_lsp_worker(None);
            self.clear_lsp_ui();
            return;
        };
        let Some(settings) =
            lsp_settings_with_service(&path, self.workspace.language_tooling_service.as_deref())
        else {
            self.language.lsp.config = None;
            self.retain_lsp_worker(None);
            self.clear_lsp_ui();
            return;
        };
        self.activate_plugins(ActivationEvent::Language(settings.language_id.clone()));
        let desired = (settings.tool.clone(), settings.language_id.clone());
        self.language
            .lsp
            .labels
            .insert(desired.clone(), settings.label.clone());
        self.retain_lsp_worker(Some(&desired));
        if !self.language.lsp.workers.contains_key(&desired) {
            self.start_lsp();
            if self.active_lsp_worker().is_some() {
                self.request_folding_ranges(None);
                self.request_code_lens();
            }
            return;
        }
        if self.language.lsp.config.as_ref() != Some(&desired) {
            self.language.lsp.config = Some(desired.clone());
            self.clear_lsp_ui();
        }
        let text = self
            .session
            .editor
            .text()
            .slice(0..self.session.editor.text().len_bytes());
        let has_worker = self.sync_lsp_document(&path, text).is_ok();
        if has_worker {
            self.request_semantic_tokens();
            self.request_folding_ranges(None);
            self.request_code_lens();
        }
    }

    pub(crate) fn start_lsp(&mut self) {
        let Some(path) = self.session.path.clone() else {
            self.status.text = "E: lsp requires an open file".to_owned();
            self.file.dirty = true;
            return;
        };
        let Some(settings) =
            lsp_settings_with_service(&path, self.workspace.language_tooling_service.as_deref())
        else {
            self.status.text = format!("E: no language server for {}", path.display());
            self.file.dirty = true;
            return;
        };
        let desired = (settings.tool.clone(), settings.language_id.clone());
        self.language
            .lsp
            .labels
            .insert(desired.clone(), settings.label.clone());
        self.retain_lsp_worker(Some(&desired));
        if self.language.lsp.workers.contains_key(&desired) {
            self.language.lsp.config = Some(desired.clone());
            if !self.language.lsp.status.contains_key(&desired) {
                let root = self
                    .language
                    .lsp
                    .workers
                    .get(&desired)
                    .map(|worker| worker.root().to_path_buf())
                    .unwrap_or_else(|| self.workspace.path.clone());
                self.language
                    .lsp
                    .status
                    .insert(desired.clone(), LspStatus::Starting { root });
            }
            self.clear_lsp_ui();
            let text = self
                .session
                .editor
                .text()
                .slice(0..self.session.editor.text().len_bytes());
            let _ = self.sync_lsp_document(&path, text);
            self.status.text = "lsp already running".to_owned();
            self.file.dirty = true;
            return;
        }
        let Some(client) = self.language.lsp.client.take().or_else(|| {
            self.workspace
                .gateway_session
                .as_mut()
                .and_then(|session| session.connect_lsp().ok())
        }) else {
            self.language.lsp.status.insert(
                desired,
                LspStatus::Failed {
                    root: self.workspace.path.clone(),
                    error: "lsp gateway connection unavailable".to_owned(),
                },
            );
            self.status.text = "E: lsp gateway connection unavailable".to_owned();
            self.file.dirty = true;
            return;
        };
        let text = self
            .session
            .editor
            .text()
            .slice(0..self.session.editor.text().len_bytes());
        let config = LspConfig {
            tool: settings.tool,
            args: settings.args,
            language_id: settings.language_id,
            root_markers: settings.root_markers,
            initialization_options: settings.initialization_options,
        };
        match LspWorker::start(
            client,
            config,
            self.workspace.path.clone(),
            path.clone(),
            text,
        ) {
            Ok(worker) => {
                self.language.lsp.config = Some(desired.clone());
                let root = worker.root().to_path_buf();
                self.language.lsp.workers.insert(desired.clone(), worker);
                self.language
                    .lsp
                    .status
                    .insert(desired.clone(), LspStatus::Starting { root });
                self.language.lsp_requests.versions.insert(path.clone(), 1);
                self.language.lsp.retries.entry(desired).or_insert(0);
                self.clear_lsp_ui();
                self.status.text = if settings.label == "jdtls" {
                    "jdtls starting (indexing workspace)".to_owned()
                } else {
                    format!("{} starting", settings.label)
                };
            }
            Err(error) => {
                let message = format!("lsp start: {error}");
                self.language.lsp.status.insert(
                    desired.clone(),
                    LspStatus::Failed {
                        root: self.workspace.path.clone(),
                        error: message.clone(),
                    },
                );
                self.status.text = format!("E: {message}");
            }
        }
        self.file.dirty = true;
    }

    pub(crate) fn stop_lsp(&mut self) {
        if let Some(key) = self.language.lsp.config.take() {
            let root = self.lsp_status_root(&key);
            self.language.lsp.workers.remove(&key);
            self.language.lsp.retries.remove(&key);
            self.language
                .lsp
                .status
                .insert(key.clone(), LspStatus::Stopped { root });
            self.language.inlay_hints.support.remove(&key);
        }
        self.clear_lsp_ui();
        self.status.text = "lsp stopped".to_owned();
        self.file.dirty = true;
    }

    fn handle_lsp_event(&mut self, key: &(String, String), event: LspEvent) {
        let active = self.language.lsp.config.as_ref() == Some(key);
        match event {
            LspEvent::Capabilities {
                inlay_hints,
                semantic_token_types,
                server_info,
            } => {
                self.language.lsp.retries.remove(key);
                self.language.lsp.retry = None;
                let root = self.lsp_status_root(key);
                self.language.lsp.status.insert(
                    key.clone(),
                    LspStatus::Running {
                        root,
                        server_info: server_info.clone(),
                    },
                );
                self.language
                    .inlay_hints
                    .support
                    .insert(key.clone(), inlay_hints);
                if active {
                    if let Some(info) = server_info {
                        self.status.text = format!("lsp: {info}");
                    }
                    self.language.inlay_hints.hints.clear();
                    self.language.inlay_hints.pending = None;
                    self.language.inlay_hints.checked = None;
                    self.language.semantic_tokens.legend = semantic_token_types;
                    self.language.semantic_tokens.tokens.clear();
                    self.language.semantic_tokens.revision = None;
                    self.request_semantic_tokens();
                }
            }
            LspEvent::Diagnostics(path, diagnostics) => {
                let path_text = path.display().to_string();
                let root_text = self.workspace.path.display().to_string();
                let error_count = diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.severity == Some(1))
                    .count() as u32;
                let warning_count = diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.severity == Some(2))
                    .count() as u32;
                let first_error: Option<String> = diagnostics
                    .iter()
                    .find(|diagnostic| diagnostic.severity == Some(1))
                    .map(|diagnostic| truncate_event_text(&diagnostic.message, 512));
                self.language.diagnostics.by_path.insert(path, diagnostics);
                self.language.diagnostics.generation =
                    self.language.diagnostics.generation.saturating_add(1);
                let payload = DiagnosticsChanged {
                    root: root_text,
                    path: path_text,
                    generation: self.language.diagnostics.generation,
                    error_count,
                    warning_count,
                    first_error,
                };
                self.runtime.plugins.supervisor.publish_event(
                    PluginEvent::DiagnosticsChanged,
                    &serde_json::to_value(payload).expect("diagnostics event serializes"),
                );
                self.status.text = format!(
                    "lsp diagnostics: {}",
                    self.language
                        .diagnostics
                        .by_path
                        .values()
                        .map(Vec::len)
                        .sum::<usize>()
                );
                if active
                    && !self.language.semantic_tokens.legend.is_empty()
                    && self.language.semantic_tokens.revision
                        != Some(self.session.editor.revision())
                {
                    self.request_semantic_tokens();
                }
            }
            LspEvent::Crashed(error) => {
                let root = self.lsp_status_root(key);
                self.language.lsp.workers.remove(key);
                if active {
                    self.language.lsp.config = None;
                    self.clear_lsp_ui();
                    let retries = {
                        let retries = self.language.lsp.retries.entry(key.clone()).or_insert(0);
                        *retries += 1;
                        *retries
                    };
                    let status = if retries <= 3 {
                        let delay_ms = 250u64 << (retries.saturating_sub(1).min(3));
                        self.language.lsp.retry = Some((
                            key.clone(),
                            Instant::now() + Duration::from_millis(delay_ms),
                        ));
                        self.status.text =
                            format!("lsp crashed; restarting in {delay_ms}ms ({retries}/3)");
                        LspStatus::Restarting {
                            root,
                            error: error.clone(),
                            attempt: retries,
                        }
                    } else {
                        self.status.text = format!("E: lsp crashed: {error}");
                        self.notify(
                            anvil_session::ui::NotificationSeverity::Error,
                            "lsp",
                            format!("crashed: {error}"),
                        );
                        LspStatus::Failed { root, error }
                    };
                    self.language.lsp.status.insert(key.clone(), status);
                } else {
                    self.language.lsp.status.remove(key);
                    self.language.lsp.retries.remove(key);
                }
            }
            LspEvent::Response { id, result, error } => {
                self.handle_lsp_response(id, result, error);
            }
            LspEvent::StartupError(error) => {
                let root = self.lsp_status_root(key);
                self.language.lsp.workers.remove(key);
                if active {
                    self.language.lsp.config = None;
                    self.clear_lsp_ui();
                    self.language.lsp.status.insert(
                        key.clone(),
                        LspStatus::Failed {
                            root,
                            error: error.clone(),
                        },
                    );
                    self.status.text = format!("E: lsp startup failed: {error}");
                    self.notify(
                        anvil_session::ui::NotificationSeverity::Error,
                        "lsp",
                        format!("startup failed: {error}"),
                    );
                } else {
                    self.language.lsp.status.remove(key);
                }
            }
            LspEvent::Error(error) => {
                let root = self.lsp_status_root(key);
                self.language.lsp.workers.remove(key);
                if active {
                    self.language.lsp.config = None;
                    self.clear_lsp_ui();
                    self.language.lsp.status.insert(
                        key.clone(),
                        LspStatus::Failed {
                            root,
                            error: error.clone(),
                        },
                    );
                    self.status.text = format!("E: lsp: {error}");
                    self.notify(anvil_session::ui::NotificationSeverity::Error, "lsp", error);
                } else {
                    self.language.lsp.status.remove(key);
                }
            }
            LspEvent::Log(line) => {
                if active {
                    self.status.text = format!("lsp: {line}");
                }
            }
        }
        self.file.dirty = true;
    }

    pub(crate) fn poll_lsp(&mut self) {
        let effects = self.language.poll(Instant::now());
        for effect in effects {
            match effect {
                LspPollEffect::Event { key, event } => self.handle_lsp_event(&key, event),
                LspPollEffect::Retry { key } => {
                    if !self.language.lsp.workers.contains_key(&key) {
                        self.start_lsp();
                    }
                }
            }
        }
    }

    pub(crate) fn diagnostic_lines(&self) -> Vec<usize> {
        let Some(path) = &self.session.path else {
            return Vec::new();
        };
        self.language
            .diagnostics
            .by_path
            .get(path)
            .map(|diagnostics| {
                diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.range.start.line as usize)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn inline_diagnostic(&self, line_number: usize) -> Option<(String, u32)> {
        let path = self.session.path.as_ref()?;
        let diagnostic = self
            .language
            .diagnostics
            .by_path
            .get(path)?
            .iter()
            .filter(|diagnostic| {
                diagnostic.range.start.line as usize <= line_number
                    && diagnostic.range.end.line as usize >= line_number
            })
            .min_by_key(|diagnostic| diagnostic.severity.unwrap_or(99))?;
        let mut message = diagnostic
            .message
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if let Some(code) = diagnostic_code(diagnostic)
            && !code.is_empty()
        {
            message = format!("[{code}] {message}");
        }
        (!message.is_empty()).then_some((message, diagnostic.severity.unwrap_or(3)))
    }

    pub(crate) fn diagnostic_ranges(
        &self,
        line_number: usize,
        line_start: usize,
        line_end: usize,
    ) -> Vec<(usize, usize)> {
        let Some(path) = &self.session.path else {
            return Vec::new();
        };
        let Some(diagnostics) = self.language.diagnostics.by_path.get(path) else {
            return Vec::new();
        };
        let editor = &self.session.editor;
        let index = editor.index();
        let buffer = editor.text();
        diagnostics
            .iter()
            .filter_map(|diagnostic| {
                if diagnostic.range.start.line as usize > line_number
                    || (diagnostic.range.end.line as usize) < line_number
                {
                    return None;
                }
                let start = if diagnostic.range.start.line as usize == line_number {
                    index.offset_at_utf16(
                        buffer,
                        line_number,
                        diagnostic.range.start.character as usize,
                    )?
                } else {
                    line_start
                };
                let end = if diagnostic.range.end.line as usize == line_number {
                    index.offset_at_utf16(
                        buffer,
                        line_number,
                        diagnostic.range.end.character as usize,
                    )?
                } else {
                    line_end
                };
                let start = index.position(buffer, start)?.display_column;
                let end = index.position(buffer, end)?.display_column.max(start + 1);
                Some((start, end))
            })
            .collect()
    }

    pub(crate) fn reconnect_gateway(&mut self) {
        if !self.workspace.require_gateway {
            self.status.text = "E: gateway is not required in this session".to_owned();
            self.file.dirty = true;
            return;
        }
        self.workspace.file_worker = None;
        self.language.lsp.workers.clear();
        self.language.lsp.client = None;
        self.language.lsp.config = None;
        self.language.lsp.retries.clear();
        self.clear_lsp_ui();
        self.workspace.gateway = None;
        if let Some(mut session) = self.workspace.gateway_session.take() {
            session.shutdown_gracefully(std::time::Duration::from_millis(250));
        }
        match GatewaySession::start(&self.workspace.path) {
            Ok((session, client, lsp_client)) => {
                self.workspace.gateway_session = Some(session);
                self.workspace.gateway = Some(client);
                self.language.lsp.client = Some(lsp_client);
                self.status.text = "gateway reconnected".to_owned();
            }
            Err(error) => {
                self.status.text = format!("E: reconnect failed: {error}");
            }
        }
        self.file.dirty = true;
    }
}

#[cfg(test)]
mod event_text_tests {
    use super::truncate_event_text;

    #[test]
    fn truncation_respects_byte_limit_and_utf8_boundaries() {
        let ascii = "a".repeat(600);
        assert_eq!(truncate_event_text(&ascii, 512).len(), 512);

        let multibyte = "é".repeat(300);
        let truncated = truncate_event_text(&multibyte, 512);
        assert!(truncated.len() <= 512);
        assert!(truncated.is_char_boundary(truncated.len()));
        assert_eq!(truncated.chars().count(), 256);
    }
}

#[cfg(test)]
mod worker_selection_tests {
    use super::inactive_lsp_keys;

    #[test]
    fn switching_language_selects_only_the_active_worker() {
        let rust = ("rust-analyzer".to_owned(), "rust".to_owned());
        let go = ("gopls".to_owned(), "go".to_owned());
        let keys = vec![rust.clone(), go.clone()];
        assert_eq!(inactive_lsp_keys(keys.clone(), Some(&go)), vec![rust]);
        assert_eq!(inactive_lsp_keys(keys.clone(), None), keys);
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;
    use anvil_editor::EditorState;

    #[test]
    fn capabilities_and_crash_update_the_status_snapshot() {
        let mut model = Model::new(
            EditorState::from_text(""),
            Some(PathBuf::from("/ws/main.c")),
        );
        model.workspace.path = PathBuf::from("/ws");
        let key = ("clangd".to_owned(), "c".to_owned());
        model.language.lsp.config = Some(key.clone());
        model.language.lsp.status.insert(
            key.clone(),
            LspStatus::Starting {
                root: PathBuf::from("/ws"),
            },
        );

        model.handle_lsp_event(
            &key,
            LspEvent::Capabilities {
                inlay_hints: true,
                semantic_token_types: Vec::new(),
                server_info: Some("clangd 18".to_owned()),
            },
        );
        assert!(matches!(
            model.language.lsp.status.get(&key),
            Some(LspStatus::Running {
                server_info: Some(info),
                ..
            }) if info == "clangd 18"
        ));

        model.handle_lsp_event(&key, LspEvent::Crashed("boom".to_owned()));
        assert!(model.language.lsp.config.is_none());
        assert!(matches!(
            model.language.lsp.status.get(&key),
            Some(LspStatus::Restarting { attempt: 1, .. })
        ));
        assert!(
            model.status.text.contains("restarting"),
            "{}",
            model.status.text
        );
    }

    #[test]
    fn stopping_records_a_visible_state() {
        let mut model = Model::new(
            EditorState::from_text(""),
            Some(PathBuf::from("/ws/main.c")),
        );
        model.workspace.path = PathBuf::from("/ws");
        let key = ("clangd".to_owned(), "c".to_owned());
        model.language.lsp.config = Some(key.clone());
        model.language.lsp.status.insert(
            key.clone(),
            LspStatus::Running {
                root: PathBuf::from("/ws"),
                server_info: None,
            },
        );
        model.stop_lsp();
        assert!(matches!(
            model.language.lsp.status.get(&key),
            Some(LspStatus::Stopped { .. })
        ));
    }

    #[test]
    fn startup_failure_records_failed_state() {
        let mut model = Model::new(
            EditorState::from_text(""),
            Some(PathBuf::from("/ws/main.c")),
        );
        model.workspace.path = PathBuf::from("/ws");
        let key = ("clangd".to_owned(), "c".to_owned());
        model.language.lsp.config = Some(key.clone());
        model.language.lsp.status.insert(
            key.clone(),
            LspStatus::Starting {
                root: PathBuf::from("/ws"),
            },
        );
        model.handle_lsp_event(&key, LspEvent::StartupError("no server".to_owned()));
        assert!(model.language.lsp.config.is_none());
        assert!(matches!(
            model.language.lsp.status.get(&key),
            Some(LspStatus::Failed { error, .. }) if error == "no server"
        ));
    }

    #[test]
    fn missing_gateway_records_failed_state() {
        let mut model = Model::new(
            EditorState::from_text(""),
            Some(PathBuf::from("/ws/main.c")),
        );
        model.workspace.path = PathBuf::from("/ws");
        let key = model.lsp_key(Path::new("/ws/main.c")).expect("lsp key");
        model.start_lsp();
        assert!(matches!(
            model.language.lsp.status.get(&key),
            Some(LspStatus::Failed { error, .. }) if error.contains("gateway")
        ));
    }
}

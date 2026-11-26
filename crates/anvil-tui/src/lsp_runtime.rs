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

/// Returns the server-provided diagnostic code when present.
fn diagnostic_code(diagnostic: &anvil_lsp_client::Diagnostic) -> Option<String> {
    match diagnostic.extra.get("code") {
        Some(serde_json::Value::String(code)) if !code.trim().is_empty() => Some(code.clone()),
        Some(serde_json::Value::Number(code)) => Some(code.to_string()),
        _ => None,
    }
}

fn lsp_worker_replacement_required(active: &LspKey, desired: &LspKey) -> bool {
    active.0 != desired.0 || active.1 != desired.1
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

    fn retain_lsp_worker(&mut self, desired: Option<&LspKey>) {
        self.language.lsp.desired = desired.cloned();
        let Some(active) = self.language.lsp.config.clone() else {
            return;
        };
        if desired == Some(&active) {
            return;
        }
        self.language.lsp.config = None;
        self.language.lsp.pending_sync = None;
        let Some(desired) = desired else {
            return;
        };
        if !lsp_worker_replacement_required(&active, desired) {
            return;
        }
        let root = self.lsp_status_root(&active);
        self.language.lsp.workers.remove(&active);
        self.language.lsp.retries.remove(&active);
        self.language
            .lsp
            .status
            .insert(active.clone(), LspStatus::Stopped { root });
        self.language.inlay_hints.support.remove(&active);
        self.language.completion.support.remove(&active);
    }

    pub(crate) fn stop_lsp_root(&mut self, root: &Path) {
        let mut keys: Vec<LspKey> = self
            .language
            .lsp
            .workers
            .keys()
            .chain(self.language.lsp.status.keys())
            .chain(self.language.lsp.retries.keys())
            .chain(self.language.lsp.labels.keys())
            .filter(|key| key.2.as_path() == root)
            .cloned()
            .collect();
        keys.sort();
        keys.dedup();
        let mut clear_ui = false;
        for key in keys {
            self.language.lsp.workers.remove(&key);
            self.language.lsp.status.remove(&key);
            self.language.lsp.retries.remove(&key);
            self.language.inlay_hints.support.remove(&key);
            self.language.completion.support.remove(&key);
            self.language.lsp.labels.remove(&key);
            if self.language.lsp.config.as_ref() == Some(&key) {
                clear_ui = true;
                self.language.lsp.config = None;
                self.language.lsp.pending_sync = None;
            }
            if self.language.lsp.desired.as_ref() == Some(&key) {
                clear_ui = true;
            }
            if self
                .language
                .lsp
                .retry
                .as_ref()
                .is_some_and(|(retry, _)| retry == &key)
            {
                self.language.lsp.retry = None;
            }
        }
        if clear_ui {
            self.clear_lsp_ui();
        }
    }

    pub(crate) fn active_lsp_worker(&self) -> Option<&LspWorker> {
        self.language
            .lsp
            .config
            .as_ref()
            .and_then(|key| self.language.lsp.workers.get(key))
    }

    pub(crate) fn lsp_owning_root(&self, path: &Path) -> Option<PathBuf> {
        if let Some(root) = self.workspace.roots.owning_root(path) {
            return root.trusted.then(|| root.path.clone());
        }
        path.starts_with(&self.workspace.path)
            .then(|| self.workspace.path.clone())
    }

    pub(crate) fn lsp_key(&self, path: &Path) -> Option<LspKey> {
        let root = self.lsp_owning_root(path)?;
        crate::lsp_ui::lsp_settings_with_root(
            path,
            &root,
            self.workspace.language_tooling_service.as_deref(),
        )
        .map(|settings| (settings.tool, settings.language_id, root))
    }

    pub(crate) fn notify_lsp_configuration(&self) {
        for worker in self.language.lsp.workers.values() {
            let mut settings = worker.settings();
            if !settings.is_object() {
                settings = serde_json::json!({});
            }
            settings["lsp"] = serde_json::json!({
                "inlay_hints": self.config.settings.lsp.inlay_hints
            });
            let _ = worker.did_change_configuration(settings);
        }
    }

    fn lsp_status_root(&self, key: &LspKey) -> PathBuf {
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
        let Some(owning_root) = self.lsp_owning_root(&path) else {
            self.status.text = format!("E: lsp: outside workspace: {}", path.display());
            self.file.dirty = true;
            return;
        };
        let Some(settings) = crate::lsp_ui::lsp_settings_with_root(
            &path,
            &owning_root,
            self.workspace.language_tooling_service.as_deref(),
        ) else {
            self.status.text = format!(
                "E: no language server for {}; run anvil doctor or configure [lsp.commands]",
                path.display()
            );
            self.file.dirty = true;
            return;
        };
        let key = (
            settings.tool.clone(),
            settings.language_id.clone(),
            owning_root.clone(),
        );
        let status = self.language.lsp.status.get(&key);
        let project_root = self.lsp_status_root(&key);
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
        let root_note = if project_root == owning_root {
            "workspace root".to_owned()
        } else {
            format!("workspace {}", owning_root.display())
        };
        let mut detail = format!(
            "lsp {} ({}) {state} · root {} ({root_note})",
            settings.label,
            key.1,
            project_root.display(),
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
        let Some(root) = self.lsp_owning_root(&path) else {
            self.language.lsp.config = None;
            self.retain_lsp_worker(None);
            self.clear_lsp_ui();
            return;
        };
        let Some(settings) = crate::lsp_ui::lsp_settings_with_root(
            &path,
            &root,
            self.workspace.language_tooling_service.as_deref(),
        ) else {
            self.language.lsp.config = None;
            self.retain_lsp_worker(None);
            self.clear_lsp_ui();
            return;
        };
        self.activate_plugins(ActivationEvent::Language(settings.language_id.clone()));
        let desired = (settings.tool.clone(), settings.language_id.clone(), root);
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

    pub(crate) fn close_lsp_document(&mut self, path: &Path) {
        if self
            .language
            .lsp
            .pending_sync
            .as_ref()
            .is_some_and(|(pending, _)| pending == path)
        {
            self.language.lsp.pending_sync = None;
        }
        if self
            .language
            .lsp
            .last_synced
            .as_ref()
            .is_some_and(|(synced, _)| synced == path)
        {
            self.language.lsp.last_synced = None;
            self.language.lsp.change_due = None;
        }
        self.language.lsp_requests.versions.remove(path);
        for worker in self.language.lsp.workers.values() {
            let _ = worker.close(path);
        }
    }

    pub(crate) fn start_lsp(&mut self) {
        let Some(path) = self.session.path.clone() else {
            self.status.text = "E: lsp requires an open file".to_owned();
            self.file.dirty = true;
            return;
        };
        let Some(root) = self.lsp_owning_root(&path) else {
            self.status.text = format!("E: lsp: outside workspace: {}", path.display());
            self.file.dirty = true;
            return;
        };
        let Some(settings) = crate::lsp_ui::lsp_settings_with_root(
            &path,
            &root,
            self.workspace.language_tooling_service.as_deref(),
        ) else {
            self.status.text = format!(
                "E: no language server for {}; run anvil doctor or configure [lsp.commands]",
                path.display()
            );
            self.file.dirty = true;
            return;
        };
        let desired = (
            settings.tool.clone(),
            settings.language_id.clone(),
            root.clone(),
        );
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
        let client = if root == self.workspace.path {
            self.workspace
                .gateway_session
                .as_mut()
                .ok_or_else(|| "primary gateway session unavailable".to_owned())
                .and_then(|session| session.connect_lsp())
        } else {
            self.workspace
                .root_gateways
                .get_mut(&root)
                .ok_or_else(|| format!("root gateway unavailable: {}", root.display()))
                .and_then(|gateway| gateway.0.connect_lsp())
        };
        let client = match client {
            Ok(client) => client,
            Err(error) => {
                self.language.lsp.status.insert(
                    desired,
                    LspStatus::Failed {
                        root: root.clone(),
                        error: format!("lsp gateway connection: {error}"),
                    },
                );
                self.status.text = format!("E: lsp gateway connection: {error}");
                self.file.dirty = true;
                return;
            }
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
            env: settings.env,
            settings: settings.settings,
        };
        match LspWorker::start(client, config, root, path.clone(), text) {
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
                let message = format!(
                    "lsp start: {error}; install {} or configure [lsp.commands]",
                    settings.label
                );
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
            self.language.completion.support.remove(&key);
        }
        self.language.lsp.pending_sync = None;
        self.clear_lsp_ui();
        self.status.text = "lsp stopped".to_owned();
        self.file.dirty = true;
    }

    fn handle_lsp_event(&mut self, key: &LspKey, event: LspEvent) {
        let active = self.language.lsp.config.as_ref() == Some(key);
        match event {
            LspEvent::Capabilities {
                inlay_hints,
                semantic_token_types,
                completion_trigger_characters,
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
                self.language
                    .completion
                    .support
                    .insert(key.clone(), completion_trigger_characters);
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
                    self.language.lsp.retry = None;
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
        self.flush_pending_lsp_sync();
        self.sync_lsp_edits();
        let effects = self.language.poll(Instant::now());
        for effect in effects {
            match effect {
                LspPollEffect::Event { key, event } => self.handle_lsp_event(&key, event),
                LspPollEffect::Retry { key } => self.retry_lsp(key),
            }
        }
    }

    pub(crate) fn retry_lsp(&mut self, key: LspKey) {
        if self.language.lsp.workers.contains_key(&key) {
            return;
        }
        if self.language.lsp.config.as_ref() == Some(&key) {
            self.start_lsp();
            return;
        }
        self.language.lsp.status.insert(
            key.clone(),
            LspStatus::Failed {
                root: key.2.clone(),
                error: "lsp crashed in the background; switch to its root to restart".to_owned(),
            },
        );
        self.language.lsp.retries.remove(&key);
        self.status.text = format!(
            "E: lsp crashed in the background: {}; switch to its root to restart",
            key.2.display()
        );
        self.file.dirty = true;
    }

    fn flush_pending_lsp_sync(&mut self) {
        let Some((path, text)) = self.language.lsp.pending_sync.take() else {
            return;
        };
        let Some(worker) = self.active_lsp_worker() else {
            return;
        };
        match worker.sync_recoverable(&path, text) {
            Ok(()) => {
                self.language.lsp_requests.record_version(&path);
                self.record_lsp_synced(&path);
            }
            Err((_, text)) => {
                self.language.lsp.pending_sync = Some((path, text));
            }
        }
    }

    fn sync_lsp_edits(&mut self) {
        let Some(path) = self.session.path.clone() else {
            self.language.lsp.change_due = None;
            return;
        };
        if self.active_lsp_worker().is_none() {
            self.language.lsp.change_due = None;
            return;
        }
        let revision = self.session.editor.revision();
        if self
            .language
            .lsp
            .last_synced
            .as_ref()
            .is_some_and(|(synced, synced_revision)| {
                synced == &path && *synced_revision == revision
            })
        {
            self.language.lsp.change_due = None;
            return;
        }
        if self.language.lsp.pending_sync.is_some() {
            return;
        }
        let now = Instant::now();
        let due = *self
            .language
            .lsp
            .change_due
            .get_or_insert(now + Duration::from_millis(150));
        if now < due {
            return;
        }
        let text = self
            .session
            .editor
            .text()
            .slice(0..self.session.editor.text().len_bytes());
        let _ = self.sync_lsp_document(&path, text);
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
        self.workspace.root_file_workers.clear();
        self.language.lsp.workers.clear();

        self.language.lsp.config = None;
        self.language.lsp.retries.clear();
        self.clear_lsp_ui();
        self.workspace.gateway = None;
        if let Some(mut session) = self.workspace.gateway_session.take() {
            session.shutdown_gracefully(std::time::Duration::from_millis(250));
        }
        match GatewaySession::start(&self.workspace.path) {
            Ok((session, client)) => {
                self.workspace.gateway_session = Some(session);
                self.workspace.gateway = Some(client);
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
mod status_tests {
    use super::*;
    use crate::workspace_roots::WorkspaceRoots;
    use anvil_editor::EditorState;
    use anvil_ipc::{GatewayClient, Listener};
    use anvil_lsp_client::{LspConfig, LspWorker};
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn capabilities_and_crash_update_the_status_snapshot() {
        let mut model = Model::new(
            EditorState::from_text(""),
            Some(PathBuf::from("/ws/main.c")),
        );
        model.workspace.path = PathBuf::from("/ws");
        let key = ("clangd".to_owned(), "c".to_owned(), PathBuf::from("/ws"));
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
                completion_trigger_characters: vec![".".to_owned(), "::".to_owned()],
                server_info: Some("clangd 18".to_owned()),
            },
        );
        assert_eq!(
            model.language.completion.support.get(&key),
            Some(&vec![".".to_owned(), "::".to_owned()])
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
    fn crash_retry_budget_exhausts_and_stops_retrying() {
        let mut model = Model::new(
            EditorState::from_text(""),
            Some(PathBuf::from("/ws/main.c")),
        );
        model.workspace.path = PathBuf::from("/ws");
        let key = ("clangd".to_owned(), "c".to_owned(), PathBuf::from("/ws"));
        model.language.lsp.status.insert(
            key.clone(),
            LspStatus::Starting {
                root: PathBuf::from("/ws"),
            },
        );

        for attempt in 1..=3 {
            model.language.lsp.config = Some(key.clone());
            model.handle_lsp_event(&key, LspEvent::Crashed("boom".to_owned()));
            assert!(matches!(
                model.language.lsp.status.get(&key),
                Some(LspStatus::Restarting { attempt: current, .. }) if *current == attempt
            ));
            assert!(model.language.lsp.retry.is_some());
        }

        model.language.lsp.config = Some(key.clone());
        model.handle_lsp_event(&key, LspEvent::Crashed("fatal".to_owned()));
        assert!(matches!(
            model.language.lsp.status.get(&key),
            Some(LspStatus::Failed { error, .. }) if error == "fatal"
        ));
        assert!(model.language.lsp.retry.is_none());
        assert!(model.status.text.contains("E: lsp crashed"));
        assert_eq!(model.language.lsp.retries.get(&key), Some(&4));
    }

    #[test]
    fn successful_capabilities_reset_crash_retry_budget() {
        let mut model = Model::new(
            EditorState::from_text(""),
            Some(PathBuf::from("/ws/main.c")),
        );
        model.workspace.path = PathBuf::from("/ws");
        let key = ("clangd".to_owned(), "c".to_owned(), PathBuf::from("/ws"));
        model.language.lsp.config = Some(key.clone());
        model.language.lsp.retries.insert(key.clone(), 3);
        model.language.lsp.retry = Some((key.clone(), Instant::now()));
        model.language.lsp.status.insert(
            key.clone(),
            LspStatus::Starting {
                root: PathBuf::from("/ws"),
            },
        );

        model.handle_lsp_event(
            &key,
            LspEvent::Capabilities {
                inlay_hints: false,
                semantic_token_types: Vec::new(),
                completion_trigger_characters: Vec::new(),
                server_info: None,
            },
        );
        assert!(!model.language.lsp.retries.contains_key(&key));
        assert!(model.language.lsp.retry.is_none());
        assert!(matches!(
            model.language.lsp.status.get(&key),
            Some(LspStatus::Running { .. })
        ));
    }

    #[test]
    fn stopping_records_a_visible_state() {
        let mut model = Model::new(
            EditorState::from_text(""),
            Some(PathBuf::from("/ws/main.c")),
        );
        model.workspace.path = PathBuf::from("/ws");
        let key = ("clangd".to_owned(), "c".to_owned(), PathBuf::from("/ws"));
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
        let key = ("clangd".to_owned(), "c".to_owned(), PathBuf::from("/ws"));
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
    fn lsp_key_is_scoped_to_the_owning_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let primary = dir.path().join("primary");
        let secondary = dir.path().join("secondary");
        std::fs::create_dir_all(&primary).expect("primary");
        std::fs::create_dir_all(&secondary).expect("secondary");
        let primary_file = primary.join("main.c");
        let secondary_file = secondary.join("main.c");
        std::fs::write(&primary_file, "int main(void) {}").expect("write");
        std::fs::write(&secondary_file, "int main(void) {}").expect("write");

        let mut model = Model::new(EditorState::from_text(""), Some(primary_file.clone()));
        model.workspace.path = primary.clone();
        model.workspace.roots = WorkspaceRoots::new(&primary).expect("primary root");
        model
            .workspace
            .roots
            .add(&secondary, true)
            .expect("secondary root");

        let first = model.lsp_key(&primary_file).expect("primary lsp key");
        let second = model.lsp_key(&secondary_file).expect("secondary lsp key");
        assert_eq!(first.2, primary);
        assert_eq!(second.2, secondary);
        assert_ne!(first, second);
    }

    #[test]
    fn stopping_one_root_leaves_other_lsp_state() {
        let mut model = Model::new(EditorState::from_text(""), None);
        let first = ("clangd".to_owned(), "c".to_owned(), PathBuf::from("/one"));
        let second = ("clangd".to_owned(), "c".to_owned(), PathBuf::from("/two"));
        for key in [first.clone(), second.clone()] {
            model.language.lsp.status.insert(
                key.clone(),
                LspStatus::Starting {
                    root: key.2.clone(),
                },
            );
            model
                .language
                .lsp
                .labels
                .insert(key.clone(), "clangd".to_owned());
            model.language.lsp.retries.insert(key, 1);
        }

        model.stop_lsp_root(Path::new("/one"));
        assert!(!model.language.lsp.status.contains_key(&first));
        assert!(!model.language.lsp.retries.contains_key(&first));
        assert!(model.language.lsp.status.contains_key(&second));
        assert!(model.language.lsp.retries.contains_key(&second));
    }

    fn fake_lsp_worker(socket: &Path, root: &Path, path: &Path) -> LspWorker {
        let client = GatewayClient::connect(socket, &[1; 32]).expect("client");
        LspWorker::start(
            client,
            LspConfig {
                tool: "clangd".to_owned(),
                args: Vec::new(),
                language_id: "c".to_owned(),
                root_markers: Vec::new(),
                initialization_options: None,
                env: BTreeMap::new(),
                settings: json!({}),
            },
            root.to_path_buf(),
            path.to_path_buf(),
            "int main(void) { return 0; }".to_owned(),
        )
        .expect("worker")
    }

    #[test]
    fn two_roots_with_same_language_keep_the_existing_worker() {
        let first = tempfile::tempdir().expect("first root");
        let second = tempfile::tempdir().expect("second root");
        let first_root = first.path().canonicalize().expect("first canonical");
        let second_root = second.path().canonicalize().expect("second canonical");
        let first_file = first_root.join("main.c");
        let second_file = second_root.join("main.c");
        std::fs::write(&first_file, "int main(void) { return 0; }").expect("first file");
        std::fs::write(&second_file, "int main(void) { return 0; }").expect("second file");
        let socket = first_root.join("gateway.sock");
        std::thread::scope(|scope| {
            let listener = Listener::bind(&socket).expect("bind");
            scope.spawn(move || {
                for _ in 0..2 {
                    let mut connection = listener.accept().expect("accept");
                    scope.spawn(move || {
                        while let Ok(Some(frame)) = connection.recv() {
                            let request: serde_json::Value =
                                serde_json::from_slice(&frame).expect("request");
                            let response = json!({
                                "jsonrpc": "2.0",
                                "id": request.get("id").cloned().unwrap_or(serde_json::Value::Null),
                                "result": {"ok": true}
                            });
                            connection
                                .send(&serde_json::to_vec(&response).expect("response"))
                                .expect("send");
                            if request["params"]["data"]["method"] == "initialize" {
                                let notification = json!({
                                    "jsonrpc": "2.0",
                                    "method": "lsp.message",
                                    "params": {"data": {
                                        "jsonrpc": "2.0",
                                        "id": 1,
                                        "result": {"capabilities": {}}
                                    }}
                                });
                                connection
                                    .send(
                                        &serde_json::to_vec(&notification)
                                            .expect("initialization notification"),
                                    )
                                    .expect("send");
                            }
                        }
                    });
                }
            });

            let first_key = ("clangd".to_owned(), "c".to_owned(), first_root.clone());
            let second_key = ("clangd".to_owned(), "c".to_owned(), second_root);
            let mut model = Model::new(EditorState::from_text(""), Some(first_file.clone()));
            model.workspace.path = first_root.clone();
            model.language.lsp.workers.insert(
                first_key.clone(),
                fake_lsp_worker(&socket, &first_root, &first_file),
            );
            model.language.lsp.workers.insert(
                second_key.clone(),
                fake_lsp_worker(&socket, &second_key.2, &second_file),
            );
            model.language.lsp.config = Some(first_key.clone());
            model.language.lsp.status.insert(
                first_key.clone(),
                LspStatus::Running {
                    root: first_root.clone(),
                    server_info: None,
                },
            );
            model.language.lsp.status.insert(
                second_key.clone(),
                LspStatus::Running {
                    root: second_key.2.clone(),
                    server_info: None,
                },
            );

            model.retain_lsp_worker(Some(&second_key));
            assert!(model.language.lsp.workers.contains_key(&first_key));
            assert!(model.language.lsp.workers.contains_key(&second_key));
            assert!(model.language.lsp.config.is_none());

            model.language.lsp.config = Some(second_key.clone());
            model.retain_lsp_worker(Some(&(
                "rust-analyzer".to_owned(),
                "rust".to_owned(),
                first_root,
            )));
            assert!(!model.language.lsp.workers.contains_key(&second_key));
            assert!(model.language.lsp.workers.contains_key(&first_key));
        });
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

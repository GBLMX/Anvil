//! LSP worker lifecycle, event polling, and diagnostic helpers.

use super::*;
use anvil_session::plugin_events::{DiagnosticsChanged, PluginEvent};

fn truncate_event_text(text: &str, max: usize) -> String {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

impl Model {
    pub(crate) fn clear_lsp_ui(&mut self) {
        self.clear_lsp_requests();
        self.cancel_completion();
        self.close_hover();
        self.close_diagnostic_peek();
        self.overlays.lsp_results.code_actions = None;
        self.close_overlay(OverlayKind::CodeActions);
        self.overlays.lsp_results.code_lens = None;
        self.overlays.lsp_results.document_symbols = None;
        self.close_overlay(OverlayKind::DocumentSymbols);
        self.overlays.lsp_results.references = None;
        self.close_overlay(OverlayKind::References);
        self.overlays.lsp_results.document_highlights = None;
        self.overlays.lsp_results.call_hierarchy = None;
        self.close_overlay(OverlayKind::CallHierarchy);
        self.overlays.lsp_results.semantic_legend.clear();
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

    pub(crate) fn sync_lsp_active(&mut self) {
        let Some(path) = self.session.path.clone() else {
            self.language.lsp.config = None;
            self.clear_lsp_ui();
            return;
        };
        let Some(settings) =
            lsp_settings_with_service(&path, self.workspace.language_tooling_service.as_deref())
        else {
            self.language.lsp.config = None;
            self.clear_lsp_ui();
            return;
        };
        self.activate_plugins(ActivationEvent::Language(settings.language_id.clone()));
        let desired = (settings.tool.clone(), settings.language_id.clone());
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
        if self.language.lsp.workers.contains_key(&desired) {
            self.language.lsp.config = Some(desired.clone());
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
                self.language.lsp.workers.insert(desired.clone(), worker);
                self.language.lsp_requests.versions.insert(path.clone(), 1);
                self.language.lsp.retries.entry(desired).or_insert(0);
                self.clear_lsp_ui();
                self.status.text = if settings.label == "jdtls" {
                    "jdtls starting (indexing workspace)".to_owned()
                } else {
                    format!("{} starting", settings.label)
                };
            }
            Err(error) => self.status.text = format!("E: lsp start: {error}"),
        }
        self.file.dirty = true;
    }

    pub(crate) fn stop_lsp(&mut self) {
        if let Some(key) = self.language.lsp.config.take() {
            self.language.lsp.workers.remove(&key);
            self.language.lsp.retries.remove(&key);
            self.overlays.lsp_results.inlay_support.remove(&key);
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
                self.overlays
                    .lsp_results
                    .inlay_support
                    .insert(key.clone(), inlay_hints);
                if active {
                    if let Some(info) = server_info {
                        self.status.text = format!("lsp: {info}");
                    }
                    self.overlays.lsp_results.inlay_hints.clear();
                    self.overlays.lsp_results.inlay_pending = None;
                    self.overlays.lsp_results.inlay_checked = None;
                    self.overlays.lsp_results.semantic_legend = semantic_token_types;
                    self.overlays.lsp_results.semantic_tokens.clear();
                    self.overlays.lsp_results.semantic_revision = None;
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
                self.plugins.supervisor.publish_event(
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
                    && !self.overlays.lsp_results.semantic_legend.is_empty()
                    && self.overlays.lsp_results.semantic_revision
                        != Some(self.session.editor.revision())
                {
                    self.request_semantic_tokens();
                }
            }
            LspEvent::Crashed(error) => {
                self.language.lsp.workers.remove(key);
                if active {
                    self.language.lsp.config = None;
                    self.clear_lsp_ui();
                    let retries = {
                        let retries = self.language.lsp.retries.entry(key.clone()).or_insert(0);
                        *retries += 1;
                        *retries
                    };
                    if retries <= 3 {
                        let delay_ms = 250u64 << (retries.saturating_sub(1).min(3));
                        self.language.lsp.retry = Some((
                            key.clone(),
                            Instant::now() + Duration::from_millis(delay_ms),
                        ));
                        self.status.text =
                            format!("lsp crashed; restarting in {delay_ms}ms ({retries}/3)");
                    } else {
                        self.status.text = format!("E: lsp crashed: {error}");
                        self.notify(
                            anvil_session::ui::NotificationSeverity::Error,
                            "lsp",
                            format!("crashed: {error}"),
                        );
                    }
                }
            }
            LspEvent::Response { id, result, error } => {
                self.handle_lsp_response(id, result, error);
            }
            LspEvent::StartupError(error) => {
                self.language.lsp.workers.remove(key);
                if active {
                    self.language.lsp.config = None;
                    self.clear_lsp_ui();
                    self.status.text = format!("E: lsp startup failed: {error}");
                    self.notify(
                        anvil_session::ui::NotificationSeverity::Error,
                        "lsp",
                        format!("startup failed: {error}"),
                    );
                }
            }
            LspEvent::Error(error) => {
                if active {
                    self.status.text = format!("E: lsp: {error}");
                    self.notify(
                        anvil_session::ui::NotificationSeverity::Error,
                        "lsp",
                        error.clone(),
                    );
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
        let keys: Vec<(String, String)> = self.language.lsp.workers.keys().cloned().collect();
        for key in keys {
            let events = self
                .language
                .lsp
                .workers
                .get(&key)
                .map(LspWorker::poll)
                .unwrap_or_default();
            for event in events {
                self.handle_lsp_event(&key, event);
            }
        }
    }

    pub(crate) fn poll_lsp_retry(&mut self) {
        let Some((key, deadline)) = self.language.lsp.retry.clone() else {
            return;
        };
        if Instant::now() < deadline {
            return;
        }
        self.language.lsp.retry = None;
        if !self.language.lsp.workers.contains_key(&key) {
            self.start_lsp();
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
        let message = diagnostic
            .message
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
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
        let line = self.session.editor.text().slice(line_start..line_end);
        diagnostics
            .iter()
            .filter_map(|diagnostic| {
                if diagnostic.range.start.line as usize > line_number
                    || (diagnostic.range.end.line as usize) < line_number
                {
                    return None;
                }
                let start_units = if diagnostic.range.start.line as usize == line_number {
                    diagnostic.range.start.character as usize
                } else {
                    0
                };
                let end_units = if diagnostic.range.end.line as usize == line_number {
                    diagnostic.range.end.character as usize
                } else {
                    line.chars().map(char::len_utf16).sum()
                };
                let start_byte = utf16_to_byte(&line, start_units);
                let end_byte = utf16_to_byte(&line, end_units)
                    .max(start_byte + 1)
                    .min(line.len());
                let start = display_column(&line[..start_byte]);
                let end = display_column(&line[..end_byte]).max(start + 1);
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

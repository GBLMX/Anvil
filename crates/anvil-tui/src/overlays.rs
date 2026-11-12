//! Notification, progress, problem, and diagnostic-peek state transitions.

use super::*;
use anvil_session::ui::{DiagnosticPeek, NotificationSeverity, PeekDiagnostic, Problem};
use std::time::{SystemTime, UNIX_EPOCH};

impl Model {
    pub(crate) fn notify(
        &mut self,
        severity: NotificationSeverity,
        source: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.ui
            .notifications
            .push(severity, source, message, now_millis());
        self.dirty = true;
    }

    pub(crate) fn finish_busy_progress(&mut self, error: Option<&str>) {
        if let Some(id) = self.busy_progress.take() {
            self.ui.progress.finish(id);
            if let Some(error) = error {
                self.notify(NotificationSeverity::Error, "file", error);
            }
            self.dirty = true;
        }
    }

    pub(crate) fn rebuild_problems(&mut self) {
        let mut problems = Vec::new();
        for (path, diagnostics) in &self.language.diagnostics.by_path {
            for diagnostic in diagnostics {
                problems.push(Problem {
                    path: path.clone(),
                    line: diagnostic.range.start.line,
                    character: diagnostic.range.start.character,
                    severity: diagnostic.severity,
                    source: diagnostic.source.clone(),
                    code: diagnostic_code(diagnostic),
                    message: diagnostic.message.clone(),
                });
            }
        }
        problems.extend(self.runtime.task_problems.iter().cloned());
        self.ui.problems.replace(problems);
        self.dirty = true;
    }

    pub(crate) fn toggle_problems(&mut self) {
        self.ui.problems.visible = !self.ui.problems.visible;
        if self.ui.problems.visible {
            self.rebuild_problems();
            self.status = format!("problems: {}", self.ui.problems.items.len());
        }
        self.dirty = true;
    }

    pub(crate) fn jump_problem(&mut self) {
        let Some(problem) = self.ui.problems.selected().cloned() else {
            self.status = "no problems".to_owned();
            self.dirty = true;
            return;
        };
        if self.path.as_deref() != Some(problem.path.as_path())
            && let Err(error) = self.open_path(&problem.path)
        {
            self.status = format!("E: {error}");
            self.dirty = true;
            return;
        }
        self.cursor = self.offset_at_utf16(problem.line as usize, problem.character as usize);
        self.ui.focus.tree = false;
        self.ui.focus.split = false;
        self.ui.problems.visible = false;
        self.ui.diagnostic_peek = None;
        self.status = format!(
            "{}:{}: {}",
            problem.path.display(),
            problem.line + 1,
            problem.message
        );
        self.dirty = true;
    }

    pub(crate) fn open_diagnostic_peek(
        &mut self,
        path: PathBuf,
        line: u32,
        character: u32,
    ) -> bool {
        let Some(diagnostics) = self.language.diagnostics.by_path.get(&path) else {
            return false;
        };
        let diagnostics: Vec<PeekDiagnostic> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.range.start.line == line)
            .map(peek_diagnostic)
            .collect();
        if diagnostics.is_empty() {
            return false;
        }
        let line_count = diagnostics
            .iter()
            .map(|diagnostic| 1 + diagnostic.message.lines().count() + diagnostic.related.len())
            .sum();
        self.ui.diagnostic_peek = Some(DiagnosticPeek {
            path,
            revision: self.editor.revision(),
            line,
            character,
            focus: self.ui.focus,
            diagnostics,
            selected: 0,
            line_count,
            scroll: 0,
        });
        self.dirty = true;
        true
    }

    pub(crate) fn close_diagnostic_peek(&mut self) {
        if let Some(peek) = self.ui.diagnostic_peek.take() {
            self.ui.focus = peek.focus;
            self.dirty = true;
        }
    }

    pub(crate) fn scroll_diagnostic_peek(&mut self, delta: isize) {
        let visible = usize::from(self.viewport_height.saturating_sub(4)).clamp(1, 10);
        if let Some(peek) = self.ui.diagnostic_peek.as_mut() {
            peek.scroll = peek
                .scroll
                .saturating_add_signed(delta)
                .min(peek.line_count.saturating_sub(visible));
            self.dirty = true;
        }
    }

    pub(crate) fn prune_diagnostic_peek(&mut self) {
        let stale = self.ui.diagnostic_peek.as_ref().is_some_and(|peek| {
            self.path.as_deref() != Some(peek.path.as_path())
                || peek.revision != self.editor.revision()
        });
        if stale {
            self.ui.diagnostic_peek = None;
            self.dirty = true;
        }
    }
}

fn peek_diagnostic(diagnostic: &lsp::Diagnostic) -> PeekDiagnostic {
    PeekDiagnostic {
        severity: diagnostic.severity,
        source: diagnostic.source.clone(),
        code: diagnostic_code(diagnostic),
        message: diagnostic.message.clone(),
        related: diagnostic
            .extra
            .get("relatedInformation")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let message = item.get("message")?.as_str()?;
                        let uri = item.pointer("/location/uri")?.as_str()?;
                        let line = item
                            .pointer("/location/range/start/line")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0)
                            + 1;
                        let path = lsp::uri_to_path(uri)?;
                        Some(format!("{message} ({}:{line})", path.display()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn diagnostic_code(diagnostic: &lsp::Diagnostic) -> Option<String> {
    diagnostic.extra.get("code").and_then(|code| {
        code.as_str()
            .map(str::to_owned)
            .or_else(|| code.as_i64().map(|code| code.to_string()))
    })
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

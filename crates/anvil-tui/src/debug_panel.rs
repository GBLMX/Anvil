//! Debug adapter panels for threads, stack frames, variables, and watches.

use super::*;
use crate::dap::{DapCancel, DapClient, DapWorker};
use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Clear, Paragraph},
};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DebugPanelKind {
    #[default]
    Threads,
    Stack,
    Variables,
    Watch,
}

impl DebugPanelKind {
    fn next(self) -> Self {
        match self {
            Self::Threads => Self::Stack,
            Self::Stack => Self::Variables,
            Self::Variables => Self::Watch,
            Self::Watch => Self::Threads,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Threads => "Threads",
            Self::Stack => "Stack",
            Self::Variables => "Variables",
            Self::Watch => "Watch",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DebugThread {
    pub(crate) id: i64,
    pub(crate) name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct DebugFrame {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) path: Option<PathBuf>,
    pub(crate) line: u32,
    pub(crate) column: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct DebugScope {
    pub(crate) name: String,
    pub(crate) variables_reference: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct DebugVariable {
    pub(crate) name: String,
    pub(crate) value: String,
    pub(crate) variables_reference: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct DebugWatch {
    pub(crate) expression: String,
    pub(crate) value: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DebugPanelState {
    pub(crate) kind: DebugPanelKind,
    pub(crate) threads: Vec<DebugThread>,
    pub(crate) frames: Vec<DebugFrame>,
    pub(crate) scopes: Vec<DebugScope>,
    pub(crate) variables: Vec<DebugVariable>,
    pub(crate) watches: Vec<DebugWatch>,
    pub(crate) selected_thread: usize,
    pub(crate) selected_frame: usize,
    pub(crate) selected_scope: usize,
    pub(crate) selected_variable: usize,
    pub(crate) selected_watch: usize,
    pub(crate) pending: bool,
}

pub(crate) struct DebugSnapshot {
    pub(crate) threads: Vec<DebugThread>,
    pub(crate) frames: Vec<DebugFrame>,
    pub(crate) scopes: Vec<DebugScope>,
    pub(crate) variables: Vec<DebugVariable>,
    pub(crate) watches: Vec<DebugWatch>,
}

/// How long a panel refresh may keep a request in flight before the adapter's
/// process group is killed.
const DAP_REFRESH_TIMEOUT: Duration = Duration::from_secs(5);

impl DebugPanelState {
    fn selected_len(&self) -> usize {
        match self.kind {
            DebugPanelKind::Threads => self.threads.len(),
            DebugPanelKind::Stack => self.frames.len(),
            DebugPanelKind::Variables => self.variables.len(),
            DebugPanelKind::Watch => self.watches.len(),
        }
    }

    fn selected_mut(&mut self) -> &mut usize {
        match self.kind {
            DebugPanelKind::Threads => &mut self.selected_thread,
            DebugPanelKind::Stack => &mut self.selected_frame,
            DebugPanelKind::Variables => &mut self.selected_variable,
            DebugPanelKind::Watch => &mut self.selected_watch,
        }
    }
}

impl Model {
    pub(crate) fn toggle_debug_panel(&mut self) {
        if self.overlays.debug_panel.is_some() {
            self.close_debug_panel();
            self.status.text = "debug panel closed".to_owned();
            self.file.dirty = true;
            return;
        }
        self.overlays.debug_panel = Some(DebugPanelState::default());
        self.push_overlay(OverlayKind::DebugPanel);
        self.status.text = "debug panel".to_owned();
        self.file.dirty = true;
        self.request_debug_refresh();
    }

    pub(crate) fn set_debug_panel(&mut self, name: &str) {
        let Some(panel) = self.overlays.debug_panel.as_mut() else {
            self.status.text = "E: debug panel is closed; use :debug".to_owned();
            self.file.dirty = true;
            return;
        };
        let Some(kind) = (match name {
            "threads" => Some(DebugPanelKind::Threads),
            "stack" => Some(DebugPanelKind::Stack),
            "variables" => Some(DebugPanelKind::Variables),
            "watch" => Some(DebugPanelKind::Watch),
            _ => None,
        }) else {
            self.status.text = "E: usage: :debug-panel <threads|stack|variables|watch>".to_owned();
            self.file.dirty = true;
            return;
        };
        panel.kind = kind;
        self.status.text = format!("debug panel: {}", kind.name());
        self.file.dirty = true;
        self.request_debug_refresh();
    }

    pub(crate) fn cycle_debug_panel(&mut self) {
        let Some(panel) = self.overlays.debug_panel.as_mut() else {
            return;
        };
        panel.kind = panel.kind.next();
        self.status.text = format!("debug panel: {}", panel.kind.name());
        self.file.dirty = true;
    }

    pub(crate) fn move_debug_selection(&mut self, delta: isize) {
        let Some(panel) = self.overlays.debug_panel.as_mut() else {
            return;
        };
        let len = panel.selected_len();
        if len == 0 {
            return;
        }
        let selected = panel.selected_mut();
        *selected = if delta < 0 {
            selected.saturating_sub(delta.unsigned_abs())
        } else {
            (*selected + delta.unsigned_abs()).min(len - 1)
        };
        self.file.dirty = true;
    }

    pub(crate) fn close_debug_panel(&mut self) {
        self.overlays.debug_panel = None;
        self.close_overlay(OverlayKind::DebugPanel);
        self.file.dirty = true;
    }

    pub(crate) fn add_debug_watch(&mut self, expression: &str) {
        let expression = expression.trim();
        if expression.is_empty() {
            self.status.text = "E: usage: :debug-watch <expression>".to_owned();
            self.file.dirty = true;
            return;
        }
        let Some(panel) = self.overlays.debug_panel.as_mut() else {
            self.status.text = "E: debug panel is closed; use :debug".to_owned();
            self.file.dirty = true;
            return;
        };
        if !panel
            .watches
            .iter()
            .any(|watch| watch.expression == expression)
        {
            panel.watches.push(DebugWatch {
                expression: expression.to_owned(),
                value: "not evaluated".to_owned(),
            });
        }
        panel.kind = DebugPanelKind::Watch;
        self.status.text = format!("watch: {expression}");
        self.file.dirty = true;
        self.request_debug_refresh();
    }

    pub(crate) fn clear_debug_watches(&mut self) {
        let Some(panel) = self.overlays.debug_panel.as_mut() else {
            return;
        };
        panel.watches.clear();
        panel.selected_watch = 0;
        self.status.text = "watches cleared".to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn request_debug_refresh(&mut self) {
        if self.language.dap.request.is_some() {
            return;
        }
        let Some(client) = self.language.dap.client.take() else {
            self.status.text = "E: no debug adapter; use :dap-attach".to_owned();
            self.file.dirty = true;
            return;
        };
        let Some(panel) = self.overlays.debug_panel.as_ref() else {
            self.language.dap.client = Some(client);
            return;
        };
        let selection = DebugSelection {
            thread: panel.selected_thread,
            frame: panel.selected_frame,
            scope: panel.selected_scope,
            watches: panel
                .watches
                .iter()
                .map(|watch| watch.expression.clone())
                .collect(),
        };
        let generation = self.language.dap.generation;
        let cancel = DapCancel::new();
        cancel.set_pid(client.pid());
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("anvil-dap-refresh".to_owned())
            .spawn(move || {
                let mut client = client;
                let result = snapshot(&mut client, &selection).map_err(|error| error.to_string());
                let _ = sender.send((generation, client, result));
            });
        match worker {
            Ok(handle) => {
                self.language.dap.request_worker = Some(DapWorker::new(cancel, handle));
                self.language.dap.request = Some(receiver);
                self.language.dap.request_started = Some(Instant::now());
                if let Some(panel) = self.overlays.debug_panel.as_mut() {
                    panel.pending = true;
                }
                self.status.text = "debug refresh…".to_owned();
            }
            Err(error) => {
                self.status.text = format!("E: dap refresh worker: {error}");
            }
        }
        self.file.dirty = true;
    }

    pub(crate) fn poll_debug_request(&mut self) {
        let received = self
            .language
            .dap
            .request
            .as_ref()
            .map(std::sync::mpsc::Receiver::try_recv);
        let Some(received) = received else {
            return;
        };
        match received {
            Ok((generation, client, result)) => {
                let worker = self.language.dap.request_worker.take();
                self.language.dap.request = None;
                self.language.dap.request_started = None;
                if let Some(worker) = worker {
                    worker.finish();
                }
                if generation != self.language.dap.generation {
                    // Result from an adapter this session already detached from;
                    // dropping the client kills its process group and the current
                    // client (if any) is left untouched.
                    return;
                }
                self.language.dap.client = Some(client);
                match result {
                    Ok(snapshot) => {
                        if let Some(panel) = self.overlays.debug_panel.as_mut() {
                            panel.apply(snapshot);
                            panel.pending = false;
                        }
                        self.status.text = "debug state refreshed".to_owned();
                    }
                    Err(error) => {
                        if let Some(panel) = self.overlays.debug_panel.as_mut() {
                            panel.pending = false;
                        }
                        self.status.text = format!("E: debug refresh: {error}");
                    }
                }
                self.file.dirty = true;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                let worker = self.language.dap.request_worker.take();
                self.language.dap.request = None;
                self.language.dap.request_started = None;
                drop(worker);
                if let Some(panel) = self.overlays.debug_panel.as_mut() {
                    panel.pending = false;
                }
                self.status.text = "E: debug refresh worker stopped".to_owned();
                self.file.dirty = true;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if self
                    .language
                    .dap
                    .request_started
                    .is_some_and(|started| started.elapsed() >= DAP_REFRESH_TIMEOUT)
                {
                    // Dropping the worker kills the hung adapter's process group
                    // and joins the thread before reporting the timeout.
                    let worker = self.language.dap.request_worker.take();
                    self.language.dap.request = None;
                    self.language.dap.request_started = None;
                    drop(worker);
                    if let Some(panel) = self.overlays.debug_panel.as_mut() {
                        panel.pending = false;
                    }
                    self.status.text = "E: debug refresh timed out; adapter stopped".to_owned();
                    self.file.dirty = true;
                }
            }
        }
    }
}

impl DebugPanelState {
    fn apply(&mut self, snapshot: DebugSnapshot) {
        self.threads = snapshot.threads;
        self.frames = snapshot.frames;
        self.scopes = snapshot.scopes;
        self.variables = snapshot.variables;
        self.watches = snapshot.watches;
        self.selected_thread = self
            .selected_thread
            .min(self.threads.len().saturating_sub(1));
        self.selected_frame = self.selected_frame.min(self.frames.len().saturating_sub(1));
        self.selected_scope = self.selected_scope.min(self.scopes.len().saturating_sub(1));
        self.selected_variable = self
            .selected_variable
            .min(self.variables.len().saturating_sub(1));
        self.selected_watch = self
            .selected_watch
            .min(self.watches.len().saturating_sub(1));
    }
}

struct DebugSelection {
    thread: usize,
    frame: usize,
    scope: usize,
    watches: Vec<String>,
}

fn snapshot(
    client: &mut DapClient,
    selection: &DebugSelection,
) -> Result<DebugSnapshot, crate::dap::DapError> {
    let threads = request_body(client, "threads", json!({}))?
        .get("threads")
        .and_then(Value::as_array)
        .map(|threads| {
            threads
                .iter()
                .filter_map(|thread| {
                    Some(DebugThread {
                        id: thread.get("id")?.as_i64()?,
                        name: thread
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("thread")
                            .to_owned(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let Some(thread) = threads.get(selection.thread.min(threads.len().saturating_sub(1))) else {
        return Ok(DebugSnapshot {
            threads,
            frames: Vec::new(),
            scopes: Vec::new(),
            variables: Vec::new(),
            watches: watch_values(client, selection.watches.as_slice(), None)?,
        });
    };
    let frames = request_body(client, "stackTrace", json!({"threadId": thread.id}))?
        .get("stackFrames")
        .and_then(Value::as_array)
        .map(|frames| {
            frames
                .iter()
                .filter_map(|frame| {
                    let source = frame.get("source");
                    Some(DebugFrame {
                        id: frame.get("id")?.as_i64()?,
                        name: frame
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("frame")
                            .to_owned(),
                        path: source
                            .and_then(|source| source.get("path"))
                            .and_then(Value::as_str)
                            .map(PathBuf::from),
                        line: frame.get("line").and_then(Value::as_u64).unwrap_or(0) as u32,
                        column: frame.get("column").and_then(Value::as_u64).unwrap_or(0) as u32,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let Some(frame) = frames.get(selection.frame.min(frames.len().saturating_sub(1))) else {
        return Ok(DebugSnapshot {
            threads,
            frames,
            scopes: Vec::new(),
            variables: Vec::new(),
            watches: watch_values(client, selection.watches.as_slice(), None)?,
        });
    };
    let frame_id = frame.id;
    let scopes = request_body(client, "scopes", json!({"frameId": frame_id}))?
        .get("scopes")
        .and_then(Value::as_array)
        .map(|scopes| {
            scopes
                .iter()
                .filter_map(|scope| {
                    Some(DebugScope {
                        name: scope
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("scope")
                            .to_owned(),
                        variables_reference: scope.get("variablesReference")?.as_i64()?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let variables = scopes
        .get(selection.scope.min(scopes.len().saturating_sub(1)))
        .map(|scope| {
            request_body(
                client,
                "variables",
                json!({"variablesReference": scope.variables_reference}),
            )
            .ok()
            .and_then(|body| body.get("variables").and_then(Value::as_array).cloned())
            .map(|variables| {
                variables
                    .iter()
                    .filter_map(|variable| {
                        Some(DebugVariable {
                            name: variable.get("name")?.as_str()?.to_owned(),
                            value: variable
                                .get("value")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_owned(),
                            variables_reference: variable
                                .get("variablesReference")
                                .and_then(Value::as_i64)
                                .unwrap_or(0),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
        })
        .unwrap_or_default();
    Ok(DebugSnapshot {
        threads,
        frames,
        scopes,
        variables,
        watches: watch_values(client, selection.watches.as_slice(), Some(frame_id))?,
    })
}

fn watch_values(
    client: &mut DapClient,
    expressions: &[String],
    frame_id: Option<i64>,
) -> Result<Vec<DebugWatch>, crate::dap::DapError> {
    expressions
        .iter()
        .map(|expression| {
            let mut arguments = json!({
                "expression": expression,
                "context": "watch",
            });
            if let Some(frame_id) = frame_id {
                arguments["frameId"] = json!(frame_id);
            }
            let value = request_body(client, "evaluate", arguments)
                .ok()
                .and_then(|body| {
                    body.get("result")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "unavailable".to_owned());
            Ok(DebugWatch {
                expression: expression.clone(),
                value,
            })
        })
        .collect()
}

fn request_body(
    client: &mut DapClient,
    command: &str,
    arguments: Value,
) -> Result<Value, crate::dap::DapError> {
    let request = client.request(command, arguments)?;
    let response = client.wait_response(request)?;
    Ok(response.get("body").cloned().unwrap_or(Value::Null))
}

pub(crate) fn render(model: &Model, frame: &mut Frame, area: Rect) {
    let Some(panel) = &model.overlays.debug_panel else {
        return;
    };
    let width = (area.width * 2 / 5)
        .max(34)
        .min(area.width.saturating_sub(2));
    let height = (panel.selected_len() as u16 + 4)
        .clamp(9, 19)
        .min(area.height.saturating_sub(2));
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width + 1),
        y: area.y + 1,
        width,
        height,
    };
    frame.render_widget(Clear, popup);
    let content_width = width.saturating_sub(4) as usize;
    let mut lines = Vec::new();
    let mut selected_line = 0;
    match panel.kind {
        DebugPanelKind::Threads => {
            for (index, thread) in panel.threads.iter().enumerate() {
                let text = format!("{} {}", thread.id, thread.name);
                push_item(
                    &mut lines,
                    &mut selected_line,
                    index,
                    panel.selected_thread,
                    text,
                    content_width,
                );
            }
        }
        DebugPanelKind::Stack => {
            for (index, item) in panel.frames.iter().enumerate() {
                let location = item
                    .path
                    .as_ref()
                    .map(|path| {
                        format!(
                            "{}:{}:{}",
                            path.file_name()
                                .map(|name| name.to_string_lossy())
                                .unwrap_or_default(),
                            item.line,
                            item.column
                        )
                    })
                    .unwrap_or_else(|| format!("{}:{}:{}", item.name, item.line, item.column));
                let text = format!("#{index} {} {location}", item.name);
                push_item(
                    &mut lines,
                    &mut selected_line,
                    index,
                    panel.selected_frame,
                    text,
                    content_width,
                );
            }
        }
        DebugPanelKind::Variables => {
            for (index, variable) in panel.variables.iter().enumerate() {
                let expandable = if variable.variables_reference > 0 {
                    "▸ "
                } else {
                    ""
                };
                let text = format!("{expandable}{} = {}", variable.name, variable.value);
                push_item(
                    &mut lines,
                    &mut selected_line,
                    index,
                    panel.selected_variable,
                    text,
                    content_width,
                );
            }
        }
        DebugPanelKind::Watch => {
            for (index, watch) in panel.watches.iter().enumerate() {
                let text = format!("{} = {}", watch.expression, watch.value);
                push_item(
                    &mut lines,
                    &mut selected_line,
                    index,
                    panel.selected_watch,
                    text,
                    content_width,
                );
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::styled(
            if panel.pending { "loading…" } else { "empty" },
            theme::panel().fg(theme::muted()),
        ));
    }
    let visible = height.saturating_sub(3) as usize;
    let start = selected_line.saturating_sub(visible.saturating_sub(1));
    let mut visible_lines = lines
        .into_iter()
        .skip(start)
        .take(visible)
        .collect::<Vec<_>>();
    visible_lines.push(Line::styled(
        "r refresh · Tab panel · Esc close",
        theme::panel().fg(theme::muted()),
    ));
    frame.render_widget(
        Paragraph::new(visible_lines).style(theme::panel()).block(
            crate::render::focused_panel_block(format!(
                "Debug · {}{}",
                panel.kind.name(),
                panel
                    .scopes
                    .get(panel.selected_scope)
                    .map(|scope| format!(" · {}", scope.name))
                    .unwrap_or_default()
            )),
        ),
        popup,
    );
}

fn push_item(
    lines: &mut Vec<Line<'static>>,
    selected_line: &mut usize,
    index: usize,
    selected: usize,
    text: String,
    width: usize,
) {
    let text = crate::status_line::middle_ellipsis(&text, width);
    if index == selected {
        *selected_line = lines.len();
        lines.push(Line::styled(text, theme::selected()));
    } else {
        lines.push(Line::styled(text, theme::panel()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn snapshot_reads_threads_stack_scopes_variables_and_watches() {
        let directory = std::env::temp_dir().join(format!(
            "anvil-debug-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&directory).expect("create temp dir");
        let adapter = directory.join("adapter.py");
        std::fs::write(
            &adapter,
            r#"import json, sys
bodies = {
    "threads": {"threads": [{"id": 1, "name": "main"}]},
    "stackTrace": {"stackFrames": [{"id": 10, "name": "run", "source": {"path": "/tmp/main.rs"}, "line": 7, "column": 3}]},
    "scopes": {"scopes": [{"name": "Locals", "variablesReference": 100}]},
    "variables": {"variables": [{"name": "x", "value": "42", "variablesReference": 0}]},
    "evaluate": {"result": "3"},
}
while True:
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            sys.exit(0)
        if line in (b"\r\n", b"\n"):
            break
        key, value = line.decode().split(":", 1)
        headers[key.lower()] = value.strip()
    body = sys.stdin.buffer.read(int(headers["content-length"]))
    request = json.loads(body)
    response = {
        "seq": 1,
        "type": "response",
        "request_seq": request["seq"],
        "success": True,
        "command": request["command"],
        "body": bodies.get(request["command"], {}),
    }
    data = json.dumps(response).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(data)}\r\n\r\n".encode() + data)
    sys.stdout.buffer.flush()
"#,
        )
        .expect("write adapter");
        let mut client = DapClient::spawn(
            Path::new("python3"),
            &[adapter.display().to_string()],
            &directory,
        )
        .expect("spawn adapter");
        let request = client.initialize().expect("initialize");
        client.wait_response(request).expect("initialize response");
        let snapshot = snapshot(
            &mut client,
            &DebugSelection {
                thread: 0,
                frame: 0,
                scope: 0,
                watches: vec!["count".to_owned()],
            },
        )
        .expect("snapshot");
        assert_eq!(snapshot.threads[0].name, "main");
        assert_eq!(snapshot.frames[0].line, 7);
        assert_eq!(snapshot.scopes[0].name, "Locals");
        assert_eq!(snapshot.variables[0].name, "x");
        assert_eq!(snapshot.variables[0].value, "42");
        assert_eq!(snapshot.watches[0].value, "3");
        client.kill();
        let _ = std::fs::remove_dir_all(directory);
    }
}

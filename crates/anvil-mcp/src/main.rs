//! Thin MCP stdio adapter over the Anvil control socket.
//!
//! This binary never reads or writes workspace files. Every tool call is a
//! typed `anvil-control` request executed by the running TUI actor.

use anvil_control::{
    COMMANDS, Capability, Client, CommandPolicyError, EditMode, EditRange, Error as ControlError,
    QueryOptions, validate_remote_command,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    process,
    time::Duration,
};

const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
const LEGACY_PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];
const PROTOCOL_VERSION_META: &str = "io.modelcontextprotocol/protocolVersion";
const SERVER_INFO_META: &str = "io.modelcontextprotocol/serverInfo";
const CACHE_TTL_MS: u64 = 60_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ProtocolEra {
    #[default]
    None,
    Legacy,
    Modern,
}

#[derive(Default)]
struct ServerState {
    era: ProtocolEra,
}

#[derive(Debug)]
struct McpError {
    code: i32,
    message: String,
    data: Option<Value>,
}

impl McpError {
    fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(-32602, message)
    }

    fn unsupported_version(requested: &str, supported: &[&str]) -> Self {
        Self {
            code: -32022,
            message: format!("Unsupported protocol version: {requested}"),
            data: Some(json!({
                "supported": supported,
                "requested": requested,
                "supportedVersions": supported,
                "requestedVersion": requested,
            })),
        }
    }
}

#[derive(Debug)]
enum ToolFailure {
    Params(String),
    Execution(String),
}

impl ToolFailure {
    fn params(message: impl Into<String>) -> Self {
        Self::Params(message.into())
    }

    fn execution(message: impl Into<String>) -> Self {
        Self::Execution(message.into())
    }

    #[cfg(test)]
    fn message(&self) -> &str {
        match self {
            Self::Params(message) | Self::Execution(message) => message,
        }
    }
}

impl From<String> for ToolFailure {
    fn from(message: String) -> Self {
        Self::Execution(message)
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("anvil-mcp: {error}");
        process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut state = ServerState::default();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_line(&line, &mut state) {
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn handle_line(line: &str, state: &mut ServerState) -> Option<Value> {
    let request: Value = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => {
            return Some(error_response(
                Value::Null,
                -32700,
                &format!("parse error: {error}"),
            ));
        }
    };
    let Some(object) = request.as_object() else {
        return Some(error_response(
            Value::Null,
            -32600,
            "invalid request: expected an object",
        ));
    };
    let id = object.get("id").cloned();
    let has_id = id.is_some();
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return has_id.then(|| {
            error_response(
                id.clone().unwrap_or(Value::Null),
                -32600,
                "invalid request: jsonrpc must be 2.0",
            )
        });
    }
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return has_id.then(|| {
            error_response(
                id.clone().unwrap_or(Value::Null),
                -32600,
                "invalid request: method is required",
            )
        });
    };
    if !has_id {
        return None;
    }
    let id = id.unwrap_or(Value::Null);
    if !(id.is_string() || id.is_number() || id.is_null()) {
        return Some(error_response(
            Value::Null,
            -32600,
            "invalid request: id must be a string, number, or null",
        ));
    }
    let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
    match dispatch(method, &params, state) {
        Ok(result) => {
            let modern = state.era == ProtocolEra::Modern || method == "server/discover";
            let result = if modern {
                modernize_result(result)
            } else {
                result
            };
            Some(success_response(id, result))
        }
        Err(error) => Some(error_response_with_data(
            id,
            error.code,
            &error.message,
            error.data,
        )),
    }
}

fn dispatch(method: &str, params: &Value, state: &mut ServerState) -> Result<Value, McpError> {
    if method == "server/discover" {
        if state.era == ProtocolEra::Legacy {
            return Err(McpError::new(
                -32600,
                "server/discover cannot be used after initialize",
            ));
        }
        return Ok(discover_result());
    }
    if method == "initialize" {
        return initialize(params, state);
    }

    if let Some(version) = requested_protocol_version(params) {
        if version != MODERN_PROTOCOL_VERSION {
            return Err(McpError::unsupported_version(
                version,
                &[MODERN_PROTOCOL_VERSION],
            ));
        }
        if state.era == ProtocolEra::Legacy {
            return Err(McpError::new(
                -32600,
                "cannot mix legacy and modern MCP semantics",
            ));
        }
        state.era = ProtocolEra::Modern;
    } else {
        match state.era {
            ProtocolEra::Legacy => {}
            ProtocolEra::Modern => {
                return Err(McpError::new(
                    -32600,
                    format!("modern requests require {PROTOCOL_VERSION_META} in params._meta"),
                ));
            }
            ProtocolEra::None => {
                return Err(McpError::new(
                    -32600,
                    "protocol era not selected; call initialize or send modern _meta",
                ));
            }
        }
    }

    match method {
        "ping" => {
            if state.era == ProtocolEra::Modern {
                Err(McpError::new(-32601, "unknown method: ping"))
            } else {
                Ok(json!({}))
            }
        }
        "tools/list" => {
            let mut result = json!({"tools": tools_list()});
            if state.era == ProtocolEra::Modern {
                result["ttlMs"] = json!(CACHE_TTL_MS);
                result["cacheScope"] = json!("private");
            }
            Ok(result)
        }
        "tools/call" => tools_call(params),
        _ => Err(McpError::new(-32601, format!("unknown method: {method}"))),
    }
}

fn initialize(params: &Value, state: &mut ServerState) -> Result<Value, McpError> {
    if state.era != ProtocolEra::None {
        return Err(McpError::new(
            -32600,
            "initialize cannot be used after a protocol era is selected",
        ));
    }
    let version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::invalid_params("initialize requires protocolVersion"))?;
    if !LEGACY_PROTOCOL_VERSIONS.contains(&version) {
        return Err(McpError::unsupported_version(
            version,
            LEGACY_PROTOCOL_VERSIONS,
        ));
    }
    state.era = ProtocolEra::Legacy;
    Ok(json!({
        "protocolVersion": version,
        "capabilities": {"tools": {}},
        "serverInfo": server_info(),
    }))
}

fn requested_protocol_version(params: &Value) -> Option<&str> {
    params
        .get("_meta")
        .and_then(|meta| meta.get(PROTOCOL_VERSION_META))
        .and_then(Value::as_str)
}

fn discover_result() -> Value {
    json!({
        "supportedVersions": [MODERN_PROTOCOL_VERSION],
        "capabilities": {"tools": {}},
        "instructions": "Anvil editor control tools. Workspace boundaries and admin policy are enforced by the TUI actor.",
        "ttlMs": CACHE_TTL_MS,
        "cacheScope": "private",
    })
}

fn modernize_result(mut result: Value) -> Value {
    if let Some(object) = result.as_object_mut() {
        object.insert("resultType".to_owned(), json!("complete"));
        let meta = object
            .entry("_meta".to_owned())
            .or_insert_with(|| json!({}));
        if let Some(meta) = meta.as_object_mut() {
            meta.insert(SERVER_INFO_META.to_owned(), server_info());
        }
    }
    result
}

fn server_info() -> Value {
    json!({
        "name": "anvil-mcp",
        "version": env!("CARGO_PKG_VERSION"),
    })
}

fn tools_call(params: &Value) -> Result<Value, McpError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::invalid_params("tools/call requires a tool name"))?;
    if !TOOLS
        .iter()
        .any(|tool| tool.name == name && tool.capability.is_remote_allowed())
    {
        return Err(McpError::invalid_params(format!("unknown tool: {name}")));
    }
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match call_tool(name, &arguments) {
        Ok(value) => Ok(json!({
            "content": [{"type": "text", "text": value.to_string()}],
            "structuredContent": value,
            "isError": false,
        })),
        Err(ToolFailure::Params(message)) => Err(McpError::invalid_params(message)),
        Err(ToolFailure::Execution(message)) => Ok(json!({
            "content": [{"type": "text", "text": message}],
            "isError": true,
        })),
    }
}

fn call_tool(name: &str, arguments: &Value) -> Result<Value, ToolFailure> {
    match name {
        "anvil_status" => to_value(client()?.status().map_err(control_error)?),
        "anvil_open" => {
            let path = string_arg(arguments, "path")?;
            let line = usize_arg(arguments, "line").unwrap_or(1);
            let column = usize_arg(arguments, "column").unwrap_or(1);
            to_value(
                client()?
                    .open(&PathBuf::from(path), line, column)
                    .map_err(control_error)?,
            )
        }
        "anvil_query" => {
            let options = QueryOptions {
                start_line: usize_arg(arguments, "start_line"),
                end_line: usize_arg(arguments, "end_line"),
                include_text: arguments
                    .get("include_text")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            };
            to_value(client()?.query_with(options).map_err(control_error)?)
        }
        "anvil_edit" => edit_tool(arguments),
        "anvil_save" => {
            let path = arguments
                .get("path")
                .and_then(Value::as_str)
                .map(PathBuf::from);
            to_value(client()?.save(path.as_deref()).map_err(control_error)?)
        }
        "anvil_command" => command_tool(arguments),
        "anvil_quit" => {
            let force = arguments
                .get("force")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            to_value(client()?.quit(force).map_err(control_error)?)
        }
        _ => Err(ToolFailure::params(format!("unknown tool: {name}"))),
    }
}

fn edit_tool(arguments: &Value) -> Result<Value, ToolFailure> {
    let mode = match arguments.get("mode").and_then(Value::as_str) {
        Some("set") => EditMode::Set,
        Some("insert") => EditMode::Insert,
        Some("replace") => EditMode::Replace,
        Some(_) => {
            return Err(ToolFailure::params("mode must be set, insert, or replace"));
        }
        None => return Err(ToolFailure::params("edit requires mode")),
    };
    let text = string_arg(arguments, "text")?;
    let expected_revision = arguments
        .get("expected_revision")
        .map(|value| {
            value.as_u64().ok_or_else(|| {
                ToolFailure::params("expected_revision must be a non-negative integer")
            })
        })
        .transpose()?;
    let mut client = client()?;
    let result = match mode {
        EditMode::Set => {
            let revision = expected_revision
                .ok_or_else(|| ToolFailure::params("set requires expected_revision"))?;
            client
                .edit_with_revision(mode, text, Some(revision))
                .map_err(control_error)?
        }
        EditMode::Insert => client
            .edit_with_revision(mode, text, expected_revision)
            .map_err(control_error)?,
        EditMode::Replace => {
            let range: EditRange = arguments
                .get("range")
                .cloned()
                .ok_or_else(|| ToolFailure::params("replace requires range"))
                .and_then(|value| {
                    serde_json::from_value(value)
                        .map_err(|error| ToolFailure::params(format!("invalid range: {error}")))
                })?;
            let revision = expected_revision
                .ok_or_else(|| ToolFailure::params("replace requires expected_revision"))?;
            client
                .replace(range, text, revision)
                .map_err(control_error)?
        }
    };
    to_value(result)
}

fn command_tool(arguments: &Value) -> Result<Value, ToolFailure> {
    let command = string_arg(arguments, "command")?;
    let command = command.trim().trim_start_matches(':').trim();
    match validate_remote_command(command) {
        Ok(()) => {}
        Err(CommandPolicyError::Empty) => {
            return Err(ToolFailure::params("command is empty"));
        }
        Err(error) => {
            return Err(ToolFailure::execution(tool_error(
                "policy_denied",
                5,
                None,
                error.to_string(),
            )));
        }
    }
    to_value(client()?.exec(command).map_err(control_error)?)
}

fn client() -> Result<Client, ToolFailure> {
    Client::connect_for_with_timeout(&workspace()?, Duration::from_secs(5)).map_err(control_error)
}

fn workspace() -> Result<PathBuf, String> {
    if let Some(workspace) = std::env::var_os("ANVIL_WORKSPACE") {
        return Ok(PathBuf::from(workspace));
    }
    std::env::current_dir().map_err(|error| format!("workspace unavailable: {error}"))
}

fn tools_list() -> Vec<Value> {
    TOOLS
        .iter()
        .filter(|tool| tool.capability.is_remote_allowed())
        .map(|tool| {
            json!({
                "name": tool.name,
                "title": tool.title,
                "description": tool.description(),
                "inputSchema": (tool.schema)(),
                "outputSchema": (tool.output_schema)(),
                "annotations": {
                    "readOnlyHint": tool.read_only,
                    "destructiveHint": tool.destructive,
                    "idempotentHint": tool.idempotent,
                    "openWorldHint": false
                }
            })
        })
        .collect()
}

fn allowed_commands() -> String {
    COMMANDS
        .iter()
        .filter(|command| command.capability.is_remote_allowed())
        .map(|command| command.name)
        .collect::<Vec<_>>()
        .join(", ")
}

struct ToolSpec {
    name: &'static str,
    title: &'static str,
    capability: Capability,
    description: &'static str,
    read_only: bool,
    destructive: bool,
    idempotent: bool,
    schema: fn() -> Value,
    output_schema: fn() -> Value,
}

impl ToolSpec {
    fn description(&self) -> String {
        if self.name == "anvil_command" {
            format!(
                "{} Allowed commands: {}",
                self.description,
                allowed_commands()
            )
        } else {
            self.description.to_owned()
        }
    }
}

const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "anvil_status",
        title: "Editor status",
        capability: Capability::Read,
        description: "Return editor version, workspace, active file, and dirty state.",
        read_only: true,
        destructive: false,
        idempotent: true,
        schema: empty_schema,
        output_schema: status_output_schema,
    },
    ToolSpec {
        name: "anvil_open",
        title: "Open file",
        capability: Capability::Read,
        description: "Open a file inside the workspace and place the cursor.",
        read_only: true,
        destructive: false,
        idempotent: true,
        schema: open_schema,
        output_schema: open_output_schema,
    },
    ToolSpec {
        name: "anvil_query",
        title: "Query buffer",
        capability: Capability::Read,
        description: "Read a bounded line range of the active buffer.",
        read_only: true,
        destructive: false,
        idempotent: true,
        schema: query_schema,
        output_schema: query_output_schema,
    },
    ToolSpec {
        name: "anvil_edit",
        title: "Edit buffer",
        capability: Capability::Edit,
        description: "Replace the buffer, insert at the cursor, or replace a revision-guarded range.",
        read_only: false,
        destructive: true,
        idempotent: false,
        schema: edit_schema,
        output_schema: edit_output_schema,
    },
    ToolSpec {
        name: "anvil_save",
        title: "Save buffer",
        capability: Capability::Edit,
        description: "Save the active buffer, optionally as a workspace path.",
        read_only: false,
        destructive: true,
        idempotent: true,
        schema: save_schema,
        output_schema: save_output_schema,
    },
    ToolSpec {
        name: "anvil_command",
        title: "Run Anvil command",
        capability: Capability::Git,
        description: "Execute an allowlisted Anvil Ex command through the editor state machine.",
        read_only: false,
        destructive: true,
        idempotent: false,
        schema: command_schema,
        output_schema: command_output_schema,
    },
    ToolSpec {
        name: "anvil_quit",
        title: "Quit editor",
        capability: Capability::Edit,
        description: "Quit the running editor; force discards unsaved changes.",
        read_only: false,
        destructive: true,
        idempotent: false,
        schema: quit_schema,
        output_schema: quit_output_schema,
    },
];

fn status_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "version": {"type": "string"},
            "protocol_version": {"type": "string"},
            "workspace": {"type": "string"},
            "file": {"anyOf": [{"type": "string"}, {"type": "null"}]},
            "dirty": {"type": "boolean"},
            "capabilities": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["version", "protocol_version", "workspace", "dirty", "capabilities"],
        "additionalProperties": true
    })
}

fn open_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "accepted": {"type": "boolean"},
            "path": {"type": "string"}
        },
        "required": ["accepted", "path"],
        "additionalProperties": true
    })
}

fn query_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "version": {"type": "string"},
            "workspace": {"type": "string"},
            "path": {"anyOf": [{"type": "string"}, {"type": "null"}]},
            "modified": {"type": "boolean"},
            "read_only": {"type": "boolean"},
            "cursor": {"type": "integer"},
            "revision": {"type": "integer"},
            "line_count": {"type": "integer"},
            "undo_depth": {"type": "integer"},
            "undo_bytes": {"type": "integer"},
            "selection": {"type": "array", "items": {"type": "array", "items": {"type": "integer"}}},
            "text": {"type": "string"},
            "text_truncated": {"type": "boolean"},
            "start_line": {"type": "integer"},
            "end_line": {"type": "integer"},
            "active": {"type": "integer"},
            "buffers": {"type": "array", "items": {"type": "object"}}
        },
        "required": ["version", "workspace", "modified", "read_only", "cursor", "revision", "text"],
        "additionalProperties": true
    })
}

fn edit_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "accepted": {"type": "boolean"},
            "revision": {"type": "integer"},
            "modified": {"type": "boolean"},
            "cursor": {"type": "integer"}
        },
        "required": ["accepted", "revision", "modified", "cursor"],
        "additionalProperties": true
    })
}

fn save_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "accepted": {"type": "boolean"},
            "path": {"anyOf": [{"type": "string"}, {"type": "null"}]},
            "status": {"type": "string"}
        },
        "required": ["accepted", "status"],
        "additionalProperties": true
    })
}

fn command_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "accepted": {"type": "boolean"},
            "status": {"type": "string"}
        },
        "required": ["accepted", "status"],
        "additionalProperties": true
    })
}

fn quit_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"accepted": {"type": "boolean"}},
        "required": ["accepted"],
        "additionalProperties": true
    })
}

fn empty_schema() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

fn open_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Workspace-relative file path"},
            "line": {"type": "integer", "minimum": 1, "default": 1},
            "column": {"type": "integer", "minimum": 1, "default": 1}
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

fn query_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "start_line": {"type": "integer", "minimum": 1},
            "end_line": {"type": "integer", "minimum": 1},
            "include_text": {"type": "boolean", "default": true}
        },
        "additionalProperties": false
    })
}

fn edit_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "mode": {"type": "string", "enum": ["set", "insert", "replace"]},
            "text": {"type": "string"},
            "range": {
                "type": "object",
                "properties": {
                    "start": {"$ref": "#/$defs/point"},
                    "end": {"$ref": "#/$defs/point"}
                },
                "required": ["start", "end"],
                "additionalProperties": false
            },
            "expected_revision": {"type": "integer", "minimum": 0}
        },
        "oneOf": [
            {"properties": {"mode": {"const": "set"}}, "required": ["mode", "text", "expected_revision"]},
            {"properties": {"mode": {"const": "insert"}}, "required": ["mode", "text"]},
            {"properties": {"mode": {"const": "replace"}}, "required": ["mode", "text", "range", "expected_revision"]}
        ],
        "required": ["mode", "text"],
        "additionalProperties": false,
        "$defs": {
            "point": {
                "type": "object",
                "properties": {
                    "line": {"type": "integer", "minimum": 1},
                    "byte_column": {"type": "integer", "minimum": 1}
                },
                "required": ["line", "byte_column"],
                "additionalProperties": false
            }
        }
    })
}

fn save_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Optional workspace-relative save-as path"}
        },
        "additionalProperties": false
    })
}

fn command_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "command": {"type": "string", "description": "Anvil Ex command, with or without ':'"}
        },
        "required": ["command"],
        "additionalProperties": false
    })
}

fn quit_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "force": {"type": "boolean", "default": false}
        },
        "additionalProperties": false
    })
}

fn string_arg<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, ToolFailure> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolFailure::params(format!("{name} is required")))
}

fn usize_arg(arguments: &Value, name: &str) -> Option<usize> {
    arguments
        .get(name)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
}

fn to_value<T: serde::Serialize>(value: T) -> Result<Value, ToolFailure> {
    serde_json::to_value(value).map_err(|error| {
        ToolFailure::execution(tool_error(
            "internal_error",
            1,
            None,
            format!("cannot serialize response: {error}"),
        ))
    })
}

fn control_error(error: ControlError) -> ToolFailure {
    let (kind, code, rpc_code) = match &error {
        ControlError::Timeout => ("timeout", 4, None),
        ControlError::Rpc { code, .. } => ("rpc", 5, Some(*code)),
        ControlError::RuntimeDir(_) | ControlError::Io(_) => ("no_instance", 3, None),
        ControlError::AlreadyRunning | ControlError::Lock(_) => ("local", 1, None),
        _ => ("control", 1, None),
    };
    ToolFailure::execution(tool_error(kind, code, rpc_code, error.to_string()))
}

fn tool_error(
    kind: &str,
    exit_code: i32,
    rpc_code: Option<i32>,
    message: impl Into<String>,
) -> String {
    json!({
        "ok": false,
        "error": {
            "kind": kind,
            "exit_code": exit_code,
            "rpc_code": rpc_code,
            "message": message.into(),
        }
    })
    .to_string()
}

fn success_response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error_response(id: Value, code: i32, message: &str) -> Value {
    error_response_with_data(id, code, message, None)
}

fn error_response_with_data(id: Value, code: i32, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(state: &mut ServerState, request: Value) -> Value {
        handle_line(&request.to_string(), state).expect("response")
    }

    fn legacy_initialize(state: &mut ServerState, version: &str) -> Value {
        call(
            state,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {"protocolVersion": version}
            }),
        )
    }

    fn modern_params(method: &str, mut extra: Value) -> Value {
        extra["_meta"] = json!({PROTOCOL_VERSION_META: MODERN_PROTOCOL_VERSION});
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": extra
        })
    }

    #[test]
    fn tool_list_uses_shared_manifest() {
        let tools = tools_list();
        let names = tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(names.contains(&"anvil_status"));
        assert!(names.contains(&"anvil_command"));
        let command = tools
            .iter()
            .find(|tool| tool["name"] == "anvil_command")
            .expect("command tool");
        let description = command["description"].as_str().expect("description");
        assert!(description.contains("git-diff"));
        assert!(!description.contains("pkg"));
        assert!(command["title"].is_string());
        assert_eq!(command["annotations"]["destructiveHint"], true);
        assert!(command["outputSchema"].is_object());
    }

    #[test]
    fn rejects_admin_command_without_connecting() {
        let response = tools_call(&json!({
            "name": "anvil_command",
            "arguments": {"command": "pkg install https://example.invalid/x"}
        }))
        .expect("tool result");
        assert_eq!(response["isError"], true);
        assert!(
            response["content"][0]["text"]
                .as_str()
                .expect("text")
                .contains("policy_denied")
        );
    }

    #[test]
    fn legacy_initialize_accepts_every_supported_version() {
        for version in LEGACY_PROTOCOL_VERSIONS {
            let mut state = ServerState::default();
            let response = legacy_initialize(&mut state, version);
            assert_eq!(response["result"]["protocolVersion"], *version);
            assert!(response["result"]["serverInfo"].is_object());
            assert!(response["result"].get("resultType").is_none());
            assert_eq!(state.era, ProtocolEra::Legacy);
            let ping = call(
                &mut state,
                json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}),
            );
            assert_eq!(ping["result"], json!({}));
        }
    }

    #[test]
    fn modern_discover_and_tools_list_are_versioned() {
        let mut state = ServerState::default();
        let discover = call(
            &mut state,
            json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover"}),
        );
        assert_eq!(
            discover["result"]["supportedVersions"],
            json!([MODERN_PROTOCOL_VERSION])
        );
        assert_eq!(discover["result"]["resultType"], "complete");
        assert_eq!(
            discover["result"]["_meta"][SERVER_INFO_META]["name"],
            "anvil-mcp"
        );
        assert_eq!(discover["result"]["ttlMs"], CACHE_TTL_MS);
        assert_eq!(discover["result"]["cacheScope"], "private");

        let first = call(&mut state, modern_params("tools/list", json!({})));
        assert_eq!(first["result"]["resultType"], "complete");
        assert_eq!(first["result"]["ttlMs"], CACHE_TTL_MS);
        assert_eq!(first["result"]["cacheScope"], "private");
        assert!(first["result"]["_meta"][SERVER_INFO_META].is_object());
        assert!(first["result"]["tools"].is_array());

        let second = call(&mut state, modern_params("tools/list", json!({})));
        assert_eq!(first["result"]["tools"], second["result"]["tools"]);
    }

    #[test]
    fn modern_tool_call_reports_result_metadata_and_error_without_structured_content() {
        let mut state = ServerState::default();
        let response = call(
            &mut state,
            modern_params(
                "tools/call",
                json!({"name": "anvil_status", "arguments": {}}),
            ),
        );
        assert_eq!(response["result"]["resultType"], "complete");
        assert_eq!(
            response["result"]["_meta"][SERVER_INFO_META]["version"],
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"].get("structuredContent").is_none());
        assert!(response["result"]["content"][0]["text"].is_string());
    }

    #[test]
    fn malformed_requests_and_errors_use_protocol_codes() {
        let mut state = ServerState::default();
        assert_eq!(
            handle_line("{", &mut state).expect("parse error")["error"]["code"],
            -32700
        );
        assert_eq!(
            call(&mut state, json!({"jsonrpc": "2.0", "id": 1}))["error"]["code"],
            -32600
        );
        assert_eq!(
            call(
                &mut state,
                json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
            )["error"]["code"],
            -32600
        );
        let unsupported = call(
            &mut state,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list",
                "params": {"_meta": {PROTOCOL_VERSION_META: "2099-01-01"}}
            }),
        );
        assert_eq!(unsupported["error"]["code"], -32022);
        assert_eq!(unsupported["error"]["data"]["requested"], "2099-01-01");
        assert_eq!(
            unsupported["error"]["data"]["supported"],
            json!([MODERN_PROTOCOL_VERSION])
        );
        assert_eq!(
            call(
                &mut state,
                json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"})
            )["error"]["code"],
            -32602
        );

        legacy_initialize(&mut state, "2024-11-05");
        assert_eq!(
            call(
                &mut state,
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "tools/call",
                    "params": {"name": "not_a_tool", "arguments": {}}
                })
            )["error"]["code"],
            -32602
        );
        assert_eq!(
            call(
                &mut state,
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "tools/call",
                    "params": {"name": "anvil_edit", "arguments": {"mode": "nope"}}
                })
            )["error"]["code"],
            -32602
        );
    }

    #[test]
    fn ping_is_legacy_only_and_notifications_are_silent() {
        let mut state = ServerState::default();
        legacy_initialize(&mut state, "2024-11-05");
        assert_eq!(
            call(
                &mut state,
                json!({"jsonrpc": "2.0", "id": 2, "method": "ping"})
            )["result"],
            json!({})
        );
        let mut modern = ServerState::default();
        call(&mut modern, modern_params("tools/list", json!({})));
        assert_eq!(
            call(&mut modern, modern_params("ping", json!({})))["error"]["code"],
            -32601
        );
        assert!(
            handle_line(
                &json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized"
                })
                .to_string(),
                &mut state
            )
            .is_none()
        );
    }

    #[test]
    fn protocol_eras_cannot_mix() {
        let mut state = ServerState::default();
        legacy_initialize(&mut state, "2024-11-05");
        assert_eq!(
            call(&mut state, modern_params("tools/list", json!({})))["error"]["code"],
            -32600
        );
        let mut modern = ServerState::default();
        call(&mut modern, modern_params("tools/list", json!({})));
        assert_eq!(
            legacy_initialize(&mut modern, "2024-11-05")["error"]["code"],
            -32600
        );
    }

    #[test]
    fn maps_control_errors_to_tool_errors() {
        let timeout = control_error(ControlError::Timeout);
        assert!(timeout.message().contains("\"kind\":\"timeout\""));
        let missing = control_error(ControlError::RuntimeDir("missing".to_owned()));
        assert!(missing.message().contains("\"kind\":\"no_instance\""));
        let rpc = control_error(ControlError::Rpc {
            code: -32002,
            message: "denied".to_owned(),
        });
        assert!(rpc.message().contains("\"kind\":\"rpc\""));
        assert!(rpc.message().contains("-32002"));
    }
}

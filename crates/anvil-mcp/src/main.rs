//! Thin MCP stdio adapter over the Anvil control socket.
//!
//! This binary never reads or writes workspace files. Every tool call is a
//! typed `anvil-control` request executed by the running TUI actor.

use anvil_control::{
    validate_remote_command, Capability, Client, CommandPolicyError, EditMode, EditRange,
    Error as ControlError, QueryOptions, COMMANDS,
};
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    process,
    time::Duration,
};

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

fn main() {
    if let Err(error) = run() {
        eprintln!("anvil-mcp: {error}");
        process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_line(&line) {
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn handle_line(line: &str) -> Option<Value> {
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
    let id = request.get("id").cloned()?;
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    match dispatch(method, params) {
        Ok(result) => Some(success_response(id, result)),
        Err((code, message)) => Some(error_response(id, code, &message)),
    }
}

fn dispatch(method: &str, params: Value) -> Result<Value, (i32, String)> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {
                "name": "anvil-mcp",
                "version": env!("CARGO_PKG_VERSION"),
            }
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools_list()})),
        "tools/call" => tools_call(&params),
        _ => Err((-32601, format!("unknown method: {method}"))),
    }
}

fn tools_call(params: &Value) -> Result<Value, (i32, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| (-32602, "tools/call requires a tool name".to_owned()))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let result = call_tool(name, &arguments);
    Ok(match result {
        Ok(value) => json!({
            "content": [{"type": "text", "text": value.to_string()}],
            "isError": false,
        }),
        Err(error) => json!({
            "content": [{"type": "text", "text": error}],
            "isError": true,
        }),
    })
}

fn call_tool(name: &str, arguments: &Value) -> Result<Value, String> {
    match name {
        "anvil_status" => to_value(client()?.status().map_err(control_error)?),
        "anvil_open" => {
            let path = string_arg(arguments, "path")?;
            let line = usize_arg(arguments, "line").unwrap_or(1);
            let column = usize_arg(arguments, "column").unwrap_or(1);
            client()?
                .open(&PathBuf::from(path), line, column)
                .map_err(control_error)
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
            client()?.save(path.as_deref()).map_err(control_error)
        }
        "anvil_command" => command_tool(arguments),
        "anvil_quit" => {
            let force = arguments
                .get("force")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            client()?.quit(force).map_err(control_error)
        }
        _ => Err(tool_error(
            "invalid_params",
            2,
            None,
            format!("unknown tool: {name}"),
        )),
    }
}

fn edit_tool(arguments: &Value) -> Result<Value, String> {
    let mode = match arguments.get("mode").and_then(Value::as_str) {
        Some("set") => EditMode::Set,
        Some("insert") => EditMode::Insert,
        Some("replace") => EditMode::Replace,
        Some(_) => {
            return Err(tool_error(
                "invalid_params",
                2,
                None,
                "mode must be set, insert, or replace",
            ));
        }
        None => {
            return Err(tool_error("invalid_params", 2, None, "edit requires mode"));
        }
    };
    let text = string_arg(arguments, "text")?;
    let mut client = client()?;
    let result = match mode {
        EditMode::Set | EditMode::Insert => client.edit(mode, text).map_err(control_error)?,
        EditMode::Replace => {
            let range: EditRange = arguments
                .get("range")
                .cloned()
                .ok_or_else(|| tool_error("invalid_params", 2, None, "replace requires range"))
                .and_then(|value| {
                    serde_json::from_value(value).map_err(|error| {
                        tool_error("invalid_params", 2, None, format!("invalid range: {error}"))
                    })
                })?;
            let revision = arguments
                .get("expected_revision")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    tool_error(
                        "invalid_params",
                        2,
                        None,
                        "replace requires expected_revision",
                    )
                })?;
            client
                .replace(range, text, revision)
                .map_err(control_error)?
        }
    };
    to_value(result)
}

fn command_tool(arguments: &Value) -> Result<Value, String> {
    let command = string_arg(arguments, "command")?;
    let command = command.trim().trim_start_matches(':').trim();
    match validate_remote_command(command) {
        Ok(()) => {}
        Err(CommandPolicyError::Empty) => {
            return Err(tool_error("invalid_params", 2, None, "command is empty"));
        }
        Err(error) => {
            return Err(tool_error("policy_denied", 5, None, error.to_string()));
        }
    }
    to_value(client()?.exec(command).map_err(control_error)?)
}

fn client() -> Result<Client, String> {
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
                "description": tool.description(),
                "inputSchema": (tool.schema)(),
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
    capability: Capability,
    description: &'static str,
    schema: fn() -> Value,
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
        capability: Capability::Read,
        description: "Return editor version, workspace, active file, and dirty state.",
        schema: empty_schema,
    },
    ToolSpec {
        name: "anvil_open",
        capability: Capability::Read,
        description: "Open a file inside the workspace and place the cursor.",
        schema: open_schema,
    },
    ToolSpec {
        name: "anvil_query",
        capability: Capability::Read,
        description: "Read a bounded line range of the active buffer.",
        schema: query_schema,
    },
    ToolSpec {
        name: "anvil_edit",
        capability: Capability::Edit,
        description:
            "Replace the buffer, insert at the cursor, or replace a revision-guarded range.",
        schema: edit_schema,
    },
    ToolSpec {
        name: "anvil_save",
        capability: Capability::Edit,
        description: "Save the active buffer, optionally as a workspace path.",
        schema: save_schema,
    },
    ToolSpec {
        name: "anvil_command",
        capability: Capability::Git,
        description: "Execute an allowlisted Anvil Ex command through the editor state machine.",
        schema: command_schema,
    },
    ToolSpec {
        name: "anvil_quit",
        capability: Capability::Edit,
        description: "Quit the running editor; force discards unsaved changes.",
        schema: quit_schema,
    },
];

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

fn string_arg<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error("invalid_params", 2, None, format!("{name} is required")))
}

fn usize_arg(arguments: &Value, name: &str) -> Option<usize> {
    arguments
        .get(name)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
}

fn to_value<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| {
        tool_error(
            "internal_error",
            1,
            None,
            format!("cannot serialize response: {error}"),
        )
    })
}

fn control_error(error: ControlError) -> String {
    let (kind, code, rpc_code) = match &error {
        ControlError::Timeout => ("timeout", 4, None),
        ControlError::Rpc { code, .. } => ("rpc", 5, Some(*code)),
        ControlError::RuntimeDir(_) | ControlError::Io(_) => ("no_instance", 3, None),
        ControlError::AlreadyRunning | ControlError::Lock(_) => ("local", 1, None),
        _ => ("control", 1, None),
    };
    tool_error(kind, code, rpc_code, error.to_string())
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
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn rejects_admin_command_without_connecting() {
        let error = call_tool(
            "anvil_command",
            &json!({"command": "pkg install https://example.invalid/x"}),
        )
        .expect_err("admin command");
        assert!(error.contains("policy_denied"));
    }

    #[test]
    fn reports_tools_and_initialization() {
        let init = dispatch("initialize", json!({})).expect("initialize");
        assert_eq!(init["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert!(init["capabilities"]["tools"].is_object());
        let tools = dispatch("tools/list", json!({})).expect("tools");
        assert!(tools["tools"].is_array());
    }

    #[test]
    fn maps_control_errors_to_tool_errors() {
        let timeout = control_error(ControlError::Timeout);
        assert!(timeout.contains("\"kind\":\"timeout\""));
        let missing = control_error(ControlError::RuntimeDir("missing".to_owned()));
        assert!(missing.contains("\"kind\":\"no_instance\""));
        let rpc = control_error(ControlError::Rpc {
            code: -32002,
            message: "denied".to_owned(),
        });
        assert!(rpc.contains("\"kind\":\"rpc\""));
        assert!(rpc.contains("-32002"));
    }
}

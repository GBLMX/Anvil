//! Sandboxed Luau runtime primitives.

use anvil_ipc::Connection;
use anvil_plugin_host::Token;
use mlua::{Lua, LuaSerdeExt, Result, Value, Variadic};
use serde_json::{Value as JsonValue, json};
use std::cell::RefCell;
use std::io::Write;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

/// Blocking editor RPC client owned by the Luau runtime.
pub(crate) struct EditorClient {
    connection: Connection,
    next_id: u64,
}

fn hex_token(token: Token) -> String {
    token.0.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl EditorClient {
    pub(crate) fn connect(path: &Path, plugin_id: &str, token: Token) -> std::io::Result<Self> {
        let mut connection = Connection::connect(path)?;
        connection.set_read_timeout(Some(Duration::from_secs(2)))?;
        let hello = json!({
            "jsonrpc": "2.0",
            "method": "session.hello",
            "params": {
                "plugin_id": plugin_id,
                "token": hex_token(token),
                "api_version": "1.0.0"
            }
        });
        connection
            .send(
                serde_json::to_string(&hello)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )
            .map_err(std::io::Error::other)?;
        Ok(Self {
            connection,
            next_id: 1,
        })
    }

    fn call(&mut self, method: &str, params: JsonValue) -> std::result::Result<JsonValue, String> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.connection
            .send(
                serde_json::to_string(&request)
                    .map_err(|error| error.to_string())?
                    .as_bytes(),
            )
            .map_err(|error| error.to_string())?;
        loop {
            let frame = self
                .connection
                .recv()
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "editor socket closed".to_owned())?;
            let response: JsonValue =
                serde_json::from_slice(&frame).map_err(|error| error.to_string())?;
            if response.get("id") != Some(&json!(id)) {
                continue;
            }
            if let Some(error) = response.get("error") {
                return Err(error
                    .get("message")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("editor RPC failed")
                    .to_owned());
            }
            return Ok(response.get("result").cloned().unwrap_or(JsonValue::Null));
        }
    }
}

/// Initializes Luau with its sandbox enabled and runs a deterministic check.
pub(crate) fn smoke() -> Result<i32> {
    let lua = Lua::new();
    lua.globals().set("require", mlua::Value::Nil)?;
    lua.sandbox(true)?;
    let value: i32 = lua.load("return 40 + 2").eval()?;
    let io_kind: String = lua.load("return type(io)").eval()?;
    let require_kind: String = lua.load("return type(require)").eval()?;
    if value != 42 || io_kind != "nil" || require_kind != "nil" {
        return Err(mlua::Error::RuntimeError(format!(
            "Luau sandbox smoke check failed: value={value}, io={io_kind}, require={require_kind}"
        )));
    }
    Ok(value)
}

/// Loads and executes one validated Luau entry inside the sandbox.
pub(crate) fn run_file(
    path: &Path,
    editor: Option<EditorClient>,
    command: Option<&str>,
) -> Result<()> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| mlua::Error::RuntimeError(format!("read plugin: {error}")))?;
    let lua = Lua::new();
    lua.globals().set("require", mlua::Value::Nil)?;
    lua.sandbox(true)?;
    install_print(&lua)?;
    install_editor(&lua, editor)?;
    install_plugin(&lua, command)?;
    lua.load(source).exec()
}

fn install_print(lua: &Lua) -> Result<()> {
    let print = lua.create_function(|lua, values: Variadic<Value>| {
        let mut output = String::new();
        for (index, value) in values.into_iter().enumerate() {
            if index > 0 {
                output.push('\t');
            }
            let debug = format!("{value:?}");
            if let Some(text) = lua.coerce_string(value)? {
                output.push_str(&text.to_string_lossy());
            } else {
                output.push_str(&debug);
            }
        }
        let _ = writeln!(std::io::stderr(), "{output}");
        Ok(())
    })?;
    lua.globals().set("print", print)
}

fn install_plugin(lua: &Lua, command: Option<&str>) -> Result<()> {
    let table = lua.create_table()?;
    table.set("command", command.unwrap_or_default())?;
    lua.globals().set("plugin", table)
}

fn install_editor(lua: &Lua, editor: Option<EditorClient>) -> Result<()> {
    let table = lua.create_table()?;
    let ui = lua.create_table()?;
    if let Some(editor) = editor {
        let editor = Rc::new(RefCell::new(editor));

        let get_client = Rc::clone(&editor);
        table.set(
            "get_view",
            lua.create_function(move |lua, ()| {
                let value = get_client
                    .borrow_mut()
                    .call("editor.get_view", json!({}))
                    .map_err(mlua::Error::RuntimeError)?;
                lua.to_value(&value)
            })?,
        )?;

        let snapshot_client = Rc::clone(&editor);
        table.set(
            "get_snapshot",
            lua.create_function(move |lua, ()| {
                let value = snapshot_client
                    .borrow_mut()
                    .call("editor.get_snapshot", json!({}))
                    .map_err(mlua::Error::RuntimeError)?;
                lua.to_value(&value)
            })?,
        )?;

        let edit_client = Rc::clone(&editor);
        table.set(
            "apply_edit",
            lua.create_function(move |lua, value: Value| {
                let params: JsonValue = lua.from_value(value)?;
                let result = edit_client
                    .borrow_mut()
                    .call("editor.apply_edit", params)
                    .map_err(mlua::Error::RuntimeError)?;
                lua.to_value(&result)
            })?,
        )?;

        let mode_client = Rc::clone(&editor);
        table.set(
            "set_mode",
            lua.create_function(move |lua, value: Value| {
                let params: JsonValue = lua.from_value(value)?;
                let result = mode_client
                    .borrow_mut()
                    .call("editor.set_mode", params)
                    .map_err(mlua::Error::RuntimeError)?;
                lua.to_value(&result)
            })?,
        )?;

        let command_client = Rc::clone(&editor);
        table.set(
            "register_command",
            lua.create_function(move |lua, value: Value| {
                let params: JsonValue = lua.from_value(value)?;
                let result = command_client
                    .borrow_mut()
                    .call("editor.register_command", params)
                    .map_err(mlua::Error::RuntimeError)?;
                lua.to_value(&result)
            })?,
        )?;

        let provider_client = Rc::clone(&editor);
        table.set(
            "register_provider",
            lua.create_function(move |lua, value: Value| {
                let params: JsonValue = lua.from_value(value)?;
                let result = provider_client
                    .borrow_mut()
                    .call("editor.register_provider", params)
                    .map_err(mlua::Error::RuntimeError)?;
                lua.to_value(&result)
            })?,
        )?;

        let notify_client = Rc::clone(&editor);
        ui.set(
            "notify",
            lua.create_function(move |lua, value: Value| {
                let params: JsonValue = lua.from_value(value)?;
                let result = notify_client
                    .borrow_mut()
                    .call("ui.notify", params)
                    .map_err(mlua::Error::RuntimeError)?;
                lua.to_value(&result)
            })?,
        )?;

        let status_client = Rc::clone(&editor);
        ui.set(
            "status",
            lua.create_function(move |lua, value: Value| {
                let params: JsonValue = lua.from_value(value)?;
                let result = status_client
                    .borrow_mut()
                    .call("ui.status", params)
                    .map_err(mlua::Error::RuntimeError)?;
                lua.to_value(&result)
            })?,
        )?;
    } else {
        for method in [
            "get_view",
            "get_snapshot",
            "apply_edit",
            "set_mode",
            "register_command",
            "register_provider",
        ] {
            table.set(
                method,
                lua.create_function(|_, ()| {
                    Err::<Value, _>(mlua::Error::RuntimeError(
                        "editor socket unavailable".to_owned(),
                    ))
                })?,
            )?;
        }
        for method in ["notify", "status"] {
            ui.set(
                method,
                lua.create_function(|_, ()| {
                    Err::<Value, _>(mlua::Error::RuntimeError(
                        "editor socket unavailable".to_owned(),
                    ))
                })?,
            )?;
        }
    }
    lua.globals().set("ui", ui)?;
    lua.globals().set("editor", table)
}

#[cfg(test)]
mod file_tests {
    use super::*;

    #[test]
    fn runs_validated_luau_entry() {
        let path = std::env::temp_dir().join(format!(
            "anvil-plugin-{}-{}.luau",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::write(&path, "return 42").expect("write");
        let result = run_file(&path, None, None);
        let _ = std::fs::remove_file(path);
        result.expect("run plugin");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn luau_sandbox_runs_without_ambient_io() {
        assert_eq!(super::smoke().expect("sandboxed Luau"), 42);
    }

    #[test]
    fn luau_editor_get_view_uses_editor_socket() {
        use anvil_ipc::Listener;

        let socket = std::env::temp_dir().join(format!(
            "anvil-luau-editor-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let listener = Listener::bind(&socket).expect("listen");
        let server = std::thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            let hello = connection.recv().expect("read").expect("hello");
            let hello: JsonValue = serde_json::from_slice(&hello).expect("hello json");
            assert_eq!(hello["method"], "session.hello");
            let frame = connection.recv().expect("read").expect("request");
            let request: JsonValue = serde_json::from_slice(&frame).expect("json");
            assert_eq!(request["method"], "editor.get_view");
            let response = json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": {
                    "revision": 0,
                    "interaction_mode": "select",
                    "viewport": {
                        "first_line": 0,
                        "first_column": 0,
                        "line_count": 24,
                        "width": 80,
                        "height": 24
                    },
                    "selections": [{
                        "anchor": {
                            "byte_offset": 5,
                            "line": 2,
                            "grapheme_column": 4,
                            "display_column": 4
                        },
                        "head": {
                            "byte_offset": 5,
                            "line": 2,
                            "grapheme_column": 4,
                            "display_column": 4
                        }
                    }]
                }
            });
            connection
                .send(
                    serde_json::to_string(&response)
                        .expect("response")
                        .as_bytes(),
                )
                .expect("send");

            let frame = connection.recv().expect("read").expect("apply request");
            let request: JsonValue = serde_json::from_slice(&frame).expect("apply json");
            assert_eq!(request["method"], "editor.apply_edit");
            assert_eq!(request["params"]["buffer_id"], 1);
            assert_eq!(request["params"]["edits"].as_array().map(Vec::len), Some(1));
            let response = json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": { "revision": 1, "applied_edits": 1 }
            });
            connection
                .send(
                    serde_json::to_string(&response)
                        .expect("apply response")
                        .as_bytes(),
                )
                .expect("apply send");
        });
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/editor_bridge/main.luau");
        let client =
            EditorClient::connect(&socket, "test", Token([3; 32])).expect("connect editor");
        run_file(&fixture, Some(client), None).expect("run bridge plugin");
        server.join().expect("server");
        let _ = std::fs::remove_file(socket);
    }

    #[test]
    fn official_trim_whitespace_plugin_applies_edits() {
        use anvil_ipc::Listener;

        let socket = std::env::temp_dir().join(format!(
            "anvil-luau-trim-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let listener = Listener::bind(&socket).expect("listen");
        let server = std::thread::spawn(move || {
            let mut connection = listener.accept().expect("accept");
            let hello = connection.recv().expect("read").expect("hello");
            let hello: JsonValue = serde_json::from_slice(&hello).expect("hello json");
            assert_eq!(hello["method"], "session.hello");
            let frame = connection.recv().expect("read").expect("snapshot request");
            let request: JsonValue = serde_json::from_slice(&frame).expect("snapshot json");
            assert_eq!(request["method"], "editor.get_snapshot");
            let response = json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": {
                    "buffer_id": 1,
                    "revision": 7,
                    "text": "a  \n\tb\t\nc"
                }
            });
            connection
                .send(
                    serde_json::to_string(&response)
                        .expect("snapshot response")
                        .as_bytes(),
                )
                .expect("snapshot send");

            let frame = connection.recv().expect("read").expect("edit request");
            let request: JsonValue = serde_json::from_slice(&frame).expect("edit json");
            assert_eq!(request["method"], "editor.apply_edit");
            assert_eq!(request["params"]["buffer_id"], 1);
            assert_eq!(request["params"]["base_revision"], 7);
            let edits = request["params"]["edits"].as_array().expect("edits");
            assert_eq!(edits.len(), 2);
            assert_eq!(edits[0]["range"]["start"], 1);
            assert_eq!(edits[0]["range"]["end"], 3);
            assert_eq!(edits[1]["range"]["start"], 6);
            assert_eq!(edits[1]["range"]["end"], 7);
            let response = json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": { "revision": 8, "applied_edits": 2 }
            });
            connection
                .send(
                    serde_json::to_string(&response)
                        .expect("edit response")
                        .as_bytes(),
                )
                .expect("edit send");
        });
        let plugin = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/official/trim-whitespace/main.luau");
        let client =
            EditorClient::connect(&socket, "test", Token([3; 32])).expect("connect editor");
        run_file(
            &plugin,
            Some(client),
            Some("anvil.trim-trailing-whitespace"),
        )
        .expect("run official plugin");
        server.join().expect("server");
        let _ = std::fs::remove_file(socket);
    }
}

//! Sandboxed Luau runtime primitives.

use anvil_ipc::Connection;
use anvil_plugin_host::{Token, write_plugin_line, write_plugin_stderr};
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
    path: std::path::PathBuf,
    plugin_id: String,
    token: [u8; 32],
}

enum CallError {
    Transport(String),
    Rpc(String),
}

fn hex_token(token: [u8; 32]) -> String {
    token.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl EditorClient {
    pub(crate) fn connect(path: &Path, plugin_id: &str, token: Token) -> std::io::Result<Self> {
        let connection = open_connection(path, plugin_id, token.0)?;
        Ok(Self {
            connection,
            next_id: 1,
            path: path.to_path_buf(),
            plugin_id: plugin_id.to_owned(),
            token: token.0,
        })
    }

    fn call(&mut self, method: &str, params: JsonValue) -> std::result::Result<JsonValue, String> {
        match self.request(method, params.clone()) {
            Ok(value) => Ok(value),
            Err(CallError::Rpc(message)) => Err(message),
            Err(CallError::Transport(first)) => {
                self.connection = open_connection(&self.path, &self.plugin_id, self.token)
                    .map_err(|error| format!("{first}; reconnect failed: {error}"))?;
                if is_retryable(method) {
                    self.request(method, params).map_err(|error| match error {
                        CallError::Transport(message) | CallError::Rpc(message) => message,
                    })
                } else {
                    Err(first)
                }
            }
        }
    }

    fn request(
        &mut self,
        method: &str,
        params: JsonValue,
    ) -> std::result::Result<JsonValue, CallError> {
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
                    .map_err(|error| CallError::Transport(error.to_string()))?
                    .as_bytes(),
            )
            .map_err(|error| CallError::Transport(error.to_string()))?;
        loop {
            let frame = self
                .connection
                .recv()
                .map_err(|error| CallError::Transport(error.to_string()))?
                .ok_or_else(|| CallError::Transport("editor socket closed".to_owned()))?;
            let response: JsonValue = serde_json::from_slice(&frame)
                .map_err(|error| CallError::Transport(error.to_string()))?;
            if response.get("id") != Some(&json!(id)) {
                continue;
            }
            if let Some(error) = response.get("error") {
                return Err(CallError::Rpc(
                    error
                        .get("message")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("editor RPC failed")
                        .to_owned(),
                ));
            }
            return Ok(response.get("result").cloned().unwrap_or(JsonValue::Null));
        }
    }
}

fn is_retryable(method: &str) -> bool {
    matches!(method, "editor.get_view" | "editor.get_snapshot")
}

fn open_connection(path: &Path, plugin_id: &str, token: [u8; 32]) -> std::io::Result<Connection> {
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
    Ok(connection)
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
) -> Result<Lua> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| mlua::Error::RuntimeError(format!("read plugin: {error}")))?;
    let lua = Lua::new();
    lua.globals().set("require", mlua::Value::Nil)?;
    lua.sandbox(true)?;
    install_print(&lua)?;
    install_editor(&lua, editor)?;
    install_plugin(&lua, command)?;
    lua.load(source).exec()?;
    Ok(lua)
}

pub(crate) fn call_provider(
    lua: &Lua,
    params: &JsonValue,
) -> std::result::Result<JsonValue, String> {
    let handler = params
        .get("handler")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "missing provider handler".to_owned())?;
    let function: mlua::Function = lua
        .globals()
        .get(handler)
        .map_err(|error| format!("provider handler {handler}: {error}"))?;
    let request = lua.to_value(params).map_err(|error| error.to_string())?;
    let result: Value = function
        .call(request)
        .map_err(|error| format!("provider handler {handler}: {error}"))?;
    lua.from_value(result).map_err(|error| error.to_string())
}

pub(crate) fn call_event(lua: &Lua, params: &JsonValue) -> std::result::Result<(), String> {
    let subscription_id = params
        .get("subscription_id")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| "missing subscription id".to_owned())?;
    let sequence = params
        .get("sequence")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    let sequences: mlua::Table = lua
        .globals()
        .get("__anvil_event_sequences")
        .map_err(|error| error.to_string())?;
    if sequences
        .get::<u64>(subscription_id)
        .is_ok_and(|last| sequence <= last)
    {
        return Ok(());
    }
    sequences
        .set(subscription_id, sequence)
        .map_err(|error| error.to_string())?;
    let handlers: mlua::Table = lua
        .globals()
        .get("__anvil_event_handlers")
        .map_err(|error| error.to_string())?;
    let Ok(handler) = handlers.get::<mlua::Function>(subscription_id) else {
        let pending: mlua::Table = lua
            .globals()
            .get("__anvil_event_pending")
            .map_err(|error| error.to_string())?;
        pending
            .set(
                subscription_id,
                lua.to_value(params).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
        return Ok(());
    };
    let event = lua.to_value(params).map_err(|error| error.to_string())?;
    handler
        .call::<()>(event)
        .map_err(|error| format!("event handler {subscription_id}: {error}"))
}

fn deliver_pending_event(lua: &Lua, subscription_id: u64, handler: &mlua::Function) {
    let Ok(pending) = lua.globals().get::<mlua::Table>("__anvil_event_pending") else {
        return;
    };
    let Ok(event) = pending.get::<Value>(subscription_id) else {
        return;
    };
    if matches!(event, Value::Nil) {
        return;
    }
    let _ = pending.set(subscription_id, Value::Nil);
    if let Err(error) = handler.call::<()>(event) {
        let message = format!("pending handler {subscription_id}: {error}");
        write_plugin_stderr("host.event: ", &message);
    }
}

/// Installs `print` so plugin output can never emit raw terminal control
/// sequences: output is escaped and capped before it reaches stderr.
fn install_print(lua: &Lua) -> Result<()> {
    install_print_with(lua, |line| {
        let _ = std::io::stderr().lock().write_all(line);
    })
}

/// Installs `print` with an injectable sink, so tests can capture what a plugin
/// actually emits.
fn install_print_with(lua: &Lua, emit: impl Fn(&[u8]) + 'static) -> Result<()> {
    let emit = Rc::new(emit);
    let print = lua.create_function(move |lua, values: Variadic<Value>| {
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
        let mut escaped = Vec::with_capacity(output.len() + 1);
        let _ = write_plugin_line(&mut escaped, &output);
        emit(&escaped);
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
    let event_handlers = lua.create_table()?;
    lua.globals()
        .set("__anvil_event_handlers", &event_handlers)?;
    let event_pending = lua.create_table()?;
    lua.globals().set("__anvil_event_pending", &event_pending)?;
    let event_sequences = lua.create_table()?;
    lua.globals()
        .set("__anvil_event_sequences", &event_sequences)?;
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

        let subscribe_client = Rc::clone(&editor);
        let subscribe_handlers = event_handlers.clone();
        table.set(
            "subscribe",
            lua.create_function(move |lua, (events, handler): (Value, mlua::Function)| {
                let events: JsonValue = lua.from_value(events)?;
                let result = subscribe_client
                    .borrow_mut()
                    .call("event.subscribe", json!({ "events": events }))
                    .map_err(mlua::Error::RuntimeError)?;
                let subscription_id = result
                    .get("subscription_id")
                    .and_then(JsonValue::as_u64)
                    .ok_or_else(|| {
                        mlua::Error::RuntimeError("missing subscription id".to_owned())
                    })?;
                let pending_handler = handler.clone();
                subscribe_handlers.set(subscription_id, handler)?;
                deliver_pending_event(lua, subscription_id, &pending_handler);
                Ok(subscription_id)
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
            "subscribe",
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
    fn print_never_reaches_captured_sink_raw() {
        let lua = Lua::new();
        let captured = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&captured);
        install_print_with(&lua, move |line| sink.borrow_mut().extend_from_slice(line))
            .expect("install print");
        lua.load(r#"print("start " .. string.char(27) .. "]0;owned" .. string.char(7) .. " end")"#)
            .exec()
            .expect("run print");
        let captured = captured.borrow();
        assert!(!captured.contains(&0x1b), "raw ESC reached output");
        assert!(!captured.contains(&0x07), "raw BEL reached output");
        let text = String::from_utf8(captured.clone()).expect("utf8");
        assert!(text.ends_with('\n'), "{text:?}");
        assert!(text.contains(r"\x1b]0;owned\x07"), "{text:?}");
    }

    #[test]
    fn editor_client_reconnects_after_peer_close() {
        use anvil_ipc::Listener;

        let socket = std::env::temp_dir().join(format!(
            "anvil-editor-reconnect-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let listener = Listener::bind(&socket).expect("listen");
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let mut first = listener.accept().expect("first accept");
                let hello = first.recv().expect("hello").expect("first hello");
                let hello: JsonValue = serde_json::from_slice(&hello).expect("hello json");
                assert_eq!(hello["method"], "session.hello");
                drop(first);

                let mut second = listener.accept().expect("second accept");
                let hello = second.recv().expect("hello").expect("second hello");
                let hello: JsonValue = serde_json::from_slice(&hello).expect("hello json");
                assert_eq!(hello["method"], "session.hello");
                let frame = second.recv().expect("request").expect("request frame");
                let request: JsonValue = serde_json::from_slice(&frame).expect("request json");
                assert_eq!(request["method"], "editor.get_view");
                second
                    .send(
                        serde_json::to_string(&json!({
                            "jsonrpc": "2.0",
                            "id": request["id"],
                            "result": "reconnected"
                        }))
                        .expect("response")
                        .as_bytes(),
                    )
                    .expect("send response");
            });
            let mut client =
                EditorClient::connect(&socket, "test", Token([6; 32])).expect("connect editor");
            let value = client
                .call("editor.get_view", json!({}))
                .expect("reconnected call");
            assert_eq!(value, json!("reconnected"));
        });
        let _ = std::fs::remove_file(socket);
    }

    #[test]
    fn editor_client_does_not_replay_mutations_after_reconnect() {
        use anvil_ipc::Listener;

        let socket = std::env::temp_dir().join(format!(
            "anvil-editor-no-replay-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let listener = Listener::bind(&socket).expect("listen");
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let mut first = listener.accept().expect("first accept");
                let hello = first.recv().expect("hello").expect("first hello");
                let hello: JsonValue = serde_json::from_slice(&hello).expect("hello json");
                assert_eq!(hello["method"], "session.hello");
                drop(first);

                let mut second = listener.accept().expect("second accept");
                let hello = second.recv().expect("hello").expect("second hello");
                let hello: JsonValue = serde_json::from_slice(&hello).expect("hello json");
                assert_eq!(hello["method"], "session.hello");
                second
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .expect("read timeout");
                assert!(
                    !matches!(second.recv(), Ok(Some(_))),
                    "mutating request was replayed after reconnect"
                );
            });
            let mut client =
                EditorClient::connect(&socket, "test", Token([7; 32])).expect("connect editor");
            assert!(
                client
                    .call("editor.apply_edit", json!({ "text": "duplicate" }))
                    .is_err()
            );
        });
        let _ = std::fs::remove_file(socket);
    }

    #[test]
    fn provider_handler_survives_entry_execution() {
        let path = std::env::temp_dir().join(format!(
            "anvil-provider-{}-{}.luau",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::write(
            &path,
            r#"
function provide(request)
    return {
        request_id = request.request_id,
        items = {
            { label = "buffer", value = "1", detail = nil, insert_text = nil }
        }
    }
end
"#,
        )
        .expect("write");
        let lua = run_file(&path, None, None).expect("run plugin");
        let _ = std::fs::remove_file(path);
        let response = call_provider(
            &lua,
            &json!({
                "request_id": 4,
                "handler": "provide",
                "query": "buf",
                "limit": 10
            }),
        )
        .expect("provider");
        assert_eq!(response["request_id"], 4);
        assert_eq!(response["items"][0]["label"], "buffer");
    }

    #[test]
    fn event_handler_receives_payload() {
        let path = std::env::temp_dir().join(format!(
            "anvil-event-{}-{}.luau",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::write(
            &path,
            r#"
__anvil_event_handlers[7] = function(event)
    received = event.payload.generation
end
"#,
        )
        .expect("write");
        let lua = run_file(&path, None, None).expect("run plugin");
        let _ = std::fs::remove_file(path);
        call_event(
            &lua,
            &json!({
                "subscription_id": 7,
                "event": "diagnostics.changed",
                "sequence": 1,
                "payload": { "generation": 9 }
            }),
        )
        .expect("event");
        let received: u64 = lua.globals().get("received").expect("received");
        assert_eq!(received, 9);
    }

    #[test]
    fn handler_error_does_not_break_later_delivery() {
        let path = std::env::temp_dir().join(format!(
            "anvil-handler-error-{}-{}.luau",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::write(
            &path,
            r#"
__anvil_event_handlers[7] = function()
    error("boom")
end
"#,
        )
        .expect("write");
        let lua = run_file(&path, None, None).expect("run plugin");
        let _ = std::fs::remove_file(path);
        assert!(
            call_event(
                &lua,
                &json!({
                    "subscription_id": 7,
                    "event": "diagnostics.changed",
                    "sequence": 1,
                    "payload": {}
                }),
            )
            .is_err()
        );
        lua.load(
            "__anvil_event_handlers[7] = function(event) received = event.payload.generation end",
        )
        .exec()
        .expect("replace handler");
        call_event(
            &lua,
            &json!({
                "subscription_id": 7,
                "event": "diagnostics.changed",
                "sequence": 2,
                "payload": { "generation": 12 }
            }),
        )
        .expect("later event");
        let received: u64 = lua.globals().get("received").expect("received");
        assert_eq!(received, 12);
    }

    #[test]
    fn subscribe_event_reaches_lua_and_status() {
        use anvil_ipc::Listener;

        let socket = std::env::temp_dir().join(format!(
            "anvil-subscribe-status-{}-{}.sock",
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
            let frame = connection.recv().expect("read").expect("subscribe");
            let request: JsonValue = serde_json::from_slice(&frame).expect("subscribe json");
            assert_eq!(request["method"], "event.subscribe");
            let response = json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": { "subscription_id": 7 }
            });
            connection
                .send(
                    serde_json::to_string(&response)
                        .expect("subscribe response")
                        .as_bytes(),
                )
                .expect("send subscribe");
            let frame = connection.recv().expect("read").expect("status");
            let request: JsonValue = serde_json::from_slice(&frame).expect("status json");
            assert_eq!(request["method"], "ui.status");
            assert_eq!(request["params"]["message"], "got 42");
            let response = json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": {}
            });
            connection
                .send(
                    serde_json::to_string(&response)
                        .expect("status response")
                        .as_bytes(),
                )
                .expect("send status");
        });
        let path = std::env::temp_dir().join(format!(
            "anvil-subscribe-status-{}-{}.luau",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::write(
            &path,
            r#"
editor.subscribe({"diagnostics.changed"}, function(event)
    ui.status({message = "got " .. tostring(event.payload.generation)})
end)
"#,
        )
        .expect("write");
        let client =
            EditorClient::connect(&socket, "test", Token([4; 32])).expect("connect editor");
        let lua = run_file(&path, Some(client), None).expect("run plugin");
        let _ = std::fs::remove_file(path);
        call_event(
            &lua,
            &json!({
                "subscription_id": 7,
                "event": "diagnostics.changed",
                "sequence": 1,
                "payload": { "generation": 42 }
            }),
        )
        .expect("event");
        server.join().expect("server");
        let _ = std::fs::remove_file(socket);
    }

    #[test]
    fn pending_event_is_delivered_after_handler_registration() {
        let path = std::env::temp_dir().join(format!(
            "anvil-pending-{}-{}.luau",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::write(&path, "return").expect("write");
        let lua = run_file(&path, None, None).expect("run plugin");
        let _ = std::fs::remove_file(path);
        call_event(
            &lua,
            &json!({
                "subscription_id": 7,
                "event": "diagnostics.changed",
                "sequence": 1,
                "payload": { "generation": 11 }
            }),
        )
        .expect("buffer event");
        lua.load(
            "__anvil_event_handlers[7] = function(event) received = event.payload.generation end",
        )
        .exec()
        .expect("register handler");
        let handlers: mlua::Table = lua
            .globals()
            .get("__anvil_event_handlers")
            .expect("handlers");
        let handler: mlua::Function = handlers.get(7).expect("handler");
        deliver_pending_event(&lua, 7, &handler);
        let received: u64 = lua.globals().get("received").expect("received");
        assert_eq!(received, 11);
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

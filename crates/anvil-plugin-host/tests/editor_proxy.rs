#![cfg(target_os = "linux")]

use anvil_api::{
    ApplyEditRequest, ApplyEditResponse, DocumentEdit, DocumentSnapshot, EditorView,
    InteractionMode, SetModeRequest, SetModeResponse, TextRange, Viewport,
};
use anvil_core::TextBuffer;
use anvil_editor::EditorState;
use anvil_ipc::{read_frame, write_frame};
use anvil_plugin_host::{Token, spawn};

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BUFFER_ID: u64 = 7;

fn host_bin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_anvil-plugin-host"))
}

fn state() -> EditorState {
    let mut state = EditorState::from_text("hello");
    state.set_viewport(Viewport {
        first_line: 0,
        first_column: 0,
        line_count: 24,
        width: 80,
        height: 24,
    });
    state
}

fn snapshot(state: &EditorState) -> DocumentSnapshot {
    DocumentSnapshot {
        buffer_id: BUFFER_ID,
        revision: state.revision(),
        text: state.text().slice(0..state.text().len_bytes()),
    }
}

fn socket_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "anvil-{name}-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ))
}

fn start_host(
    name: &str,
    timeout_ms: u64,
) -> (UnixListener, UnixListener, anvil_plugin_host::HostProcess) {
    let control_path = socket_path(&format!("control-{name}"));
    let editor_path = socket_path(&format!("editor-{name}"));
    let _ = std::fs::remove_file(&control_path);
    let _ = std::fs::remove_file(&editor_path);
    let control = UnixListener::bind(&control_path).expect("bind control");
    let editor = UnixListener::bind(&editor_path).expect("bind editor");
    let mut host = spawn(
        host_bin(),
        &[
            "--socket".to_owned(),
            control_path.display().to_string(),
            "--editor-socket".to_owned(),
            editor_path.display().to_string(),
            "--editor-timeout-ms".to_owned(),
            timeout_ms.to_string(),
            "--plugin-id".to_owned(),
            "test".to_owned(),
        ],
        Token([31; 32]),
    )
    .expect("spawn host");
    let _ = host.read_hello().expect("hello");
    (control, editor, host)
}

fn rpc(
    connection: &mut UnixStream,
    id: u64,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    write_frame(
        connection,
        serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .expect("request json")
        .as_bytes(),
    )
    .expect("write request");
    let frame = read_frame(connection)
        .expect("read response")
        .expect("response");
    serde_json::from_slice(&frame).expect("response json")
}

fn fake_editor_result(
    state: &mut EditorState,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, (i32, String)> {
    match method {
        "editor.get_view" => Ok(serde_json::to_value(state.snapshot().view()).expect("view")),
        "editor.get_snapshot" => Ok(serde_json::to_value(snapshot(state)).expect("snapshot")),
        "editor.set_mode" => {
            let request: SetModeRequest =
                serde_json::from_value(params).map_err(|error| (-32602, error.to_string()))?;
            state.set_interaction_mode(request.mode);
            Ok(serde_json::to_value(SetModeResponse { mode: request.mode }).expect("set mode"))
        }
        "editor.apply_edit" => {
            let request: ApplyEditRequest =
                serde_json::from_value(params).map_err(|error| (-32602, error.to_string()))?;
            if request.buffer_id != BUFFER_ID {
                return Err((-32602, "buffer mismatch".to_owned()));
            }
            let edits = request
                .edits
                .iter()
                .map(|edit| {
                    let start = usize::try_from(edit.range.start)
                        .map_err(|_| (-32602, "invalid range".to_owned()))?;
                    let end = usize::try_from(edit.range.end)
                        .map_err(|_| (-32602, "invalid range".to_owned()))?;
                    Ok((start..end, edit.text.clone()))
                })
                .collect::<Result<Vec<_>, (i32, String)>>()?;
            match state.apply_transaction_at(request.base_revision, edits) {
                Ok(revision) => Ok(serde_json::to_value(ApplyEditResponse {
                    revision,
                    applied_edits: request.edits.len() as u32,
                })
                .expect("apply response")),
                Err(error) => Err((-32009, error.to_string())),
            }
        }
        _ => Err((-32601, "method not found".to_owned())),
    }
}

fn fake_editor(
    listener: UnixListener,
    mut state: EditorState,
    delay: Duration,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut connection, _) = listener.accept().expect("accept editor");
        let frame = read_frame(&mut connection)
            .expect("read handshake")
            .expect("handshake");
        let hello: serde_json::Value = serde_json::from_slice(&frame).expect("handshake json");
        assert_eq!(hello["method"], "session.hello");
        assert_eq!(hello["params"]["plugin_id"], "test");
        assert_eq!(hello["params"]["api_version"], "1.0.0");
        assert_eq!(hello["params"]["token"].as_str().map(str::len), Some(64));
        loop {
            let frame = match read_frame(&mut connection) {
                Ok(Some(frame)) => frame,
                Ok(None) | Err(_) => return,
            };
            let request: serde_json::Value = serde_json::from_slice(&frame).expect("request");
            let id = request["id"].clone();
            let method = request["method"].as_str().expect("method");
            let params = request.get("params").cloned().unwrap_or_default();
            thread::sleep(delay);
            let result = fake_editor_result(&mut state, method, params);
            let response = match result {
                Ok(result) => serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result,
                }),
                Err((code, message)) => serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": code, "message": message },
                }),
            };
            if write_frame(
                &mut connection,
                serde_json::to_string(&response)
                    .expect("response json")
                    .as_bytes(),
            )
            .is_err()
            {
                return;
            }
        }
    })
}

#[test]
fn proxy_matches_first_party_editor_dtos_revisions_and_errors() {
    let mut direct = state();
    let (control_listener, editor_listener, mut host) = start_host("parity", 1_000);
    let editor_thread = fake_editor(editor_listener, state(), Duration::ZERO);
    let mut control = control_listener.accept().expect("accept control").0;

    let response = rpc(&mut control, 1, "editor.get_view", serde_json::json!({}));
    let proxied: EditorView = serde_json::from_value(response["result"].clone()).expect("view");
    assert_eq!(proxied, direct.snapshot().view());

    let response = rpc(
        &mut control,
        2,
        "editor.get_snapshot",
        serde_json::json!({}),
    );
    let proxied: DocumentSnapshot =
        serde_json::from_value(response["result"].clone()).expect("snapshot");
    assert_eq!(proxied, snapshot(&direct));

    let response = rpc(
        &mut control,
        3,
        "editor.set_mode",
        serde_json::json!({ "mode": "select" }),
    );
    let proxied: SetModeResponse =
        serde_json::from_value(response["result"].clone()).expect("set mode");
    assert_eq!(
        proxied,
        SetModeResponse {
            mode: InteractionMode::Select
        }
    );

    let edit = ApplyEditRequest {
        buffer_id: BUFFER_ID,
        base_revision: direct.revision(),
        edits: vec![DocumentEdit {
            range: TextRange { start: 1, end: 4 },
            text: "aXd".to_owned(),
        }],
    };
    let response = rpc(
        &mut control,
        4,
        "editor.apply_edit",
        serde_json::to_value(&edit).expect("apply request"),
    );
    let proxied: ApplyEditResponse =
        serde_json::from_value(response["result"].clone()).expect("apply");
    let expected_revision = direct
        .apply_transaction_at(edit.base_revision, vec![(1..4, "aXd".to_owned())])
        .expect("direct apply");
    assert_eq!(
        proxied,
        ApplyEditResponse {
            revision: expected_revision,
            applied_edits: 1,
        }
    );

    let response = rpc(
        &mut control,
        5,
        "editor.get_snapshot",
        serde_json::json!({}),
    );
    let proxied: DocumentSnapshot =
        serde_json::from_value(response["result"].clone()).expect("snapshot");
    assert_eq!(proxied, snapshot(&direct));

    let stale = ApplyEditRequest {
        buffer_id: BUFFER_ID,
        base_revision: expected_revision - 1,
        edits: vec![DocumentEdit {
            range: TextRange { start: 0, end: 0 },
            text: "x".to_owned(),
        }],
    };
    let direct_error = direct
        .apply_transaction_at(stale.base_revision, vec![(0..0, "x".to_owned())])
        .expect_err("stale edit");
    let response = rpc(
        &mut control,
        6,
        "editor.apply_edit",
        serde_json::to_value(&stale).expect("stale request"),
    );
    assert_eq!(response["error"]["code"], -32009);
    assert_eq!(response["error"]["message"], direct_error.to_string());

    control
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown");
    assert_eq!(host.wait().expect("wait").code(), Some(0));
    editor_thread.join().expect("editor thread");
}

#[test]
fn proxy_cancels_an_in_flight_editor_request() {
    let (control_listener, editor_listener, mut host) = start_host("cancel", 2_000);
    let editor_thread = fake_editor(editor_listener, state(), Duration::from_millis(500));
    let mut control = control_listener.accept().expect("accept control").0;

    write_frame(
        &mut control,
        br#"{"jsonrpc":"2.0","id":1,"method":"editor.get_view","params":{}}"#,
    )
    .expect("request");
    let cancel = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "$/cancelRequest",
        "params": { "id": 1 },
    });
    write_frame(
        &mut control,
        serde_json::to_string(&cancel)
            .expect("cancel json")
            .as_bytes(),
    )
    .expect("cancel");
    let frame = read_frame(&mut control).expect("read").expect("response");
    let response: serde_json::Value = serde_json::from_slice(&frame).expect("response json");
    assert_eq!(response["error"]["code"], -32004);

    control
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown");
    assert_eq!(host.wait().expect("wait").code(), Some(0));
    editor_thread.join().expect("editor thread");
}

#[test]
fn proxy_times_out_without_an_editor_reply() {
    let (control_listener, editor_listener, mut host) = start_host("timeout", 50);
    let editor_thread = fake_editor(editor_listener, state(), Duration::from_millis(500));
    let mut control = control_listener.accept().expect("accept control").0;

    let started = Instant::now();
    let response = rpc(&mut control, 1, "editor.get_view", serde_json::json!({}));
    assert!(started.elapsed() < Duration::from_millis(400));
    assert_eq!(response["error"]["code"], -32005);

    control
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown");
    assert_eq!(host.wait().expect("wait").code(), Some(0));
    editor_thread.join().expect("editor thread");
}

#[test]
fn host_without_editor_socket_reports_proxy_unavailable() {
    let control_path = socket_path("no-proxy");
    let _ = std::fs::remove_file(&control_path);
    let listener = UnixListener::bind(&control_path).expect("bind control");
    let mut host = spawn(
        host_bin(),
        &["--socket".to_owned(), control_path.display().to_string()],
        Token([32; 32]),
    )
    .expect("spawn host");
    let _ = host.read_hello().expect("hello");
    let mut control = listener.accept().expect("accept control").0;

    let response = rpc(&mut control, 1, "editor.get_view", serde_json::json!({}));
    assert_eq!(response["error"]["code"], -32008);

    control
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown");
    assert_eq!(host.wait().expect("wait").code(), Some(0));
}

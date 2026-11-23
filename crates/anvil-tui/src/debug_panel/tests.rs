use super::*;

#[test]
fn hit_maps_visible_rows() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.overlays.debug_panel = Some(DebugPanelState {
        threads: vec![
            DebugThread {
                id: 1,
                name: "main".to_owned(),
            },
            DebugThread {
                id: 2,
                name: "worker".to_owned(),
            },
        ],
        ..Default::default()
    });
    let area = Rect::new(0, 0, 100, 30);
    let popup = popup_rect(area, 2);
    assert_eq!(hit(&model, area, popup.x + 2, popup.y + 1), Some(0));
    assert_eq!(hit(&model, area, popup.x + 2, popup.y + 2), Some(1));
    assert_eq!(hit(&model, area, popup.x, popup.y + 1), None);
}

#[test]
fn outside_click_closes_debug_panel() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.overlays.debug_panel = Some(DebugPanelState::default());
    let click = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    crate::keys::handle_event(&mut model, crossterm::event::Event::Mouse(click));
    assert!(model.overlays.debug_panel.is_none());
}

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

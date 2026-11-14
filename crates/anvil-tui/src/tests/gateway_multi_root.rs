use super::*;
use crate::gateway::GatewaySession;
use std::time::{Duration, Instant};

fn wait_until(model: &mut Model, mut predicate: impl FnMut(&Model) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        model.poll_file_worker();
        if predicate(model) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "timeout waiting for multi-root tree; status={}",
        model.status.text
    );
}

#[test]
#[ignore = "requires ANVIL_GATEWAY"]
fn secondary_root_tree_lists_and_opens_through_owning_gateway() {
    let primary = tempfile::tempdir().expect("primary");
    let secondary = tempfile::tempdir().expect("secondary");
    std::fs::write(primary.path().join("main.rs"), "primary").expect("primary file");
    std::fs::write(secondary.path().join("lib.rs"), "secondary").expect("secondary file");
    let primary_root = primary.path().canonicalize().expect("primary canonical");
    let secondary_root = secondary
        .path()
        .canonicalize()
        .expect("secondary canonical");

    let (primary_session, primary_client, _primary_lsp) =
        GatewaySession::start(&primary_root).expect("primary gateway");
    let (secondary_session, secondary_client, _secondary_lsp) =
        GatewaySession::start(&secondary_root).expect("secondary gateway");
    let mut model = Model::with_gateway(primary_client, primary_root.clone());
    model.workspace.gateway_session = Some(primary_session);
    model
        .workspace
        .roots
        .add(&secondary_root, true)
        .expect("secondary root");
    model.workspace.root_gateways.insert(
        secondary_root.clone(),
        (secondary_session, secondary_client),
    );

    model.open_tree();
    wait_until(&mut model, |model| {
        model
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == primary_root.join("main.rs"))
            && model
                .tree
                .entries
                .iter()
                .any(|entry| entry.path == secondary_root)
    });

    let index = model
        .tree
        .entries
        .iter()
        .position(|entry| entry.path == secondary_root)
        .expect("secondary root node");
    model.tree.selected = Some(index);
    model.tree_toggle();
    wait_until(&mut model, |model| {
        model
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == secondary_root.join("lib.rs"))
    });

    model
        .open_path(secondary_root.join("lib.rs"))
        .expect("open secondary file");
    assert_eq!(model.session.editor.text().slice(0..9), "secondary");

    std::fs::write(primary_root.join("main.rs"), "needle primary").expect("primary needle");
    std::fs::write(secondary_root.join("lib.rs"), "needle secondary").expect("secondary needle");
    model.request_project_search("needle");
    wait_until(&mut model, |model| {
        model.project.search_pending.is_none() && model.overlays.location.stdin_locations.len() == 2
    });
    assert!(
        model.status.text.contains("2 matches"),
        "{}",
        model.status.text
    );
    assert!(
        model
            .overlays
            .location
            .stdin_locations
            .iter()
            .any(|location| location.path == "main.rs")
    );
    assert!(
        model
            .overlays
            .location
            .stdin_locations
            .iter()
            .any(|location| location.path == secondary_root.join("lib.rs").to_string_lossy())
    );

    model.finish_tree_create(
        secondary_root.clone(),
        "created.rs".to_owned(),
        anvil_session::ui::CreateKind::File,
    );
    assert!(secondary_root.join("created.rs").is_file());
}

#[test]
fn git_worker_routes_by_owning_root() {
    use anvil_workspace::FileWorker;

    fn serve_handshake(
        listener: anvil_ipc::Listener,
        connections: usize,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            for _ in 0..connections {
                let mut connection = listener.accept().expect("accept");
                let frame = connection.recv().expect("recv").expect("frame");
                let request: serde_json::Value = serde_json::from_slice(&frame).expect("request");
                assert_eq!(request["method"], "session.hello");
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "result": {"ok": true},
                });
                connection
                    .send(&serde_json::to_vec(&response).expect("response"))
                    .expect("send");
            }
        })
    }

    let primary = tempfile::tempdir().expect("primary");
    let secondary = tempfile::tempdir().expect("secondary");
    let primary_root = primary.path().canonicalize().expect("primary canonical");
    let secondary_root = secondary
        .path()
        .canonicalize()
        .expect("secondary canonical");
    let primary_file = primary_root.join("main.rs");
    let secondary_file = secondary_root.join("lib.rs");
    std::fs::write(&primary_file, "primary").expect("primary file");
    std::fs::write(&secondary_file, "secondary").expect("secondary file");

    let primary_socket = primary_root.join("gateway.sock");
    let secondary_socket = secondary_root.join("gateway.sock");
    let primary_listener = anvil_ipc::Listener::bind(&primary_socket).expect("primary bind");
    let secondary_listener = anvil_ipc::Listener::bind(&secondary_socket).expect("secondary bind");
    let primary_server = serve_handshake(primary_listener, 2);
    let secondary_server = serve_handshake(secondary_listener, 1);

    let primary_client =
        anvil_ipc::GatewayClient::connect(&primary_socket, &[21; 32]).expect("primary client");
    let primary_worker_client =
        anvil_ipc::GatewayClient::connect(&primary_socket, &[22; 32]).expect("primary worker");
    let secondary_client =
        anvil_ipc::GatewayClient::connect(&secondary_socket, &[23; 32]).expect("secondary client");

    let mut model = Model::with_gateway(primary_client, primary_root.clone());
    model.workspace.file_worker = Some(FileWorker::start(
        primary_worker_client,
        primary_root.clone(),
    ));
    model
        .workspace
        .roots
        .add(&secondary_root, true)
        .expect("secondary root");
    model.workspace.root_file_workers.insert(
        secondary_root.clone(),
        FileWorker::start(secondary_client, secondary_root.clone()),
    );

    let primary_worker = model
        .workspace
        .file_worker
        .as_ref()
        .expect("primary worker");
    let secondary_worker = model
        .workspace
        .root_file_workers
        .get(&secondary_root)
        .expect("secondary worker");
    assert!(std::ptr::eq(
        model
            .file_worker_for_path(&primary_file)
            .expect("primary route"),
        primary_worker
    ));
    assert!(std::ptr::eq(
        model
            .file_worker_for_path(&secondary_file)
            .expect("secondary route"),
        secondary_worker
    ));

    primary_server.join().expect("primary server");
    secondary_server.join().expect("secondary server");
}

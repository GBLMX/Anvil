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
        model.status
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
    assert_eq!(model.editor.text().slice(0..9), "secondary");

    std::fs::write(primary_root.join("main.rs"), "needle primary").expect("primary needle");
    std::fs::write(secondary_root.join("lib.rs"), "needle secondary").expect("secondary needle");
    model.request_project_search("needle");
    wait_until(&mut model, |model| {
        model.project_search_pending.is_none() && model.overlays.location.stdin_locations.len() == 2
    });
    assert!(model.status.contains("2 matches"), "{}", model.status);
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

use super::*;
use crate::workspace_roots::WorkspaceRoots;
use anvil_editor::EditorState;

#[test]
fn rejects_cross_root_mutations_and_allows_same_root() {
    let first = tempfile::tempdir().expect("first root");
    let second = tempfile::tempdir().expect("second root");
    std::fs::write(first.path().join("a.txt"), "a").expect("write");
    std::fs::write(second.path().join("a.txt"), "a").expect("write");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = first.path().to_path_buf();
    model.workspace.roots = WorkspaceRoots::new(first.path()).expect("primary root");
    model
        .workspace
        .roots
        .add(second.path(), true)
        .expect("second root");

    let error = model
        .rename_path(&first.path().join("a.txt"), &second.path().join("b.txt"))
        .expect_err("cross-root rename");
    assert!(error.contains("across workspace roots"));
    assert!(first.path().join("a.txt").exists());

    model
        .rename_path(&second.path().join("a.txt"), &second.path().join("b.txt"))
        .expect("same-root rename");
    assert!(second.path().join("b.txt").exists());
}

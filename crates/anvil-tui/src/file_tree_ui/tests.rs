use super::*;
use crate::EditorState;

#[test]
fn snapshot_keeps_expansion_missing_from_listing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    let src = workspace.join("src");
    fs::create_dir(&src).expect("src");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace.clone();
    model.tree.entries = vec![TreeEntry {
        path: src.clone(),
        depth: 0,
        is_dir: true,
        ino: 0,
        expanded: true,
    }];
    model.tree.selected = Some(0);
    model.tree.refresh_generation = 1;
    model.apply_tree_snapshot(
        1,
        vec![(
            workspace,
            Ok(vec![DirEntry {
                name: "src".to_owned(),
                is_dir: true,
                ino: 0,
            }]),
        )],
    );
    assert!(
        model
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == src && entry.expanded)
    );
}

#[test]
fn snapshot_follows_external_rename_by_inode() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace.clone();
    model.tree.entries = vec![TreeEntry {
        path: workspace.join("old.rs"),
        depth: 0,
        is_dir: false,
        ino: 42,
        expanded: false,
    }];
    model.tree.selected = Some(0);
    model.tree.refresh_generation = 1;
    model.apply_tree_snapshot(
        1,
        vec![(
            workspace.clone(),
            Ok(vec![DirEntry {
                name: "new.rs".to_owned(),
                is_dir: false,
                ino: 42,
            }]),
        )],
    );
    let selected = model.tree.selected.expect("selection");
    assert_eq!(model.tree.entries[selected].path, workspace.join("new.rs"));
}

#[test]
fn git_status_merges_primary_and_secondary_roots() {
    let dir = tempfile::tempdir().expect("tempdir");
    let primary = dir.path().join("primary.rs");
    let secondary = dir.path().join("secondary.rs");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.git.status_pending = Some(7);
    model.git.status_expected = 2;
    model.git.status_received = 0;
    model.git.status.clear();

    model.handle_git_status(
        7,
        Ok(std::collections::HashMap::from([(secondary.clone(), 'M')])),
    );
    assert_eq!(model.git.status_pending, Some(7));

    model.handle_git_status(
        7,
        Ok(std::collections::HashMap::from([(primary.clone(), 'A')])),
    );
    assert_eq!(model.git.status_pending, None);
    assert_eq!(model.git.status.get(&primary), Some(&'A'));
    assert_eq!(model.git.status.get(&secondary), Some(&'M'));
}

#[test]
fn untrusted_root_cannot_expand() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().join("primary");
    let root = dir.path().join("secondary");
    std::fs::create_dir_all(&workspace).expect("mkdir");
    std::fs::create_dir_all(&root).expect("mkdir");
    std::fs::write(root.join("secret.rs"), "").expect("write");
    let workspace = workspace.canonicalize().expect("canonical");
    let root = root.canonicalize().expect("canonical");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace;
    model.workspace.roots.add(&root, false).expect("add root");
    model.open_tree();
    let index = model
        .tree
        .entries
        .iter()
        .position(|entry| entry.path == root)
        .expect("root node");
    model.tree.selected = Some(index);
    model.tree_toggle();
    assert!(
        !model
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == root.join("secret.rs"))
    );
    assert!(model.status.text.contains("untrusted workspace root"));
    let entry = &model.tree.entries[index];
    assert!(model.tree_entry_untrusted(entry));
    assert!(model.tree_entry_label(entry).contains("[untrusted]"));
}

#[test]
fn root_labels_disambiguate_duplicate_basenames() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("a/repo");
    let second = dir.path().join("b/repo");
    let unique = dir.path().join("unique");
    for path in [&first, &second, &unique] {
        std::fs::create_dir_all(path).expect("mkdir");
    }
    let first = first.canonicalize().expect("first");
    let second = second.canonicalize().expect("second");
    let unique = unique.canonicalize().expect("unique");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = first.clone();
    model.workspace.roots = crate::workspace_roots::WorkspaceRoots::new(&first).expect("roots");
    model
        .workspace
        .roots
        .add(&second, true)
        .expect("second root");
    model
        .workspace
        .roots
        .add(&unique, true)
        .expect("unique root");

    assert_eq!(model.root_label(&first), "a/repo");
    assert_eq!(model.root_label(&second), "b/repo");
    assert_eq!(model.root_label(&unique), "unique");
}

#[test]
fn removing_a_root_cancels_pending_project_search() {
    let mut model = Model::new(EditorState::from_text(""), None);
    model.project.search_pending = Some(9);
    model.project.search_expected = 2;
    model.stop_root_file_worker(Path::new("/tmp/removed-root"));
    assert!(model.project.search_pending.is_none());
}

#[test]
fn primary_refresh_preserves_expanded_secondary_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().join("primary");
    let root = dir.path().join("secondary");
    std::fs::create_dir_all(&workspace).expect("mkdir");
    std::fs::create_dir_all(&root).expect("mkdir");
    std::fs::write(workspace.join("main.rs"), "").expect("write");
    std::fs::write(root.join("lib.rs"), "").expect("write");
    let workspace = workspace.canonicalize().expect("canonical");
    let root = root.canonicalize().expect("canonical");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace.clone();
    model.workspace.roots.add(&root, true).expect("add root");
    model.open_tree();
    let root_index = model
        .tree
        .entries
        .iter()
        .position(|entry| entry.path == root)
        .expect("root node");
    model.tree.selected = Some(root_index);
    model.tree_toggle();
    assert!(
        model
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == root.join("lib.rs"))
    );

    model.refresh_tree(None);

    assert!(model.tree.entries.iter().any(|entry| entry.path == root));
    assert!(
        model
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == root.join("lib.rs"))
    );
    assert!(
        model
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == workspace.join("main.rs"))
    );
}

#[test]
fn secondary_refresh_preserves_pending_selection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let primary = dir.path().join("primary");
    let secondary = dir.path().join("secondary");
    let src = secondary.join("src");
    std::fs::create_dir_all(&primary).expect("primary");
    std::fs::create_dir_all(&src).expect("src");
    std::fs::write(src.join("new.rs"), "").expect("new file");
    let primary = primary.canonicalize().expect("primary canonical");
    let secondary = secondary.canonicalize().expect("secondary canonical");
    let src = secondary.join("src");
    let target = src.join("new.rs");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = primary.clone();
    model.workspace.roots = crate::workspace_roots::WorkspaceRoots::new(&primary).expect("roots");
    model
        .workspace
        .roots
        .add(&secondary, true)
        .expect("secondary");
    model.tree.entries = vec![
        TreeEntry {
            path: secondary.clone(),
            depth: 0,
            is_dir: true,
            ino: 0,
            expanded: true,
        },
        TreeEntry {
            path: src,
            depth: 1,
            is_dir: true,
            ino: 0,
            expanded: true,
        },
    ];
    model.tree.pending_select = Some(target.clone());

    model.refresh_root_tree(&secondary);

    let selected = model.tree.selected.expect("selection");
    assert_eq!(model.tree.entries[selected].path, target);
}

#[test]
fn tree_watch_cap_is_reported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = dir.path().to_path_buf();
    let workspace = model.workspace.path.clone();
    model.tree.entries = (0..MAX_TREE_WATCH_DIRS + 1)
        .map(|index| TreeEntry {
            path: workspace.join(format!("d{index}")),
            depth: 0,
            is_dir: true,
            ino: 0,
            expanded: true,
        })
        .collect();
    model.sync_tree_watches();
    assert!(model.tree.watch_limited);
    assert!(model.status.text.contains("watch limit"));
}

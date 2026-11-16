use super::*;
use crate::EditorState;

#[test]
fn watched_tree_excludes_git_and_target_directories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace.clone();
    model.workspace.roots = anvil_workspace::roots::WorkspaceRoots::new(&workspace).expect("roots");
    assert!(model.watchable_tree_dir(&workspace));
    assert!(model.watchable_tree_dir(&workspace.join("src")));
    assert!(!model.watchable_tree_dir(&workspace.join(".git")));
    assert!(!model.watchable_tree_dir(&workspace.join("target/debug")));
}
#[test]
fn snapshot_keeps_expansion_missing_from_listing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    let src = workspace.join("src");
    fs::create_dir(&src).expect("src");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace.clone();
    model.workspace.tree.entries = vec![TreeEntry {
        path: src.clone(),
        depth: 0,
        is_dir: true,
        ino: 0,
        expanded: true,
    }];
    model.workspace.tree.selected = Some(0);
    model.workspace.tree.refresh_generation = 1;
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
            .workspace
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
    model.workspace.tree.entries = vec![TreeEntry {
        path: workspace.join("old.rs"),
        depth: 0,
        is_dir: false,
        ino: 42,
        expanded: false,
    }];
    model.workspace.tree.selected = Some(0);
    model.workspace.tree.refresh_generation = 1;
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
    let selected = model.workspace.tree.selected.expect("selection");
    assert_eq!(
        model.workspace.tree.entries[selected].path,
        workspace.join("new.rs")
    );
}

#[test]
fn git_status_merges_primary_and_secondary_roots() {
    let dir = tempfile::tempdir().expect("tempdir");
    let primary = dir.path().join("primary.rs");
    let secondary = dir.path().join("secondary.rs");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.git.status_pending = Some(7);
    model.workspace.git.status_expected = 2;
    model.workspace.git.status_received = 0;
    model.workspace.git.status.clear();

    model.handle_git_status(
        7,
        Ok(std::collections::HashMap::from([(secondary.clone(), 'M')])),
    );
    assert_eq!(model.workspace.git.status_pending, Some(7));

    model.handle_git_status(
        7,
        Ok(std::collections::HashMap::from([(primary.clone(), 'A')])),
    );
    assert_eq!(model.workspace.git.status_pending, None);
    assert_eq!(model.workspace.git.status.get(&primary), Some(&'A'));
    assert_eq!(model.workspace.git.status.get(&secondary), Some(&'M'));
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
        .workspace
        .tree
        .entries
        .iter()
        .position(|entry| entry.path == root)
        .expect("root node");
    model.workspace.tree.selected = Some(index);
    model.tree_toggle();
    assert!(
        !model
            .workspace
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == root.join("secret.rs"))
    );
    assert!(model.status.text.contains("untrusted workspace root"));
    let entry = &model.workspace.tree.entries[index];
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

    assert_eq!(model.workspace.root_label(&first), "a/repo");
    assert_eq!(model.workspace.root_label(&second), "b/repo");
    assert_eq!(model.workspace.root_label(&unique), "unique");
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
        .workspace
        .tree
        .entries
        .iter()
        .position(|entry| entry.path == root)
        .expect("root node");
    model.workspace.tree.selected = Some(root_index);
    model.tree_toggle();
    assert!(
        model
            .workspace
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == root.join("lib.rs"))
    );

    model.refresh_tree(None);

    assert!(
        model
            .workspace
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == root)
    );
    assert!(
        model
            .workspace
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == root.join("lib.rs"))
    );
    assert!(
        model
            .workspace
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
    model.workspace.tree.entries = vec![
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
    model.workspace.tree.pending_select = Some(target.clone());

    model.refresh_root_tree(&secondary);

    let selected = model.workspace.tree.selected.expect("selection");
    assert_eq!(model.workspace.tree.entries[selected].path, target);
}

#[test]
fn tree_watch_cap_is_reported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = dir.path().to_path_buf();
    let workspace = model.workspace.path.clone();
    model.workspace.tree.entries = (0..MAX_TREE_WATCH_DIRS + 1)
        .map(|index| TreeEntry {
            path: workspace.join(format!("d{index}")),
            depth: 0,
            is_dir: true,
            ino: 0,
            expanded: true,
        })
        .collect();
    model.sync_tree_watches();
    assert!(model.workspace.tree.watch_limited);
    assert!(model.status.text.contains("watch limit"));
}

#[test]
fn watch_status_reports_scope_and_disable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = dir.path().to_path_buf();
    model.workspace.roots = crate::workspace_roots::WorkspaceRoots::new(dir.path()).expect("roots");
    model.show_watch_status();
    assert!(model.status.text.contains("open buffers + visible tree"));
    model.config.settings.editor.file_watching = false;
    model.show_watch_status();
    assert!(
        model.status.text.starts_with("watch: off"),
        "{}",
        model.status.text
    );
    assert!(model.status.text.contains("mode=auto"));
    assert!(model.status.text.contains("max_dirs=8192"));
}

#[test]
fn natural_sort_puts_directories_first_and_numbers_numerically() {
    let mut entries = vec![
        DirEntry {
            name: "file10".to_owned(),
            is_dir: false,
            ino: 0,
        },
        DirEntry {
            name: "file2".to_owned(),
            is_dir: false,
            ino: 0,
        },
        DirEntry {
            name: "src".to_owned(),
            is_dir: true,
            ino: 0,
        },
    ];
    sort_dir_entries(&mut entries);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["src", "file2", "file10"]
    );
}

#[test]
fn fallback_listing_filters_temporary_and_ignored_entries() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    fs::write(workspace.join("file10"), "").expect("file10");
    fs::write(workspace.join("file2"), "").expect("file2");
    fs::write(workspace.join(".anvil-write-deadbeef"), "").expect("temp");
    fs::create_dir(workspace.join("target")).expect("target");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace.clone();
    model.workspace.roots = crate::workspace_roots::WorkspaceRoots::new(&workspace).expect("roots");
    let entries = model.tree_entries(&workspace).expect("listing");
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["file2", "file10"]
    );
}

#[test]
fn reveal_expands_collapsed_ancestors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    let nested = workspace.join("a/b");
    fs::create_dir_all(&nested).expect("nested");
    let target = nested.join("c.txt");
    fs::write(&target, "").expect("target");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace.clone();
    model.workspace.roots = crate::workspace_roots::WorkspaceRoots::new(&workspace).expect("roots");
    model.open_tree();
    model.reveal_tree_path(target.clone());
    assert_eq!(
        model
            .workspace
            .tree
            .selected
            .and_then(|index| model.workspace.tree.entries.get(index))
            .map(|entry| entry.path.clone()),
        Some(target)
    );
}

#[test]
fn tree_find_tracks_loaded_rows_and_caps_matches() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace.clone();
    model.workspace.roots = crate::workspace_roots::WorkspaceRoots::new(&workspace).expect("roots");
    model.workspace.tree.entries = vec![
        TreeEntry {
            path: workspace.join("main.rs"),
            depth: 0,
            is_dir: false,
            ino: 0,
            expanded: false,
        },
        TreeEntry {
            path: workspace.join("lib.rs"),
            depth: 0,
            is_dir: false,
            ino: 0,
            expanded: false,
        },
    ];
    model.ui.input_mode = InputMode::TreeFind {
        query: anvil_session::ui::InputBuffer::new("main"),
    };
    model.refresh_tree_find();
    assert_eq!(
        model
            .workspace
            .tree
            .selected
            .and_then(|index| model.workspace.tree.entries.get(index))
            .map(|entry| entry.path.clone()),
        Some(workspace.join("main.rs"))
    );
    assert_eq!(
        model
            .workspace
            .tree
            .find
            .as_ref()
            .map(|find| find.matches.len()),
        Some(1)
    );
}

#[test]
fn incremental_listing_replaces_only_the_changed_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    let src = workspace.join("src");
    let other = workspace.join("other");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace.clone();
    model.workspace.roots = crate::workspace_roots::WorkspaceRoots::new(&workspace).expect("roots");
    model.workspace.tree.entries = vec![
        TreeEntry {
            path: workspace.clone(),
            depth: 0,
            is_dir: true,
            ino: 0,
            expanded: true,
        },
        TreeEntry {
            path: src.clone(),
            depth: 1,
            is_dir: true,
            ino: 0,
            expanded: true,
        },
        TreeEntry {
            path: other.clone(),
            depth: 1,
            is_dir: true,
            ino: 0,
            expanded: false,
        },
    ];
    model.workspace.tree.refresh_generation = 3;
    model.apply_tree_listings(
        3,
        vec![(
            src.clone(),
            Ok(vec![DirEntry {
                name: "new.rs".to_owned(),
                is_dir: false,
                ino: 0,
            }]),
        )],
    );
    assert!(
        model
            .workspace
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == src.join("new.rs"))
    );
    assert!(
        model
            .workspace
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == other)
    );
}

#[test]
fn failed_incremental_listing_removes_stale_subtree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    let parent = workspace.join("parent");
    let child = parent.join("child");
    let grandchild = child.join("grandchild");
    fs::create_dir_all(&grandchild).expect("tree");
    let mut model = Model::new(EditorState::from_text(""), None);
    model.workspace.path = workspace.clone();
    model.workspace.roots = crate::workspace_roots::WorkspaceRoots::new(&workspace).expect("roots");
    model.workspace.tree.entries = vec![
        TreeEntry {
            path: parent.clone(),
            depth: 0,
            is_dir: true,
            ino: 0,
            expanded: true,
        },
        TreeEntry {
            path: child.clone(),
            depth: 1,
            is_dir: true,
            ino: 0,
            expanded: true,
        },
        TreeEntry {
            path: grandchild.clone(),
            depth: 2,
            is_dir: true,
            ino: 0,
            expanded: false,
        },
    ];
    model.workspace.tree.refresh_generation = 4;
    fs::remove_dir_all(&child).expect("remove child");
    model.apply_tree_listings(4, vec![(child.clone(), Err("gone".to_owned()))]);
    assert!(
        !model
            .workspace
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == child || entry.path == grandchild)
    );
    assert!(
        model
            .workspace
            .tree
            .entries
            .iter()
            .any(|entry| entry.path == parent)
    );
}

//! Workspace session state and dirty-buffer crash backups.

use super::*;
use anvil_editor::EditorState;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const SESSION_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct SessionFile {
    version: u32,
    workspace: PathBuf,
    active: usize,
    buffers: Vec<SessionBuffer>,
    #[serde(default)]
    split: Option<SessionBuffer>,
    #[serde(default)]
    split_focus: bool,
    split_ratio: u16,
    tree_visible: bool,
    show_hidden: bool,
    #[serde(default)]
    folded: Vec<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionBuffer {
    path: Option<PathBuf>,
    cursor: usize,
    scroll: usize,
    hscroll: usize,
    modified: bool,
    #[serde(default)]
    conflict: bool,
    swap: Option<PathBuf>,
    source_mtime_ns: Option<u64>,
}

pub(crate) fn restore(model: &mut Model, workspace: &Path) -> Result<bool, String> {
    let state_path = state_path(workspace)?;
    let data = match fs::read(&state_path) {
        Ok(data) => data,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    let state: SessionFile =
        serde_json::from_slice(&data).map_err(|error| format!("session JSON: {error}"))?;
    if state.version != SESSION_VERSION || state.workspace != workspace {
        return Ok(false);
    }
    model.conflict_swaps.clear();
    model.stale_paths.clear();
    let mut buffers = Vec::new();
    let mut conflicts = 0;
    for saved in state.buffers {
        let (buffer, conflict, conflicted) = restore_buffer(model, saved);
        if buffer.path.is_none() && !buffer.modified {
            continue;
        }
        if conflicted {
            conflicts += 1;
        }
        if let (Some(path), Some(swap)) = (buffer.path.as_ref(), conflict) {
            model.conflict_swaps.insert(path.clone(), swap);
        }
        buffers.push(buffer);
    }
    if buffers.is_empty() {
        return Ok(false);
    }
    model.buffers.entries = buffers;
    model.buffers.active = state
        .active
        .min(model.buffers.entries.len().saturating_sub(1));
    model.view.split_ratio = state.split_ratio.min(100);
    model.tree.visible = state.tree_visible;
    model.tree.show_hidden = state.show_hidden;
    let index = model.buffers.active;
    model.load_buffer(index);
    model.overlays.lsp_results.folded = state.folded.into_iter().collect();
    if let Some(saved_split) = state.split {
        let (buffer, conflict, conflicted) = restore_buffer(model, saved_split);
        if conflicted {
            conflicts += 1;
        }
        if let (Some(path), Some(swap)) = (buffer.path.as_ref(), conflict) {
            model.conflict_swaps.insert(path.clone(), swap);
        }
        model.buffers.split = Some(Pane { buffer });
        model.ui.focus.split = state.split_focus;
    }

    model.status.text = if conflicts == 0 {
        "session restored".to_owned()
    } else {
        format!("session restored with {conflicts} swap conflict(s)")
    };
    model.dirty = true;
    Ok(true)
}

fn restore_buffer(model: &mut Model, saved: SessionBuffer) -> (Buffer, Option<PathBuf>, bool) {
    let path = saved
        .path
        .as_ref()
        .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()));
    let disk_text = match &path {
        Some(path) => model.read_file(path).ok(),
        None => None,
    };
    let swap_text = saved
        .swap
        .as_ref()
        .and_then(|swap| fs::read_to_string(swap).ok());
    let changed = saved
        .source_mtime_ns
        .zip(path.as_deref().and_then(source_mtime_ns))
        .is_some_and(|(saved, current)| saved != current);
    let mut conflict_swap = None;
    let mut conflicted = false;
    let (text, modified) = if saved.modified {
        match swap_text {
            Some(swap_text) if !changed => (swap_text, true),
            other => {
                conflicted = true;
                conflict_swap = saved.swap.clone();
                (disk_text.unwrap_or(other.unwrap_or_default()), false)
            }
        }
    } else if saved.conflict {
        conflicted = true;
        conflict_swap = saved.swap.clone();
        (disk_text.unwrap_or_default(), false)
    } else {
        (disk_text.unwrap_or_default(), false)
    };
    let buffer = Buffer {
        editor: EditorState::from_text(&text).into(),
        path: path.clone(),
        cursor: saved.cursor,
        scroll: saved.scroll,
        hscroll: saved.hscroll,
        modified,
        missing: path.as_ref().is_some_and(|path| !path.is_file()),
        visual_anchor: None,
        visual_line: false,
        search_query: String::new(),
        syntax: Default::default(),
    };
    (buffer, conflict_swap, conflicted)
}

impl Model {
    pub(crate) fn clear_conflict(&mut self, path: &Path) {
        if let Some(swap) = self.conflict_swaps.remove(path) {
            let _ = fs::remove_file(swap);
        }
    }

    pub(crate) fn recover_conflict(&mut self) {
        if self.reject_read_only() {
            return;
        }
        let Some(path) = self.path.clone() else {
            self.status.text = "E: no file name".to_owned();
            self.dirty = true;
            return;
        };
        let Some(swap) = self.conflict_swaps.get(&path).cloned() else {
            self.status.text = "no swap conflict for this buffer".to_owned();
            self.dirty = true;
            return;
        };
        let text = match fs::read_to_string(&swap) {
            Ok(text) => text,
            Err(error) => {
                self.status.text = format!("E: conflict swap: {error}");
                self.dirty = true;
                return;
            }
        };
        self.editor = EditorState::from_text(&text).into();
        self.cursor = 0;
        self.view.scroll = 0;
        self.view.hscroll = 0;
        self.editor.set_selection_bytes(0, 0);
        self.modified = true;
        self.missing = false;
        self.conflict_swaps.remove(&path);
        self.syntax
            .borrow_mut()
            .switch_path(self.path.as_deref(), false);
        self.sync_lsp_active();
        self.sync_watch_paths();
        self.status.text = "conflict recovered from swap; buffer modified".to_owned();
        self.dirty = true;
    }

    pub(crate) fn discard_conflict(&mut self) {
        let Some(path) = self.path.clone() else {
            self.status.text = "E: no file name".to_owned();
            self.dirty = true;
            return;
        };
        if !self.conflict_swaps.contains_key(&path) {
            self.status.text = "no swap conflict for this buffer".to_owned();
            self.dirty = true;
            return;
        }
        self.clear_conflict(&path);
        self.status.text = "conflict discarded; current buffer kept".to_owned();
        self.dirty = true;
    }
}

pub(crate) fn save(model: &mut Model, workspace: &Path) -> Result<(), String> {
    model.sync_active();
    let state_path = state_path(workspace)?;
    let directory = state_path.parent().expect("state path parent");
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let swap_dir = directory.join("swaps");
    fs::create_dir_all(&swap_dir).map_err(|error| error.to_string())?;
    let mut buffers = if model.buffers.entries.is_empty() {
        vec![model.snapshot()]
    } else {
        model.buffers.entries.clone()
    };
    let mut saved = Vec::with_capacity(buffers.len());
    let mut kept_swaps = Vec::new();
    for (index, buffer) in buffers.drain(..).enumerate() {
        let conflict_swap = buffer
            .path
            .as_ref()
            .and_then(|path| model.conflict_swaps.get(path))
            .cloned();
        let swap = if buffer.modified {
            let path = swap_dir.join(format!("{index}.swap"));
            let text = buffer
                .editor
                .text()
                .slice(0..buffer.editor.text().len_bytes());
            write_atomic(&path, text.as_bytes()).map_err(|error| error.to_string())?;
            kept_swaps.push(path.clone());
            Some(path)
        } else {
            if let Some(path) = &conflict_swap {
                kept_swaps.push(path.clone());
            }
            conflict_swap
        };
        saved.push(SessionBuffer {
            path: buffer.path.clone(),
            cursor: buffer.cursor,
            scroll: buffer.scroll,
            hscroll: buffer.hscroll,
            modified: buffer.modified,
            conflict: !buffer.modified && swap.is_some(),
            swap,
            source_mtime_ns: buffer.path.as_deref().and_then(source_mtime_ns),
        });
    }
    let split = if let Some(pane) = model.buffers.split.as_ref() {
        let buffer = &pane.buffer;
        let conflict_swap = buffer
            .path
            .as_ref()
            .and_then(|path| model.conflict_swaps.get(path))
            .cloned();
        let swap = if buffer.modified {
            let path = swap_dir.join("split.swap");
            let text = buffer
                .editor
                .text()
                .slice(0..buffer.editor.text().len_bytes());
            write_atomic(&path, text.as_bytes()).map_err(|error| error.to_string())?;
            kept_swaps.push(path.clone());
            Some(path)
        } else {
            if let Some(path) = &conflict_swap {
                kept_swaps.push(path.clone());
            }
            conflict_swap
        };
        Some(SessionBuffer {
            path: buffer.path.clone(),
            cursor: buffer.cursor,
            scroll: buffer.scroll,
            hscroll: buffer.hscroll,
            modified: buffer.modified,
            conflict: !buffer.modified && swap.is_some(),
            swap,
            source_mtime_ns: buffer.path.as_deref().and_then(source_mtime_ns),
        })
    } else {
        None
    };
    if let Ok(entries) = fs::read_dir(&swap_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("swap")
                && !kept_swaps.contains(&path)
            {
                let _ = fs::remove_file(path);
            }
        }
    }
    let state = SessionFile {
        version: SESSION_VERSION,
        workspace: workspace.to_path_buf(),
        active: model.buffers.active,
        buffers: saved,
        split,
        split_focus: model.ui.focus.split,
        split_ratio: model.view.split_ratio,
        tree_visible: model.tree.visible,
        show_hidden: model.tree.show_hidden,
        folded: model.overlays.lsp_results.folded.iter().copied().collect(),
    };
    let data = serde_json::to_vec(&state).map_err(|error| error.to_string())?;
    write_atomic(&state_path, &data).map_err(|error| error.to_string())
}

fn state_path(workspace: &Path) -> Result<PathBuf, String> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local/state"))
        })
        .ok_or_else(|| "XDG_STATE_HOME and HOME are unavailable".to_owned())?;
    let mut hash = Sha256::new();
    hash.update(workspace.as_os_str().as_encoded_bytes());
    let key = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(root.join("anvil/sessions").join(format!("{key}.json")))
}

fn source_mtime_ns(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_nanos().min(u128::from(u64::MAX)) as u64)
}

fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "state path has no parent"))?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("state"),
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temp)?;
    file.write_all(data)?;
    file.sync_all()?;
    fs::rename(&temp, path)?;
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    use std::os::unix::fs::PermissionsExt;

    fn state_root() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn saves_and_restores_clean_buffers_and_cursors() {
        let dir = state_root();
        let _guard = ENV_LOCK.lock().expect("env lock");
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        let first = workspace.join("a.rs");
        let second = workspace.join("b.rs");
        fs::write(&first, "fn a() {}\n").expect("a");
        fs::write(&second, "fn b() {}\n").expect("b");
        unsafe { std::env::set_var("XDG_STATE_HOME", dir.path().join("state")) };
        let mut model = Model::open(&first).expect("open a");
        model.open_path(&second).expect("open b");
        model.cursor = 3;
        model.overlays.lsp_results.folded.insert(2);
        model.sync_active();
        save(&mut model, &workspace).expect("save");
        let mut restored = Model::new(EditorState::from_text(""), None);
        assert!(restore(&mut restored, &workspace).expect("restore"));
        assert_eq!(restored.buffers.entries.len(), 2);
        assert_eq!(restored.buffers.active, 1);
        assert_eq!(restored.cursor, 3);
        assert!(restored.overlays.lsp_results.folded.contains(&2));
        let state = state_path(&workspace).expect("state");
        assert_eq!(
            fs::metadata(state).expect("state").permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn dirty_swap_survives_and_detects_source_changes() {
        let dir = state_root();
        let _guard = ENV_LOCK.lock().expect("env lock");
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        let path = workspace.join("a.rs");
        fs::write(&path, "clean\n").expect("file");
        unsafe { std::env::set_var("XDG_STATE_HOME", dir.path().join("state")) };
        let mut model = Model::open(&path).expect("open");
        model.insert_text("dirty");
        save(&mut model, &workspace).expect("save");
        let mut restored = Model::new(EditorState::from_text(""), None);
        assert!(restore(&mut restored, &workspace).expect("restore"));
        assert!(restored.modified);
        assert_eq!(
            restored.editor.undo_depth(),
            0,
            "restore must start a fresh undo stack"
        );
        assert_eq!(restored.editor.text().slice(0..5), "dirty");

        fs::write(&path, "changed on disk\n").expect("changed");
        let mut conflicted = Model::new(EditorState::from_text(""), None);
        assert!(restore(&mut conflicted, &workspace).expect("restore"));
        assert!(!conflicted.modified);
        assert!(conflicted.status.text.contains("conflict"));
        let swap = conflicted
            .conflict_swaps
            .values()
            .next()
            .cloned()
            .expect("conflict swap");
        assert!(swap.exists());
        save(&mut conflicted, &workspace).expect("save conflict");
        assert!(swap.exists(), "save deleted the conflict swap");
        let mut still_conflicted = Model::new(EditorState::from_text(""), None);
        assert!(restore(&mut still_conflicted, &workspace).expect("restore"));
        assert!(still_conflicted.status.text.contains("conflict"));
        assert!(swap.exists());
    }
    #[test]
    fn restores_open_split_and_focus_with_dirty_text() {
        let dir = state_root();
        let _guard = ENV_LOCK.lock().expect("env lock");
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        let left = workspace.join("left.rs");
        let right = workspace.join("right.rs");
        fs::write(&left, "left\n").expect("left");
        fs::write(&right, "right\n").expect("right");
        unsafe { std::env::set_var("XDG_STATE_HOME", dir.path().join("state")) };
        let mut model = Model::open(&left).expect("open");
        model.split_vertical();
        model.focus_split(true);
        model.open_path(&right).expect("open right");
        model.insert_text("dirty");
        save(&mut model, &workspace).expect("save");

        let mut restored = Model::new(EditorState::from_text(""), None);
        assert!(restore(&mut restored, &workspace).expect("restore"));
        let split = restored.buffers.split.as_ref().expect("split restored");
        assert!(restored.ui.focus.split);
        assert!(restored.modified);
        assert_eq!(restored.editor.text().slice(0..5), "dirty");
        assert_eq!(restored.path.as_deref(), Some(right.as_path()));
        assert_eq!(split.buffer.path.as_deref(), Some(left.as_path()));
    }
    #[test]
    fn resolves_conflict_by_recovering_or_discarding_swap() {
        let dir = state_root();
        let _guard = ENV_LOCK.lock().expect("env lock");
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        let path = workspace.join("a.rs");
        fs::write(&path, "clean\n").expect("file");
        unsafe { std::env::set_var("XDG_STATE_HOME", dir.path().join("state")) };

        let mut model = Model::open(&path).expect("open");
        model.insert_text("dirty");
        save(&mut model, &workspace).expect("save");
        fs::write(&path, "changed on disk\n").expect("changed");

        let mut recovered = Model::new(EditorState::from_text(""), None);
        assert!(restore(&mut recovered, &workspace).expect("restore"));
        assert!(!recovered.save());
        assert_eq!(
            fs::read_to_string(&path).expect("disk"),
            "changed on disk\n"
        );
        let swap = recovered
            .conflict_swaps
            .values()
            .next()
            .cloned()
            .expect("conflict swap");
        recovered.insert_text("edited");
        let other = workspace.join("other.rs");
        assert!(!recovered.save_to(other.clone()));
        assert!(!other.exists());
        assert!(!recovered.conflict_swaps.is_empty());
        assert!(swap.exists());
        recovered.execute_command("conflict-recover");
        assert!(recovered.modified);
        assert_eq!(recovered.editor.text().slice(0..5), "dirty");
        assert!(recovered.conflict_swaps.is_empty());
        assert!(recovered.save());
        assert_eq!(fs::read_to_string(&path).expect("disk"), "dirtyclean\n");

        fs::write(&path, "changed again\n").expect("changed again");
        let mut discarded = Model::new(EditorState::from_text(""), None);
        assert!(restore(&mut discarded, &workspace).expect("restore"));
        let swap = discarded
            .conflict_swaps
            .values()
            .next()
            .cloned()
            .expect("conflict swap");
        discarded.execute_command("conflict-discard");
        assert!(!discarded.modified);
        assert!(discarded.conflict_swaps.is_empty());
        assert!(!swap.exists());
        assert_eq!(fs::read_to_string(&path).expect("disk"), "changed again\n");
    }
}

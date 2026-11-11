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
    split_ratio: u16,
    tree_visible: bool,
    show_hidden: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionBuffer {
    path: Option<PathBuf>,
    cursor: usize,
    scroll: usize,
    hscroll: usize,
    modified: bool,
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
    let mut buffers = Vec::new();
    let mut conflicts = 0;
    for (index, saved) in state.buffers.into_iter().enumerate() {
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
        let (text, modified) = if saved.modified {
            if let Some(swap_text) = swap_text {
                if changed {
                    conflicts += 1;
                    (disk_text.unwrap_or_default(), false)
                } else {
                    (swap_text, true)
                }
            } else {
                (disk_text.unwrap_or_default(), false)
            }
        } else {
            (disk_text.unwrap_or_default(), false)
        };
        if path.is_none() && !modified {
            continue;
        }
        buffers.push(Buffer {
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
        });
        let _ = index;
    }
    if buffers.is_empty() {
        return Ok(false);
    }
    model.buffers = buffers;
    model.active = state.active.min(model.buffers.len().saturating_sub(1));
    model.split_ratio = state.split_ratio.min(100);
    model.tree_visible = state.tree_visible;
    model.show_hidden = state.show_hidden;
    let index = model.active;
    model.load_buffer(index);
    model.status = if conflicts == 0 {
        "session restored".to_owned()
    } else {
        format!("session restored with {conflicts} swap conflict(s)")
    };
    model.dirty = true;
    Ok(true)
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
    let mut buffers = if model.buffers.is_empty() {
        vec![model.snapshot()]
    } else {
        model.buffers.clone()
    };
    let mut saved = Vec::with_capacity(buffers.len());
    let mut kept_swaps = Vec::new();
    for (index, buffer) in buffers.drain(..).enumerate() {
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
            None
        };
        saved.push(SessionBuffer {
            path: buffer.path.clone(),
            cursor: buffer.cursor,
            scroll: buffer.scroll,
            hscroll: buffer.hscroll,
            modified: buffer.modified,
            swap,
            source_mtime_ns: buffer.path.as_deref().and_then(source_mtime_ns),
        });
    }
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
        active: model.active,
        buffers: saved,
        split_ratio: model.split_ratio,
        tree_visible: model.tree_visible,
        show_hidden: model.show_hidden,
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
        model.sync_active();
        save(&mut model, &workspace).expect("save");
        let mut restored = Model::new(EditorState::from_text(""), None);
        assert!(restore(&mut restored, &workspace).expect("restore"));
        assert_eq!(restored.buffers.len(), 2);
        assert_eq!(restored.active, 1);
        assert_eq!(restored.cursor, 3);
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
        assert_eq!(restored.editor.text().slice(0..5), "dirty");

        fs::write(&path, "changed on disk\n").expect("changed");
        let mut conflicted = Model::new(EditorState::from_text(""), None);
        assert!(restore(&mut conflicted, &workspace).expect("restore"));
        assert!(!conflicted.modified);
        assert!(conflicted.status.contains("conflict"));
    }
}

//! Durable, auditable state for tools installed by `anvil setup apply`.

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use crate::RunError;

pub(super) struct InstalledTool {
    pub(super) language: &'static str,
    pub(super) tool: String,
    pub(super) provider: &'static str,
    pub(super) command: String,
    pub(super) path: Option<PathBuf>,
}

pub(super) fn record_installed(tools: &[InstalledTool]) -> Result<(), RunError> {
    if tools.is_empty() {
        return Ok(());
    }
    let directory = tools_directory()?;
    record_installed_at(&directory, tools)
}

fn tools_directory() -> Result<PathBuf, RunError> {
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        let data_home = data_home.to_string_lossy();
        if !data_home.trim().is_empty() {
            return Ok(PathBuf::from(data_home.into_owned()).join("anvil/tools"));
        }
    }
    let home = env::var_os("HOME")
        .ok_or_else(|| RunError::Args("cannot resolve tool state: HOME is unset".to_owned()))?;
    Ok(PathBuf::from(home).join(".local/share/anvil/tools"))
}

fn record_installed_at(directory: &Path, tools: &[InstalledTool]) -> Result<(), RunError> {
    fs::create_dir_all(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;

    let lock_path = directory.join("state.lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(&lock_path)?;
    fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o600))?;
    lock.lock().map_err(|error| {
        RunError::Io(io::Error::other(format!(
            "cannot lock {}: {error}",
            lock_path.display()
        )))
    })?;

    let state_path = directory.join("state.json");
    let mut state = match fs::read_to_string(&state_path) {
        Ok(text) => serde_json::from_str::<Value>(&text).map_err(|error| {
            RunError::Io(io::Error::new(
                ErrorKind::InvalidData,
                format!("invalid {}: {error}", state_path.display()),
            ))
        })?,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            json!({"schema_version": 1, "tools": {}})
        }
        Err(error) => return Err(error.into()),
    };
    let root = state.as_object_mut().ok_or_else(|| {
        RunError::Io(io::Error::new(
            ErrorKind::InvalidData,
            format!("{} is not a JSON object", state_path.display()),
        ))
    })?;
    root.entry("schema_version").or_insert(json!(1));
    let entries = root
        .entry("tools")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            RunError::Io(io::Error::new(
                ErrorKind::InvalidData,
                format!("{} has a non-object tools field", state_path.display()),
            ))
        })?;
    let installed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    for tool in tools {
        entries.insert(
            tool.language.to_ascii_lowercase(),
            json!({
                "language": tool.language,
                "tool": tool.tool,
                "provider": tool.provider,
                "command": tool.command,
                "path": tool.path.as_ref().map(|path| path.to_string_lossy().into_owned()),
                "installed_at": installed_at,
            }),
        );
    }

    let body = serde_json::to_vec_pretty(&state)
        .map_err(|error| RunError::Io(io::Error::new(ErrorKind::InvalidData, error.to_string())))?;
    let stage = directory.join(format!(".state-{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&stage)?;
    file.write_all(&body)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&stage, &state_path)?;
    File::open(directory)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(language: &'static str, tool: &'static str) -> InstalledTool {
        InstalledTool {
            language,
            tool: tool.to_owned(),
            provider: "npm",
            command: format!("npm install -g {tool}"),
            path: None,
        }
    }

    #[test]
    fn state_merges_tools_and_uses_private_permissions() {
        let directory = tempfile::tempdir().expect("tempdir");
        record_installed_at(directory.path(), &[sample("Rust", "rust-analyzer")])
            .expect("first state write");
        record_installed_at(
            directory.path(),
            &[sample("TypeScript", "typescript-language-server")],
        )
        .expect("second state write");

        let text = fs::read_to_string(directory.path().join("state.json")).expect("state");
        let state: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(state["schema_version"], 1);
        assert_eq!(state["tools"]["rust"]["tool"], "rust-analyzer");
        assert_eq!(
            state["tools"]["typescript"]["tool"],
            "typescript-language-server"
        );
        assert_eq!(
            fs::metadata(directory.path())
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(directory.path().join("state.json"))
                .expect("state metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

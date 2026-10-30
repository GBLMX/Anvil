//! Linux cgroup v2 placement for plugin hosts.
//!
//! The caller must provide a delegated cgroup root. Creation intentionally fails
//! closed: if the limits cannot be written, no host is started with a false
//! promise of memory/pids enforcement.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A cgroup v2 directory containing one plugin host.
pub struct Cgroup {
    path: PathBuf,
    removable: bool,
}

impl Cgroup {
    /// Creates `<root>/<name>` and writes `pids.max` and `memory.max`.
    pub fn create(
        root: impl AsRef<Path>,
        name: &str,
        pids_max: u32,
        memory_max_bytes: u64,
    ) -> io::Result<Self> {
        if name.is_empty() || name.contains('/') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid cgroup name",
            ));
        }
        let path = root.as_ref().join(name);
        fs::create_dir(&path)?;
        let cgroup = Self {
            path,
            removable: true,
        };
        cgroup.write("pids.max", &pids_max.to_string())?;
        cgroup.write("memory.max", &memory_max_bytes.to_string())?;
        Ok(cgroup)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn attach(&self, pid: u32) -> io::Result<()> {
        self.write("cgroup.procs", &pid.to_string())
    }

    fn write(&self, file: &str, value: &str) -> io::Result<()> {
        fs::write(self.path.join(file), value)
    }

    /// Removes the cgroup directory after the host has exited.
    pub fn remove(&mut self) -> io::Result<()> {
        if self.removable {
            fs::remove_dir(&self.path)?;
            self.removable = false;
        }
        Ok(())
    }
}

impl Drop for Cgroup {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal_names() {
        assert!(Cgroup::create("/tmp", "../escape", 64, 256 << 20).is_err());
        assert!(Cgroup::create("/tmp", "", 64, 256 << 20).is_err());
    }

    #[test]
    fn writes_limits_when_delegated_root_is_available() {
        let root = Path::new("/sys/fs/cgroup");
        if !root.is_dir() {
            return;
        }
        let name = format!("anvil-plugin-test-{}", std::process::id());
        let Ok(mut cgroup) = Cgroup::create(root, &name, 64, 256 << 20) else {
            return;
        };
        assert_eq!(
            fs::read_to_string(cgroup.path().join("pids.max")).expect("pids"),
            "64"
        );
        assert_eq!(
            fs::read_to_string(cgroup.path().join("memory.max")).expect("memory"),
            (256u64 << 20).to_string()
        );
        cgroup.remove().expect("remove");
    }
}

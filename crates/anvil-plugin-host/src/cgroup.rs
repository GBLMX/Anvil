//! Linux cgroup v2 placement for plugin hosts.
//!
//! The caller must provide a delegated cgroup root. Creation intentionally fails
//! closed: if the limits cannot be written, no host is started with a false
//! promise of memory/pids enforcement.

use std::fs;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
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
        let root = root.as_ref();
        if !is_cgroup2(root)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cgroup root is not cgroup v2",
            ));
        }
        let path = root.join(name);
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
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(self.path.join(file))?;
        file.write_all(value.as_bytes())
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

const CGROUP2_SUPER_MAGIC: libc::c_long = 0x6367_7270;

fn is_cgroup2(root: &Path) -> io::Result<bool> {
    let path = std::ffi::CString::new(root.as_os_str().as_bytes())?;
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(path.as_ptr(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stat.f_type == CGROUP2_SUPER_MAGIC)
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
    fn rejects_non_cgroup_root() {
        let root = std::env::temp_dir().join(format!("anvil-not-cgroup-{}", std::process::id()));
        let _ = fs::create_dir(&root);
        let name = format!("anvil-plugin-not-cgroup-{}", std::process::id());
        assert!(Cgroup::create(&root, &name, 64, 256 << 20).is_err());
        let _ = fs::remove_dir(&root);
    }

    #[test]
    fn writes_limits_when_delegated_root_is_available() {
        let root = Path::new("/sys/fs/cgroup");
        if !root.is_dir() {
            return;
        }
        let name = format!("anvil-plugin-test-{}", std::process::id());
        let Ok(mut cgroup) = Cgroup::create(root, &name, 64, 256 << 20) else {
            eprintln!("skipping cgroup limit test: no delegated writable cgroup2 root");
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

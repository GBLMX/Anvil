//! Frontend-injected plugin host process boundary.

use std::io;
use std::path::Path;

/// Fresh 32-byte capability token for one host process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostToken(pub [u8; 32]);

impl HostToken {
    pub fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or_default();
        let mut bytes = [0u8; 32];
        bytes[..8].copy_from_slice(&(std::process::id() as u64).to_le_bytes());
        bytes[8..24].copy_from_slice(&nanos.to_le_bytes());
        bytes[24..].copy_from_slice(&(nanos.rotate_left(17) as u64).to_le_bytes());
        Self(bytes)
    }
}

impl Default for HostToken {
    fn default() -> Self {
        Self::new()
    }
}

/// How a plugin host process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostExit {
    Exited(i32),
    Signaled(i32),
    OomKilled,
}

/// Session-owned view of a spawned host process.
pub trait HostProcess: Send {
    fn id(&self) -> u32;
    fn try_wait(&mut self) -> io::Result<Option<HostExit>>;
    fn terminate(&mut self);
}

/// Everything a frontend needs to launch one host process.
pub struct HostStart<'a> {
    pub plugin_id: &'a str,
    pub plugin_root: Option<&'a Path>,
    pub token: HostToken,
    pub gateway_socket: Option<&'a Path>,
    pub editor_socket: Option<&'a Path>,
    pub command: Option<&'a str>,
    pub allocate_mb: Option<u32>,
}

/// Frontend adapter that owns the concrete host binary, sandbox, and process API.
pub trait HostSpawner: Send + Sync {
    fn spawn(&self, start: &HostStart<'_>, socket: &Path) -> Result<Box<dyn HostProcess>, String>;
}

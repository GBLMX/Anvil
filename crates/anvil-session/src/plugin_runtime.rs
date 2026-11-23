//! Frontend-injected plugin host process boundary.

use std::io;
use std::path::Path;

/// Fresh 32-byte capability token for one host process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostToken(pub [u8; 32]);

impl HostToken {
    pub fn new() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("OS CSPRNG unavailable for host token");
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

#[cfg(test)]
mod tests {
    use super::HostToken;

    #[test]
    fn host_tokens_use_fresh_random_bytes() {
        let first = HostToken::new();
        let second = HostToken::new();
        assert_ne!(first.0, [0; 32]);
        assert_ne!(first, second);
    }
}

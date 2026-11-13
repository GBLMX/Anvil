//! Trusted desktop URL opener.
//!
//! The service accepts only HTTP(S) URLs and launches the fixed `xdg-open`
//! command with one argument. It never invokes a shell or accepts a
//! configurable opener.

#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anvil_module::desktop::{DesktopError, DesktopService};

pub const MODULE_ID: anvil_module::ModuleId = anvil_module::ModuleId::new("official.desktop");
pub const MAX_URL_BYTES: usize = 4096;
pub const MAX_IN_FLIGHT: usize = 4;

const DEFAULT_PROGRAM: &str = "xdg-open";

#[derive(Debug, Clone)]
pub struct SystemDesktopService {
    program: PathBuf,
    in_flight: Arc<AtomicUsize>,
}

impl Default for SystemDesktopService {
    fn default() -> Self {
        Self {
            program: PathBuf::from(DEFAULT_PROGRAM),
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl SystemDesktopService {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    fn with_program(program: PathBuf) -> Self {
        Self {
            program,
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn acquire(&self) -> Result<(), DesktopError> {
        self.in_flight
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_IN_FLIGHT).then_some(count + 1)
            })
            .map(|_| ())
            .map_err(|_| DesktopError::Unavailable("too many desktop openers".to_owned()))
    }

    fn release(&self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

impl DesktopService for SystemDesktopService {
    fn open_url(&self, url: &str) -> Result<(), DesktopError> {
        if !valid_url(url) {
            return Err(DesktopError::InvalidUrl);
        }
        self.acquire()?;
        let child = Command::new(&self.program)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match child {
            Ok(mut child) => {
                let in_flight = Arc::clone(&self.in_flight);
                std::thread::spawn(move || {
                    let _ = child.wait();
                    in_flight.fetch_sub(1, Ordering::AcqRel);
                });
                Ok(())
            }
            Err(error) => {
                self.release();
                Err(DesktopError::Spawn(error.to_string()))
            }
        }
    }
}

pub struct DesktopModule;

impl anvil_module::Module for DesktopModule {
    fn metadata(&self) -> anvil_module::ModuleMetadata {
        anvil_module::ModuleMetadata {
            id: MODULE_ID,
            name: "Official Desktop",
            contract: anvil_module::ContractRange::current(),
            commands: &[],
        }
    }

    fn register(
        &self,
        registry: &mut anvil_module::ModuleRegistry,
    ) -> Result<(), anvil_module::ModuleError> {
        registry.set_desktop_service(Arc::new(SystemDesktopService::new()))
    }
}

fn strip_scheme<'a>(url: &'a str, scheme: &str) -> Option<&'a str> {
    let prefix = url.get(..scheme.len())?;
    prefix
        .eq_ignore_ascii_case(scheme)
        .then(|| &url[scheme.len()..])
}

fn valid_url(url: &str) -> bool {
    if url.is_empty() || url.len() > MAX_URL_BYTES {
        return false;
    }
    if url
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return false;
    }
    let rest = strip_scheme(url, "https://").or_else(|| strip_scheme(url, "http://"));
    let Some(rest) = rest else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() {
        return false;
    }
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = if host_port.starts_with('[') {
        host_port
            .strip_prefix('[')
            .and_then(|value| value.split(']').next())
            .unwrap_or_default()
    } else {
        host_port.split(':').next().unwrap_or_default()
    };
    !host.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("xdg-open");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("script");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("permissions");
        path
    }

    fn open_with_retry(service: &SystemDesktopService, url: &str) -> Result<(), DesktopError> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match service.open_url(url) {
                Err(DesktopError::Spawn(error))
                    if error.contains("Text file busy") && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                result => return result,
            }
        }
    }

    fn wait_for(path: &Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(text) = fs::read_to_string(path) {
                return text;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for {}", path.display());
    }

    #[test]
    fn validates_only_bounded_http_urls() {
        assert!(valid_url("https://example.com"));
        assert!(valid_url("http://127.0.0.1:8080/a?b=c#d"));
        assert!(valid_url("https://[::1]/"));
        assert!(valid_url("HTTPS://EXAMPLE.COM/Path"));
        assert!(!valid_url(""));
        assert!(!valid_url("file:///tmp/a"));
        assert!(!valid_url("mailto:user@example.com"));
        assert!(!valid_url("https://"));
        assert!(!valid_url("https:///path"));
        assert!(!valid_url("https://example.com/a b"));
        assert!(!valid_url("https://example.com/\n"));
        assert!(!valid_url(&format!(
            "https://example.com/{}",
            "a".repeat(MAX_URL_BYTES)
        )));
    }

    #[test]
    fn passes_the_url_as_one_argument_without_shell_expansion() {
        let dir = tempfile::tempdir().expect("tempdir");
        let args = dir.path().join("xdg-open.args");
        let program = script(dir.path(), "printf '%s\\n' \"$1\" > \"$0.args\"");
        let service = SystemDesktopService::with_program(program);
        let url = "https://example.com/a?x=1&y=$HOME;echo";
        open_with_retry(&service, url).expect("open");
        assert_eq!(wait_for(&args), format!("{url}\n"));
    }

    #[test]
    fn rejects_urls_without_spawning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let args = dir.path().join("xdg-open.args");
        let program = script(dir.path(), "printf '%s\\n' \"$1\" > \"$0.args\"");
        let service = SystemDesktopService::with_program(program);
        assert_eq!(
            service.open_url("file:///tmp/secret"),
            Err(DesktopError::InvalidUrl)
        );
        std::thread::sleep(Duration::from_millis(50));
        assert!(!args.exists());
    }

    #[test]
    fn caps_in_flight_openers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = script(dir.path(), "sleep 0.4");
        let service = SystemDesktopService::with_program(program);
        for _ in 0..MAX_IN_FLIGHT {
            open_with_retry(&service, "https://example.com").expect("within cap");
        }
        assert!(matches!(
            service.open_url("https://example.com"),
            Err(DesktopError::Unavailable(_))
        ));
    }

    #[test]
    fn registers_the_desktop_service() {
        let mut registry = anvil_module::ModuleRegistry::new();
        registry.register(&DesktopModule).expect("register");
        assert!(registry.contains(MODULE_ID));
        assert!(registry.desktop_service().is_some());
    }
}

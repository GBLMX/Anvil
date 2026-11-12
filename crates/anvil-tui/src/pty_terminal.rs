//! PTY-backed shell session owned by the editor process.

use nix::{
    errno::Errno,
    libc,
    pty::openpty,
    unistd::{read, setsid, write},
};
use std::{
    io,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    path::Path,
    process::{Child, Command, Stdio},
};

pub(crate) const MAX_TERMINAL_OUTPUT: usize = 256 * 1024;

pub(crate) struct PtySession {
    master: OwnedFd,
    child: Child,
    shell: String,
}

impl PtySession {
    pub(crate) fn spawn(
        program: &str,
        workspace: &Path,
        columns: u16,
        rows: u16,
    ) -> io::Result<Self> {
        let pty = openpty(None, None).map_err(errno)?;
        let slave_fd = pty.slave.as_raw_fd();
        let mut command = Command::new(program);
        command
            .current_dir(workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                if setsid().is_err() {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(slave_fd, libc::TIOCSCTTY, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                for target in 0..3 {
                    if libc::dup2(slave_fd, target) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        drop(pty.slave);
        let flags = unsafe { libc::fcntl(pty.master.as_raw_fd(), libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe {
            libc::fcntl(
                pty.master.as_raw_fd(),
                libc::F_SETFL,
                flags | libc::O_NONBLOCK,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        let session = Self {
            master: pty.master,
            child,
            shell: program.to_owned(),
        };
        session.resize(columns, rows)?;
        Ok(session)
    }

    pub(crate) fn shell(&self) -> &str {
        &self.shell
    }

    pub(crate) fn pid(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut written = 0;
        while written < bytes.len() {
            match write(&self.master, &bytes[written..]) {
                Ok(0) => break,
                Ok(count) => written += count,
                Err(Errno::EAGAIN) => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(error) => return Err(errno(error)),
            }
        }
        Ok(())
    }

    pub(crate) fn read_available(&mut self) -> io::Result<Vec<u8>> {
        let mut output = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match read(&self.master, &mut chunk) {
                Ok(0) => break,
                Ok(count) => output.extend_from_slice(&chunk[..count]),
                Err(Errno::EAGAIN) => break,
                Err(error) => return Err(errno(error)),
            }
        }
        Ok(output)
    }

    pub(crate) fn resize(&self, columns: u16, rows: u16) -> io::Result<()> {
        let size = libc::winsize {
            ws_row: rows.max(1),
            ws_col: columns.max(1),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let result = unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &size) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    pub(crate) fn kill(&mut self) {
        if let Ok(Some(_)) = self.child.try_wait() {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.kill();
    }
}

pub(crate) fn sanitize_terminal_output(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    for next in chars.by_ref() {
                        if next == '\u{7}' {
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        match character {
            '\n' | '\t' => output.push(character),
            '\r' => {}
            character if character.is_control() => {}
            character => output.push(character),
        }
    }
    output
}

pub(crate) fn default_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "/bin/sh".to_owned())
}

fn errno(error: Errno) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn sanitizes_ansi_and_control_sequences() {
        assert_eq!(
            sanitize_terminal_output(b"a\x1b[31mred\x1b[0m\r\nb\x07"),
            "ared\nb"
        );
    }

    #[test]
    fn runs_shell_in_pty() {
        if !Path::new("/bin/sh").exists() {
            return;
        }
        let mut session =
            PtySession::spawn("/bin/sh", Path::new("/tmp"), 80, 24).expect("spawn pty shell");
        session.write_all(b"printf 'pty-ok\\n'\n").expect("write");
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut output = Vec::new();
        while Instant::now() < deadline {
            if let Ok(bytes) = session.read_available() {
                output.extend_from_slice(&bytes);
                if String::from_utf8_lossy(&output).contains("pty-ok") {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(String::from_utf8_lossy(&output).contains("pty-ok"));
        session.resize(100, 30).expect("resize");
        session.kill();
        assert!(session.try_wait().expect("wait").is_some());
    }
}

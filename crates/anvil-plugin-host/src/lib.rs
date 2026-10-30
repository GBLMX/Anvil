//! Process boundary for the empty plugin host.
//!
//! The editor side only creates a pipe, passes the token FD, starts the host in
//! its own process group, and kills the group on shutdown. Plugin code and VM
//! initialization are deliberately not present.

use std::path::Path;

/// A fresh 32-byte capability token for one host process.
#[derive(Clone, Copy)]
pub struct Token(pub [u8; 32]);

const TOKEN_FD: libc::c_int = 3;

#[cfg(target_os = "linux")]
mod linux {
    use super::{Path, TOKEN_FD, Token};
    use std::io::{self, Read};
    use std::os::fd::{FromRawFd, RawFd};
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, ExitStatus, Stdio};

    /// A running empty host. Dropping it kills the whole process group.
    pub struct HostProcess {
        child: Child,
        pgid: i32,
    }

    impl HostProcess {
        pub fn id(&self) -> u32 {
            self.child.id()
        }

        pub fn pgid(&self) -> i32 {
            self.pgid
        }

        pub fn read_hello(&mut self) -> io::Result<Vec<u8>> {
            let mut stdout = self
                .child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("host stdout was already taken"))?;
            let mut len = [0u8; 4];
            stdout.read_exact(&mut len)?;
            let len = u32::from_be_bytes(len) as usize;
            if len > 1024 * 1024 {
                return Err(io::Error::other("host frame too large"));
            }
            let mut body = vec![0; len];
            stdout.read_exact(&mut body)?;
            Ok(body)
        }

        pub fn wait(&mut self) -> io::Result<ExitStatus> {
            self.child.wait()
        }

        pub fn terminate(&mut self) {
            unsafe {
                libc::kill(-self.pgid, libc::SIGKILL);
            }
            let _ = self.child.wait();
        }
    }

    impl Drop for HostProcess {
        fn drop(&mut self) {
            self.terminate();
        }
    }

    /// Starts the host with only stdin/stdout/stderr and a token FD.
    ///
    /// All other inherited descriptors are closed in the child after `fork` and
    /// before `exec`, so gateway/editor sockets cannot leak into a plugin.
    pub fn spawn(host: &Path, args: &[String], token: Token) -> io::Result<HostProcess> {
        let mut pipe = [0; 2];
        if unsafe { libc::pipe2(pipe.as_mut_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let (read_fd, write_fd) = (pipe[0], pipe[1]);

        let mut command = Command::new(host);
        command
            .args(args)
            .arg("--token-fd")
            .arg(TOKEN_FD.to_string())
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        unsafe {
            command.pre_exec(move || {
                if libc::setpgid(0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if read_fd != TOKEN_FD && libc::dup2(read_fd, TOKEN_FD) < 0 {
                    return Err(io::Error::last_os_error());
                }
                let closed =
                    libc::syscall(libc::SYS_close_range, (TOKEN_FD + 1) as u32, u32::MAX, 0u32);
                if closed != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let child = command.spawn();
        unsafe {
            libc::close(read_fd);
        }
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                unsafe {
                    libc::close(write_fd);
                }
                return Err(error);
            }
        };
        let pgid = child.id() as i32;
        let mut writer = unsafe { std::fs::File::from_raw_fd(write_fd as RawFd) };
        use std::io::Write;
        if let Err(error) = writer.write_all(&token.0) {
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
            let _ = child.wait();
            return Err(error);
        }
        drop(writer);

        Ok(HostProcess { child, pgid })
    }
}

#[cfg(not(target_os = "linux"))]
mod linux {
    use super::{Path, Token};
    use std::io;
    use std::process::ExitStatus;

    pub struct HostProcess;

    impl HostProcess {
        pub fn id(&self) -> u32 {
            0
        }

        pub fn pgid(&self) -> i32 {
            0
        }

        pub fn read_hello(&mut self) -> io::Result<Vec<u8>> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "plugin host is Linux-only",
            ))
        }

        pub fn wait(&mut self) -> io::Result<ExitStatus> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "plugin host is Linux-only",
            ))
        }

        pub fn terminate(&mut self) {}
    }

    pub fn spawn(_host: &Path, _args: &[String], _token: Token) -> io::Result<HostProcess> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "plugin host is Linux-only",
        ))
    }
}

pub use linux::{HostProcess, spawn};

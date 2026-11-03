//! Process boundary for the empty plugin host.
//!
//! The editor side only creates a pipe, passes the token FD, starts the host in
//! its own process group, and kills the group on shutdown. Plugin code and VM
//! initialization are deliberately not present.

use std::path::Path;

#[cfg(target_os = "linux")]
pub mod manifest;

/// A fresh 32-byte capability token for one host process.
#[derive(Clone, Copy)]
pub struct Token(pub [u8; 32]);

const TOKEN_FD: libc::c_int = 3;

/// How a plugin host process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostExit {
    Exited(i32),
    Signaled(i32),
    OomKilled,
}

/// Classifies SIGKILL as OOM only when a memory limit was actually active.
pub fn classify_exit(status: std::process::ExitStatus, memory_limited: bool) -> HostExit {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(code) = status.code() {
            return HostExit::Exited(code);
        }
        if let Some(signal) = status.signal() {
            if signal == 9 && memory_limited {
                return HostExit::OomKilled;
            }
            return HostExit::Signaled(signal);
        }
    }
    HostExit::Exited(status.code().unwrap_or(-1))
}

#[cfg(target_os = "linux")]
mod cgroup;
#[cfg(target_os = "linux")]
pub use cgroup::Cgroup;

/// Non-Linux placeholder so workspace targets still compile.
#[cfg(not(target_os = "linux"))]
pub struct Cgroup;

#[cfg(target_os = "linux")]
mod linux {
    use super::{Cgroup, Path, TOKEN_FD, Token};
    use std::io::{self, Read};
    use std::os::fd::{FromRawFd, IntoRawFd, RawFd};
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, ExitStatus, Stdio};

    /// A running empty host. Dropping it kills the process and its cgroup.
    pub struct HostProcess {
        child: Child,
        pgid: i32,
        pidfd: Option<RawFd>,
        cgroup: Option<Cgroup>,
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

        pub fn memory_limited(&self) -> bool {
            self.cgroup.is_some()
        }

        pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            let status = self.child.try_wait()?;
            if status.is_some() {
                self.pgid = 0;
                self.close_pidfd();
            }
            Ok(status)
        }

        pub fn wait(&mut self) -> io::Result<ExitStatus> {
            let status = self.child.wait()?;
            self.pgid = 0;
            self.close_pidfd();
            Ok(status)
        }

        pub fn wait_classified(&mut self, memory_limited: bool) -> io::Result<super::HostExit> {
            let status = self.wait()?;
            Ok(super::classify_exit(status, memory_limited))
        }

        pub fn terminate(&mut self) {
            let cgroup_killed = self
                .cgroup
                .as_ref()
                .is_some_and(|cgroup| cgroup.kill().is_ok());
            if !cgroup_killed {
                if let Some(pidfd) = self.pidfd {
                    unsafe {
                        libc::syscall(
                            libc::SYS_pidfd_send_signal,
                            pidfd,
                            libc::SIGKILL,
                            std::ptr::null::<libc::siginfo_t>(),
                            0,
                        );
                    }
                } else if self.pgid > 0 {
                    unsafe {
                        libc::kill(-self.pgid, libc::SIGKILL);
                    }
                }
                self.pgid = 0;
            }
            let _ = self.child.wait();
            self.pgid = 0;
            self.close_pidfd();
            if let Some(mut cgroup) = self.cgroup.take() {
                let _ = cgroup.remove();
            }
        }

        fn close_pidfd(&mut self) {
            if let Some(pidfd) = self.pidfd.take() {
                unsafe {
                    libc::close(pidfd);
                }
            }
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
        let (child, pgid, pidfd) = spawn_process(host, args, token, None)?;
        Ok(HostProcess {
            child,
            pgid,
            pidfd,
            cgroup: None,
        })
    }

    /// Starts the host inside `cgroup` before `execve`.
    ///
    /// The cgroup placement is performed in `pre_exec` with raw descriptors, so
    /// the plugin never runs outside the limits.
    pub fn spawn_in_cgroup(
        host: &Path,
        args: &[String],
        token: Token,
        cgroup: Cgroup,
    ) -> io::Result<HostProcess> {
        let cgroup_fd = cgroup.procs_file()?.into_raw_fd();
        let result = spawn_process(host, args, token, Some(cgroup_fd));
        unsafe {
            libc::close(cgroup_fd);
        }
        let (child, pgid, pidfd) = result?;
        Ok(HostProcess {
            child,
            pgid,
            pidfd,
            cgroup: Some(cgroup),
        })
    }

    fn spawn_process(
        host: &Path,
        args: &[String],
        token: Token,
        cgroup_fd: Option<RawFd>,
    ) -> io::Result<(Child, i32, Option<RawFd>)> {
        let mut pipe = [0; 2];
        if unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let (read_fd, write_fd) = (pipe[0], pipe[1]);
        let parent_pid = unsafe { libc::getpid() };

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
                if libc::getppid() != parent_pid {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "parent exited before sandbox setup",
                    ));
                }
                let no_core = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                if libc::setrlimit(libc::RLIMIT_CORE, &no_core) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if let Some(fd) = cgroup_fd {
                    write_pid(fd, libc::getpid() as u32)?;
                }
                if read_fd != TOKEN_FD && libc::dup2(read_fd, TOKEN_FD) < 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::fcntl(TOKEN_FD, libc::F_SETFD, 0) < 0 {
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
        let pidfd = open_pidfd(child.id());
        let mut writer = unsafe { std::fs::File::from_raw_fd(write_fd as RawFd) };
        use std::io::Write;
        if let Err(error) = writer.write_all(&token.0) {
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
            let _ = child.wait();
            if let Some(pidfd) = pidfd {
                unsafe {
                    libc::close(pidfd);
                }
            }
            return Err(error);
        }
        drop(writer);

        Ok((child, pgid, pidfd))
    }

    fn open_pidfd(pid: u32) -> Option<RawFd> {
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        (fd >= 0).then_some(fd as RawFd)
    }

    fn write_pid(fd: RawFd, pid: u32) -> io::Result<()> {
        let mut buffer = [0u8; 16];
        let mut value = pid;
        let mut index = buffer.len();
        loop {
            index -= 1;
            buffer[index] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        let mut written = 0;
        while written < buffer.len() - index {
            let count = unsafe {
                libc::write(
                    fd,
                    buffer[index + written..].as_ptr().cast(),
                    buffer.len() - index - written,
                )
            };
            if count < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            written += count as usize;
        }
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
mod linux {
    use super::{Cgroup, Path, Token};
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

        pub fn memory_limited(&self) -> bool {
            false
        }

        pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            Ok(None)
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
        pub fn wait_classified(&mut self, _memory_limited: bool) -> io::Result<super::HostExit> {
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

    pub fn spawn_in_cgroup(
        _host: &Path,
        _args: &[String],
        _token: Token,
        _cgroup: Cgroup,
    ) -> io::Result<HostProcess> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "plugin host is Linux-only",
        ))
    }
}

pub use linux::{HostProcess, spawn, spawn_in_cgroup};

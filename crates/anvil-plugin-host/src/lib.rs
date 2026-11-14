//! Process boundary for the empty plugin host.
//!
//! The editor side only creates a pipe, passes the token FD, starts the host in
//! its own process group, and kills the group on shutdown. Plugin code and VM
//! initialization are deliberately not present.

use std::io::{self, Write};
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

/// Maximum number of bytes of untrusted plugin output forwarded to the host's
/// inherited stderr. Everything past this budget is replaced by a truncation
/// marker so a plugin cannot flood the editor terminal.
pub const MAX_PLUGIN_OUTPUT_BYTES: usize = 8 * 1024;

const TRUNCATED_MARKER: &[u8] = b"[truncated]";
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Renders `ch` as `\xNN`; every `char::is_control` character is `<= U+009F`,
/// so two hex digits are lossless.
fn escape_control(piece: &mut [u8; 4], ch: char) -> usize {
    let code = ch as u32;
    debug_assert!(code <= 0xff, "control character outside C0/C1 range");
    piece.copy_from_slice(&[
        b'\\',
        b'x',
        HEX_DIGITS[((code >> 4) & 0xf) as usize],
        HEX_DIGITS[(code & 0xf) as usize],
    ]);
    4
}

/// Writes untrusted plugin text with terminal control characters neutralized.
///
/// ESC and every other C0/C1 control character except `\n` and `\t` is
/// rendered as `\xNN`, so plugin output can never emit an ANSI/OSC sequence to
/// the terminal that inherited the host's stderr. Output is capped at
/// [`MAX_PLUGIN_OUTPUT_BYTES`] to bound terminal flooding.
pub fn write_plugin_output(out: &mut impl Write, text: &str) -> io::Result<()> {
    const CHUNK: usize = 512;
    let mut chunk = [0u8; CHUNK];
    let mut chunk_len = 0usize;
    let mut budget = MAX_PLUGIN_OUTPUT_BYTES;
    for ch in text.chars() {
        let mut piece = [0u8; 4];
        let piece_len = match ch {
            '\n' | '\t' => ch.encode_utf8(&mut piece).len(),
            ch if ch.is_control() => escape_control(&mut piece, ch),
            ch => ch.encode_utf8(&mut piece).len(),
        };
        if piece_len > budget {
            out.write_all(&chunk[..chunk_len])?;
            return out.write_all(TRUNCATED_MARKER);
        }
        if chunk_len + piece_len > CHUNK {
            out.write_all(&chunk[..chunk_len])?;
            chunk_len = 0;
        }
        chunk[chunk_len..chunk_len + piece_len].copy_from_slice(&piece[..piece_len]);
        chunk_len += piece_len;
        budget -= piece_len;
    }
    out.write_all(&chunk[..chunk_len])
}

/// Writes one escaped, capped plugin output line.
pub fn write_plugin_line(out: &mut impl Write, text: &str) -> io::Result<()> {
    write_plugin_output(out, text)?;
    out.write_all(b"\n")
}

/// Writes `prefix` (host-controlled) and escaped `text` to the host's stderr.
///
/// All stderr paths that can carry plugin-influenced bytes must go through
/// this function: `print`, `host.event` handler failures, and Lua load or
/// runtime errors surfaced by `main`.
pub fn write_plugin_stderr(prefix: &str, text: &str) {
    let mut stderr = io::stderr().lock();
    let _ = stderr.write_all(prefix.as_bytes());
    let _ = write_plugin_line(&mut stderr, text);
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

    /// Upper bound on how long [`HostProcess::terminate`] waits for a killed
    /// host to be reaped.
    const TERMINATE_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

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

        /// Kills the host and its process group or cgroup, then reaps it.
        ///
        /// SIGKILL is delivered through the pidfd when available. A failed
        /// pidfd signal falls back to the process group, so a dead pidfd cannot
        /// leave the host running. The reap is bounded: a host that does not
        /// exit within [`TERMINATE_WAIT`] is abandoned rather than blocking the
        /// caller forever.
        pub fn terminate(&mut self) {
            let cgroup_killed = self
                .cgroup
                .as_ref()
                .is_some_and(|cgroup| cgroup.kill().is_ok());
            if !cgroup_killed && !self.signal_pidfd() {
                self.signal_group();
            }
            // Clear the group id only after both kill paths ran; a pidfd
            // failure must never skip the process-group fallback.
            self.pgid = 0;
            reap_until(&mut self.child, TERMINATE_WAIT);
            self.close_pidfd();
            if let Some(mut cgroup) = self.cgroup.take() {
                let _ = cgroup.remove();
            }
        }

        /// Sends SIGKILL through the pidfd; returns `false` when there is no
        /// pidfd or the kernel rejected the signal.
        fn signal_pidfd(&mut self) -> bool {
            let Some(pidfd) = self.pidfd else {
                return false;
            };
            // SAFETY: `pidfd` is an open pidfd owned by this struct; the
            // remaining arguments are a signal number and a null `siginfo_t`,
            // which is the documented `pidfd_send_signal(2)` form.
            let result = unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    pidfd,
                    libc::SIGKILL,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            };
            result == 0
        }

        /// Sends SIGKILL to the host's process group.
        ///
        /// `pgid > 0` keeps `kill(0, ...)` (our own group) out of reach; the
        /// group id is a live pid that cannot be recycled while the child is an
        /// unreaped zombie.
        fn signal_group(&self) {
            if self.pgid > 0 {
                // SAFETY: a negative pid addresses the child's process group;
                // `pgid` is the group id created in the child (`setpgid(0,0)`).
                unsafe {
                    libc::kill(-self.pgid, libc::SIGKILL);
                }
            }
        }

        fn close_pidfd(&mut self) {
            if let Some(pidfd) = self.pidfd.take() {
                // SAFETY: `pidfd` was returned by `pidfd_open` and is removed
                // from the struct here, so it is closed exactly once.
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

    /// Reaps `child`, waiting at most `timeout` for SIGKILL to take effect.
    fn reap_until(child: &mut Child, timeout: std::time::Duration) {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) if std::time::Instant::now() >= deadline => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
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
        // SAFETY: `cgroup_fd` came from `into_raw_fd` and is used only by the
        // child's pre-exec closure, which has already run; this closes our copy.
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
        // SAFETY: `pipe2` writes exactly two descriptors into `pipe`, which is
        // sized for two `c_int`s.
        if unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let (read_fd, write_fd) = (pipe[0], pipe[1]);
        // SAFETY: `getpid` has no preconditions and no side effects.
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

        // SAFETY: the closure runs in the forked child before `exec`. It only
        // calls async-signal-safe libc primitives, performs no heap allocation
        // (failures are raw errno values, never formatted strings), and mutates
        // no shared parent state.
        unsafe {
            command.pre_exec(move || {
                if libc::setpgid(0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::getppid() != parent_pid {
                    // Allocation-free stand-in for `io::Error::new`: the child
                    // must not allocate after fork. ESRCH is also the errno a
                    // vanished parent produces.
                    return Err(io::Error::from_raw_os_error(libc::ESRCH));
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
        // SAFETY: `read_fd` is the pipe read end created above; the child holds
        // its own copy and this end is not used again.
        unsafe {
            libc::close(read_fd);
        }
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                // SAFETY: `write_fd` is the pipe write end created above and is
                // not used after the spawn failure.
                unsafe {
                    libc::close(write_fd);
                }
                return Err(error);
            }
        };
        let pgid = child.id() as i32;
        let pidfd = open_pidfd(child.id());
        // SAFETY: `write_fd` is an owned pipe descriptor; `File` takes
        // ownership and closes it exactly once on drop.
        let mut writer = unsafe { std::fs::File::from_raw_fd(write_fd) };
        use std::io::Write;
        if let Err(error) = writer.write_all(&token.0) {
            // SAFETY: `pgid` is the child's process group id; signalling it
            // requires no Rust-side invariants.
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
            reap_until(&mut child, TERMINATE_WAIT);
            if let Some(pidfd) = pidfd {
                // SAFETY: `pidfd` was opened by `open_pidfd` for this child and
                // is closed only here.
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
        // SAFETY: `pidfd_open` only reads its arguments and returns a new
        // descriptor or a negative errno.
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
            // SAFETY: `fd` is the caller's `cgroup.procs` descriptor and the
            // slice is a valid readable range inside `buffer`.
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

#[cfg(test)]
mod output_tests {
    use super::*;

    #[test]
    fn osc_sequence_is_escaped_before_output() {
        let mut captured = Vec::new();
        write_plugin_line(&mut captured, "\x1b]0;owned\x07done").expect("write");
        let text = String::from_utf8(captured).expect("utf8");
        assert!(!text.contains('\x1b'), "raw ESC reached output: {text:?}");
        assert!(!text.contains('\x07'), "raw BEL reached output: {text:?}");
        assert!(text.contains(r"\x1b]0;owned\x07done"), "{text:?}");
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn newlines_and_tabs_are_preserved() {
        let mut captured = Vec::new();
        write_plugin_line(&mut captured, "a\tb\nc\u{9c}").expect("write");
        assert_eq!(String::from_utf8(captured).expect("utf8"), "a\tb\nc\\x9c\n");
    }

    #[test]
    fn output_is_capped() {
        let huge = "a".repeat(MAX_PLUGIN_OUTPUT_BYTES * 2);
        let mut captured = Vec::new();
        write_plugin_line(&mut captured, &huge).expect("write");
        assert!(captured.len() <= MAX_PLUGIN_OUTPUT_BYTES + TRUNCATED_MARKER.len() + 1);
        assert!(String::from_utf8_lossy(&captured).ends_with("[truncated]\n"));
    }

    #[test]
    fn escape_budget_never_splits_a_character() {
        let text = format!("{}\x1b]0;x", "a".repeat(MAX_PLUGIN_OUTPUT_BYTES - 1));
        let mut captured = Vec::new();
        write_plugin_line(&mut captured, &text).expect("write");
        let output = String::from_utf8(captured).expect("utf8");
        assert!(!output.contains('\x1b'));
        assert!(output.ends_with("[truncated]\n"), "{output:?}");
    }
}

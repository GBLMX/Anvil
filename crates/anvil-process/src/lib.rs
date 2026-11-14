//! Single-owner child process groups.
//!
//! One `ProcessTask` owns its `Child`; callers can borrow stdio once, inspect
//! the pid, and shut the process group down. Dropping the task kills it.

use std::{
    io,
    path::{Path, PathBuf},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

/// How a child process should be launched.
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    program: PathBuf,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    stdin: bool,
    stdout: bool,
    stderr: bool,
    own_process_group: bool,
}

impl ProcessSpec {
    /// Create a spec for one executable.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            stdin: false,
            stdout: false,
            stderr: false,
            own_process_group: true,
        }
    }

    /// Append arguments.
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Set the working directory.
    pub fn cwd(mut self, cwd: impl AsRef<Path>) -> Self {
        self.cwd = Some(cwd.as_ref().to_path_buf());
        self
    }

    /// Pipe stdin.
    pub fn stdin(mut self, enabled: bool) -> Self {
        self.stdin = enabled;
        self
    }

    /// Pipe stdout.
    pub fn stdout(mut self, enabled: bool) -> Self {
        self.stdout = enabled;
        self
    }

    /// Pipe stderr.
    pub fn stderr(mut self, enabled: bool) -> Self {
        self.stderr = enabled;
        self
    }
    /// Control whether the child becomes a new process-group leader.
    pub fn own_process_group(mut self, enabled: bool) -> Self {
        self.own_process_group = enabled;
        self
    }
}

/// One owned child process group.
pub struct ProcessTask {
    child: Option<Child>,
    pid: u32,
    status: Option<ExitStatus>,
}

impl ProcessTask {
    /// Spawn a child in its own process group.
    pub fn spawn(spec: ProcessSpec) -> io::Result<Self> {
        Self::launch(build_command(spec))
    }

    /// Spawn with a Unix pre-exec hook, used by PTY/session setup.
    pub fn spawn_with<F>(spec: ProcessSpec, pre_exec: F) -> io::Result<Self>
    where
        F: FnMut() -> io::Result<()> + Send + Sync + 'static,
    {
        let mut command = build_command(spec);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                command.pre_exec(pre_exec);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = pre_exec;
        }
        Self::launch(command)
    }

    fn launch(mut command: Command) -> io::Result<Self> {
        let child = command.spawn()?;
        let pid = child.id();
        Ok(Self {
            child: Some(child),
            pid,
            status: None,
        })
    }

    /// Process id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Take the child's stdin pipe.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.as_mut()?.stdin.take()
    }

    /// Take the child's stdout pipe.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.as_mut()?.stdout.take()
    }

    /// Take the child's stderr pipe.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.as_mut()?.stderr.take()
    }

    /// Poll for exit without blocking.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };
        let status = child.try_wait()?;
        if let Some(status) = status {
            self.status = Some(status);
            self.child = None;
            return Ok(Some(status));
        }
        Ok(None)
    }

    /// Last exit status observed for this process.
    pub fn last_status(&self) -> Option<ExitStatus> {
        self.status
    }

    /// Terminate immediately and reap the child.
    pub fn kill(&mut self) -> io::Result<()> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        kill_group(self.pid);
        let _ = child.kill();
        if let Ok(status) = child.wait() {
            self.status = Some(status);
        }
        Ok(())
    }

    /// Ask the process group to exit, then kill it after `grace`.
    pub fn shutdown(&mut self, grace: Duration) -> io::Result<()> {
        if self.child.is_none() {
            return Ok(());
        }
        #[cfg(unix)]
        {
            use nix::{
                sys::signal::{Signal, killpg},
                unistd::Pid,
            };
            if i32::try_from(self.pid).is_ok() {
                let _ = killpg(Pid::from_raw(self.pid as i32), Signal::SIGTERM);
            }
        }
        let deadline = Instant::now() + grace;
        loop {
            let status = match self.child.as_mut() {
                Some(child) => child.try_wait()?,
                None => return Ok(()),
            };
            if let Some(status) = status {
                self.status = Some(status);
                self.child = None;
                return Ok(());
            }
            if Instant::now() >= deadline {
                return self.kill();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Signal-kill the process group led by `pid` without owning its `Child`.
///
/// Callers that own the child still reap it (for example by dropping their
/// [`ProcessTask`]); this helper exists for supervisors that need to unblock a
/// reader inside another thread.
pub fn kill_process_group(pid: u32) {
    kill_group(pid);
}

impl Drop for ProcessTask {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

fn build_command(spec: ProcessSpec) -> Command {
    let mut command = Command::new(spec.program);
    command.args(spec.args);
    if let Some(cwd) = spec.cwd {
        command.current_dir(cwd);
    }
    command.stdin(stdio(spec.stdin));
    command.stdout(stdio(spec.stdout));
    command.stderr(stdio(spec.stderr));
    #[cfg(unix)]
    if spec.own_process_group {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

fn stdio(enabled: bool) -> Stdio {
    if enabled {
        Stdio::piped()
    } else {
        Stdio::null()
    }
}

#[cfg(unix)]
fn kill_group(pid: u32) {
    use nix::{
        sys::signal::{Signal, killpg},
        unistd::Pid,
    };
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
}

#[cfg(windows)]
fn kill_group(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(any(unix, windows)))]
fn kill_group(_pid: u32) {}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn shutdown_escalates_and_reaps() {
        let mut process = ProcessTask::spawn(ProcessSpec::new("/bin/sh").args(["-c", "sleep 30"]))
            .expect("spawn");
        let started = Instant::now();
        process
            .shutdown(Duration::from_millis(1))
            .expect("shutdown");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(process.try_wait().expect("wait").is_none());
    }

    #[test]
    fn stdio_can_be_taken_once() {
        let mut process = ProcessTask::spawn(
            ProcessSpec::new("/bin/sh")
                .args(["-c", "printf hello"])
                .stdout(true),
        )
        .expect("spawn");
        assert!(process.take_stdout().is_some());
        assert!(process.take_stdout().is_none());
    }

    #[test]
    fn shutdown_timeout_kills_the_process_group() {
        use std::{io::BufRead, path::Path};

        let mut process = ProcessTask::spawn(
            ProcessSpec::new("/bin/sh")
                .args(["-c", "trap '' TERM; sleep 30 & echo $!; wait"])
                .stdout(true),
        )
        .expect("spawn");
        let stdout = process.take_stdout().expect("stdout");
        let mut line = String::new();
        std::io::BufReader::new(stdout)
            .read_line(&mut line)
            .expect("child pid");
        let child = line.trim().parse::<u32>().expect("pid");

        process
            .shutdown(Duration::from_millis(20))
            .expect("shutdown");
        let deadline = Instant::now() + Duration::from_secs(1);
        let path = format!("/proc/{child}");
        while Path::new(&path).exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!Path::new(&path).exists(), "background child survived");
        assert!(process.last_status().is_some());
    }
}

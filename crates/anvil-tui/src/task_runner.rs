//! Bounded background task execution and compiler-style problem parsing.

use regex::Regex;
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

pub(crate) const MAX_TASK_OUTPUT: usize = 256 * 1024;
pub(crate) const MAX_TASK_PROBLEMS: usize = 500;
pub(crate) const TASK_TIMEOUT: Duration = Duration::from_secs(300);
const READ_CHUNK: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskProblem {
    pub(crate) path: PathBuf,
    pub(crate) line: u32,
    pub(crate) character: u32,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskResult {
    pub(crate) command: String,
    pub(crate) exit_code: Option<i32>,
    pub(crate) output: String,
    pub(crate) truncated: bool,
    pub(crate) timed_out: bool,
    pub(crate) problems: Vec<TaskProblem>,
}

pub(crate) enum TaskEvent {
    Finished(TaskResult),
}

pub(crate) struct TaskWorker {
    receiver: Receiver<TaskEvent>,
}

impl TaskWorker {
    pub(crate) fn start(command: String, workspace: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = run_task(&command, &workspace);
            let _ = sender.send(TaskEvent::Finished(result));
        });
        Self { receiver }
    }

    pub(crate) fn try_recv(&self) -> Option<TaskEvent> {
        self.receiver.try_recv().ok()
    }
}

fn run_task(command: &str, workspace: &Path) -> TaskResult {
    run_task_with_timeout(command, workspace, TASK_TIMEOUT)
}

fn run_task_with_timeout(command: &str, workspace: &Path, timeout: Duration) -> TaskResult {
    let mut process = shell_command(command);
    process
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(error) => {
            return TaskResult {
                command: command.to_owned(),
                exit_code: None,
                output: format!("task spawn failed: {error}"),
                truncated: false,
                timed_out: false,
                problems: Vec::new(),
            };
        }
    };
    let stdout = child.stdout.take().map(spawn_reader);
    let stderr = child.stderr.take().map(spawn_reader);
    let task_id = child.id();
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            kill_task_group(task_id);
            timed_out = true;
            break child.wait().ok();
        }
        thread::sleep(Duration::from_millis(20));
    };
    kill_task_group(task_id);
    let (stdout, stdout_truncated) = join_reader(stdout);
    let (stderr, stderr_truncated) = join_reader(stderr);
    let mut output = String::from_utf8_lossy(&stdout).into_owned();
    output.push_str(&String::from_utf8_lossy(&stderr));
    let mut truncated = stdout_truncated || stderr_truncated;
    if output.len() > MAX_TASK_OUTPUT {
        output.truncate(MAX_TASK_OUTPUT);
        truncated = true;
    }
    let problems = parse_problems(&output, workspace);
    TaskResult {
        command: command.to_owned(),
        exit_code: status.and_then(|status| status.code()),
        output,
        truncated,
        timed_out,
        problems,
    }
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut process = Command::new("cmd");
        process.args(["/C", command]);
        process
    }
    #[cfg(not(windows))]
    {
        let mut process = Command::new("sh");
        process.args(["-c", command]);
        process
    }
}

#[cfg(unix)]
fn kill_task_group(pid: u32) {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    let _ = kill(Pid::from_raw(-pid), Signal::SIGKILL);
}

#[cfg(windows)]
fn kill_task_group(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(any(unix, windows)))]
fn kill_task_group(_pid: u32) {}

fn spawn_reader<R>(mut reader: R) -> thread::JoinHandle<(Vec<u8>, bool)>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut captured = Vec::new();
        let mut chunk = [0u8; READ_CHUNK];
        let mut truncated = false;
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if captured.len() < MAX_TASK_OUTPUT {
                        let remaining = MAX_TASK_OUTPUT - captured.len();
                        let take = remaining.min(read);
                        captured.extend_from_slice(&chunk[..take]);
                        if take < read {
                            truncated = true;
                        }
                    } else {
                        truncated = true;
                    }
                }
            }
        }
        (captured, truncated)
    })
}

fn join_reader(handle: Option<thread::JoinHandle<(Vec<u8>, bool)>>) -> (Vec<u8>, bool) {
    handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

fn parse_problems(output: &str, workspace: &Path) -> Vec<TaskProblem> {
    let pattern = match Regex::new(r"^(.+?):(\d+)(?::(\d+))?:\s*(.+)$") {
        Ok(pattern) => pattern,
        Err(_) => return Vec::new(),
    };
    let mut problems = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in output.lines() {
        if problems.len() >= MAX_TASK_PROBLEMS {
            break;
        }
        let Some(captures) = pattern.captures(line.trim()) else {
            continue;
        };
        let character = captures
            .get(3)
            .and_then(|capture| capture.as_str().parse::<u32>().ok())
            .unwrap_or(1);
        let (Some(path), Ok(line_number), Some(message)) =
            (captures.get(1), captures[2].parse::<u32>(), captures.get(4))
        else {
            continue;
        };
        let path = resolve_path(workspace, path.as_str());
        let message = message.as_str().trim().to_owned();
        if message.is_empty() {
            continue;
        }
        let key = (path.clone(), line_number, character, message.clone());
        if !seen.insert(key) {
            continue;
        }
        problems.push(TaskProblem {
            path,
            line: line_number.saturating_sub(1),
            character: character.saturating_sub(1),
            message,
        });
    }
    problems
}

fn resolve_path(workspace: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compiler_style_problems() {
        let workspace = Path::new("/tmp/work");
        let problems = parse_problems(
            "src/main.rs:12:4: expected expression\nerror: not a source location\n",
            workspace,
        );
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].path, PathBuf::from("/tmp/work/src/main.rs"));
        assert_eq!(problems[0].line, 11);
        assert_eq!(problems[0].character, 3);
        assert_eq!(problems[0].message, "expected expression");
    }

    #[test]
    fn parses_line_only_go_and_pytest_failures() {
        let workspace = Path::new("/tmp/work");
        let problems = parse_problems(
            "foo_test.go:12: expected 1\ntests/test_x.py:10: AssertionError\n",
            workspace,
        );
        assert_eq!(problems.len(), 2);
        assert_eq!(problems[0].path, PathBuf::from("/tmp/work/foo_test.go"));
        assert_eq!(problems[0].line, 11);
        assert_eq!(problems[0].character, 0);
        assert_eq!(problems[1].path, PathBuf::from("/tmp/work/tests/test_x.py"));
        assert_eq!(problems[1].line, 9);
        assert_eq!(problems[1].character, 0);
    }

    #[cfg(unix)]
    #[test]
    fn bounded_output_and_exit_code() {
        let result = run_task("printf 'a.rs:1:2: broken\\n'; exit 3", Path::new("/tmp"));
        assert_eq!(result.exit_code, Some(3));
        assert!(result.output.contains("a.rs:1:2: broken"));
        assert_eq!(result.problems.len(), 1);
        assert!(!result.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn truncates_large_output() {
        let result = run_task("head -c 1048576 /dev/zero | tr '\\0' x", Path::new("/tmp"));
        assert!(result.truncated);
        assert_eq!(result.output.len(), MAX_TASK_OUTPUT);
    }
}

#[cfg(test)]
mod process_tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn kills_background_children_after_shell_exit() {
        let started = Instant::now();
        let result = run_task_with_timeout("sleep 30 &", Path::new("/tmp"), Duration::from_secs(2));
        assert!(!result.timed_out);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn times_out_and_kills_process_group() {
        let started = Instant::now();
        let result =
            run_task_with_timeout("sleep 30", Path::new("/tmp"), Duration::from_millis(100));
        assert!(result.timed_out);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

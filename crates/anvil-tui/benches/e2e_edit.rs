use std::{
    ffi::CString,
    fs::{self, File},
    io::{self, Read, Write},
    os::fd::AsFd,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anvil_tui::{Model, view};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use nix::{
    poll::{PollFd, PollFlags, PollTimeout, poll},
    pty::{ForkptyResult, Winsize, forkpty},
    sys::{
        signal::{Signal, kill},
        wait::waitpid,
    },
    unistd::{Pid, execvp},
};
use ratatui::{Terminal, backend::TestBackend};

const ROWS: u16 = 40;
const COLS: u16 = 120;
const SCROLL_SAMPLES: usize = 20;
const SEARCH_MARKERS: usize = 11;

fn main() {
    let temp = tempfile::tempdir().expect("temp dir");
    let small_path = temp.path().join("e2e-small.txt");
    let large_path = temp.path().join("e2e-100mb.txt");
    fs::write(&small_path, make_text(1, 0)).expect("write small fixture");
    fs::write(&large_path, make_text(100, SEARCH_MARKERS)).expect("write large fixture");

    rust_side(&small_path, &large_path);
    pty_side(temp.path(), &small_path, &large_path);
}

fn rust_side(small_path: &Path, large_path: &Path) {
    let mut key_times = Vec::new();
    let mut model = Model::open(small_path).expect("open small");
    let mut terminal = test_terminal();
    warm(&mut model, &mut terminal);
    for _ in 0..SCROLL_SAMPLES {
        let start = Instant::now();
        dispatch_key(&mut model, KeyCode::Char('j'));
        model.ensure_cursor_visible_for_bench(ROWS - 2, COLS - 2);
        draw(&mut terminal, &model);
        key_times.push(start.elapsed());
    }
    report("rust", "key_to_screen", &key_times);

    let start = Instant::now();
    let mut large = Model::open(large_path).expect("open 100MB");
    let mut terminal = test_terminal();
    large.ensure_cursor_visible_for_bench(ROWS - 2, COLS - 2);
    draw(&mut terminal, &large);
    report("rust", "open_100MB_first_frame", &[start.elapsed()]);

    let mut scroll_times = Vec::new();
    for _ in 0..SCROLL_SAMPLES {
        let start = Instant::now();
        dispatch_key(&mut large, KeyCode::Char('j'));
        large.ensure_cursor_visible_for_bench(ROWS - 2, COLS - 2);
        draw(&mut terminal, &large);
        scroll_times.push(start.elapsed());
    }
    report("rust", "scroll_100MB", &scroll_times);

    let mut search_first = None;
    let mut search_times = Vec::new();
    for index in 0..SEARCH_MARKERS {
        dispatch_key(&mut large, KeyCode::Char('/'));
        for character in format!("SEARCH_{index}").chars() {
            dispatch_key(&mut large, KeyCode::Char(character));
        }
        let start = Instant::now();
        dispatch_key(&mut large, KeyCode::Enter);
        large.ensure_cursor_visible_for_bench(ROWS - 2, COLS - 2);
        draw(&mut terminal, &large);
        let elapsed = start.elapsed();
        if index == 0 {
            search_first = Some(elapsed);
        } else {
            search_times.push(elapsed);
        }
    }
    if let Some(first) = search_first {
        report("rust", "search_100MB_first", &[first]);
    }
    report("rust", "search_100MB", &search_times);
}

fn pty_side(workspace: &Path, small_path: &Path, large_path: &Path) {
    let bin = anvil_bin();
    let marker = large_path
        .file_name()
        .expect("file name")
        .as_encoded_bytes()
        .to_vec();
    let mut startup_times = Vec::new();
    {
        let start = Instant::now();
        let mut session = PtySession::spawn(&bin, workspace, large_path).expect("spawn 100MB");
        session
            .wait_for(&marker, Duration::from_secs(30))
            .expect("100MB first frame");
        startup_times.push(start.elapsed());
    }
    report("pty", "open_100MB_first_frame", &startup_times);

    let mut key_times = Vec::new();
    {
        let mut session = PtySession::spawn(&bin, workspace, small_path).expect("spawn small");
        session
            .wait_for(
                small_path
                    .file_name()
                    .expect("file name")
                    .as_encoded_bytes(),
                Duration::from_secs(10),
            )
            .expect("small first frame");
        session.send(b"i").expect("send insert");
        session.drain();
        for character in "甲乙丙丁戊己庚辛壬癸子丑寅卯辰巳午未申酉".chars() {
            let bytes = character.to_string().into_bytes();
            let start = Instant::now();
            session.send(&bytes).expect("send key");
            session
                .wait_for_output(Duration::from_secs(10))
                .expect("key frame");
            key_times.push(start.elapsed());
        }
    }
    report("pty", "key_to_screen", &key_times);

    let mut scroll_times = Vec::new();
    {
        let mut session = PtySession::spawn(&bin, workspace, large_path).expect("spawn 100MB");
        session
            .wait_for(&marker, Duration::from_secs(30))
            .expect("100MB first frame");
        for _ in 0..50 {
            session.send(b"j").expect("send warmup j");
        }
        session.drain();
        for _ in 0..SCROLL_SAMPLES {
            let start = Instant::now();
            session.send(b"j").expect("send j");
            session
                .wait_for_output(Duration::from_secs(10))
                .expect("scroll frame");
            scroll_times.push(start.elapsed());
        }
    }
    report("pty", "scroll_100MB", &scroll_times);

    let mut search_first = None;
    let mut search_times = Vec::new();
    {
        let mut session = PtySession::spawn(&bin, workspace, large_path).expect("spawn 100MB");
        session
            .wait_for(&marker, Duration::from_secs(30))
            .expect("100MB first frame");
        for index in 0..SEARCH_MARKERS {
            let query = format!("SEARCH_{index}");
            session.send(b"/").expect("send search");
            session.send(query.as_bytes()).expect("send query");
            session.drain();
            let start = Instant::now();
            session.send(b"\r").expect("send enter");
            session
                .wait_for_output(Duration::from_secs(30))
                .expect("search frame");
            let elapsed = start.elapsed();
            if index == 0 {
                search_first = Some(elapsed);
            } else {
                search_times.push(elapsed);
            }
        }
    }
    if let Some(first) = search_first {
        report("pty", "search_100MB_first", &[first]);
    }
    report("pty", "search_100MB", &search_times);
}

fn test_terminal() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(COLS, ROWS)).expect("test terminal")
}

fn warm(model: &mut Model, terminal: &mut Terminal<TestBackend>) {
    for _ in 0..5 {
        dispatch_key(model, KeyCode::Char('j'));
        model.ensure_cursor_visible_for_bench(ROWS - 2, COLS - 2);
        draw(terminal, model);
    }
}

fn draw(terminal: &mut Terminal<TestBackend>, model: &Model) {
    terminal.draw(|frame| view(model, frame)).expect("draw");
}

fn dispatch_key(model: &mut Model, code: KeyCode) {
    model.dispatch(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn make_text(megabytes: usize, markers: usize) -> String {
    let target = megabytes * 1024 * 1024;
    let mut text = String::with_capacity(target + 9 * markers);
    let mut line = 0_usize;
    while text.len() + 9 * markers < target {
        text.push_str(&format!("{line:08}\n"));
        line += 1;
    }
    for index in 0..markers {
        text.push_str(&format!("SEARCH_{index}\n"));
    }
    text
}

fn anvil_bin() -> PathBuf {
    let release = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/release/anvil");
    if release.exists() {
        return release;
    }
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_anvil") {
        return PathBuf::from(path);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/anvil")
}

fn report(layer: &str, scenario: &str, samples: &[Duration]) {
    let mut micros = samples
        .iter()
        .map(|duration| duration.as_secs_f64() * 1_000_000.0)
        .collect::<Vec<_>>();
    micros.sort_by(f64::total_cmp);
    let p50 = percentile(&micros, 50);
    let p95 = percentile(&micros, 95);
    let p99 = percentile(&micros, 99);
    println!(
        "{layer},{scenario},p50={p50:.1}us,p95={p95:.1}us,p99={p99:.1}us,n={}",
        micros.len()
    );
}

fn percentile(sorted: &[f64], percentile: usize) -> f64 {
    sorted[(sorted.len() * percentile / 100).min(sorted.len() - 1)]
}

struct PtySession {
    master: File,
    child: Pid,
}

impl PtySession {
    fn spawn(bin: &Path, workspace: &Path, file: &Path) -> io::Result<Self> {
        let winsize = Winsize {
            ws_row: ROWS,
            ws_col: COLS,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let result = unsafe { forkpty(Some(&winsize), None) }
            .map_err(|error| io::Error::other(error.to_string()))?;
        match result {
            ForkptyResult::Child => {
                let bin = CString::new(bin.as_os_str().as_encoded_bytes())
                    .expect("binary path contains NUL");
                let workspace = CString::new(workspace.as_os_str().as_encoded_bytes())
                    .expect("workspace path contains NUL");
                let file = CString::new(file.as_os_str().as_encoded_bytes())
                    .expect("file path contains NUL");
                let workspace_flag = CString::new("--workspace").expect("literal");
                let args = [bin.clone(), workspace_flag, workspace, file];
                let _ = execvp(&bin, &args);
                std::process::exit(127);
            }
            ForkptyResult::Parent { child, master } => Ok(Self {
                master: File::from(master),
                child,
            }),
        }
    }

    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.master.write_all(bytes)
    }

    fn wait_for_output(&mut self, timeout: Duration) -> io::Result<()> {
        let start = Instant::now();
        let mut buffer = [0_u8; 8192];
        while start.elapsed() < timeout {
            let mut fds = [PollFd::new(self.master.as_fd(), PollFlags::POLLIN)];
            let ready = poll(&mut fds, PollTimeout::from(50_u16))
                .map_err(|error| io::Error::other(error.to_string()))?;
            if ready > 0 && self.master.read(&mut buffer)? > 0 {
                return Ok(());
            }
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "PTY produced no output",
        ))
    }

    fn drain(&mut self) {
        let mut buffer = [0_u8; 8192];
        loop {
            let mut fds = [PollFd::new(self.master.as_fd(), PollFlags::POLLIN)];
            match poll(&mut fds, PollTimeout::from(10_u16)) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if self.master.read(&mut buffer).unwrap_or(0) == 0 {
                        break;
                    }
                }
            }
        }
    }

    fn wait_for(&mut self, needle: &[u8], timeout: Duration) -> io::Result<()> {
        let start = Instant::now();
        let mut output = Vec::new();
        let mut buffer = [0_u8; 8192];
        while start.elapsed() < timeout {
            let mut fds = [PollFd::new(self.master.as_fd(), PollFlags::POLLIN)];
            let ready = poll(&mut fds, PollTimeout::from(50_u16))
                .map_err(|error| io::Error::other(error.to_string()))?;
            if ready == 0 {
                continue;
            }
            let read = self.master.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            output.extend_from_slice(&buffer[..read]);
            if output.windows(needle.len()).any(|window| window == needle) {
                return Ok(());
            }
        }
        let tail = String::from_utf8_lossy(&output[output.len().saturating_sub(400)..]);
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("PTY output did not contain {needle:?}; tail={tail:?}"),
        ))
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = kill(self.child, Signal::SIGTERM);
        let _ = waitpid(self.child, None);
    }
}

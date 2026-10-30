#![cfg(target_os = "linux")]

use anvil_plugin_host::{Token, spawn};
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn host_bin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_anvil-plugin-host"))
}

#[test]
fn host_reads_token_and_sends_hello() {
    let mut host = spawn(host_bin(), &[], Token([1; 32])).expect("spawn host");
    let hello = host.read_hello().expect("hello");
    let hello = String::from_utf8(hello).expect("utf8");
    assert!(hello.contains("session.hello"), "{hello}");
    assert!(hello.contains("\"session_id\":\"empty\""), "{hello}");
    assert_eq!(host.wait().expect("wait").code(), Some(0));
}

#[test]
fn host_closes_all_but_stdio() {
    let mut host =
        spawn(host_bin(), &["--check-fds".to_owned()], Token([3; 32])).expect("spawn host");
    assert_eq!(host.wait().expect("wait").code(), Some(0));
}

#[test]
fn host_allows_unix_sockets() {
    let mut host =
        spawn(host_bin(), &["--check-uds".to_owned()], Token([7; 32])).expect("spawn host");
    assert_eq!(host.wait().expect("wait").code(), Some(23));
}

#[test]
fn host_denies_network() {
    let mut host =
        spawn(host_bin(), &["--check-network".to_owned()], Token([4; 32])).expect("spawn host");
    assert_eq!(host.wait().expect("wait").code(), Some(14));
}

#[test]
fn editor_uds_eof_exits_host() {
    let path = std::env::temp_dir().join(format!("anvil-uds-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("bind uds");
    let mut host = spawn(
        host_bin(),
        &["--socket".to_owned(), path.display().to_string()],
        Token([8; 32]),
    )
    .expect("spawn host");
    let _ = host.read_hello().expect("hello");
    let editor = listener.accept().expect("accept").0;
    editor
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown");
    let deadline = Instant::now() + Duration::from_millis(500);
    while host.try_wait().expect("try wait").is_none() {
        if Instant::now() >= deadline {
            panic!("host did not exit after editor EOF");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn landlock_denies_write() {
    let mut host =
        spawn(host_bin(), &["--check-write".to_owned()], Token([6; 32])).expect("spawn host");
    assert_eq!(host.wait().expect("wait").code(), Some(20));
}

#[test]
fn landlock_denies_exec() {
    let mut host =
        spawn(host_bin(), &["--check-exec".to_owned()], Token([5; 32])).expect("spawn host");
    assert_eq!(host.wait().expect("wait").code(), Some(18));
}

#[test]
fn landlock_denies_etc_passwd() {
    let mut host = spawn(
        host_bin(),
        &["--check-read".to_owned(), "/etc/passwd".to_owned()],
        Token([2; 32]),
    )
    .expect("spawn host");
    assert_eq!(
        host.wait().expect("wait").code(),
        Some(11),
        "Landlock must return EACCES for /etc/passwd"
    );
}

#[test]
fn editor_kill_9_reaps_held_host() {
    let mut editor = Command::new(env!("CARGO_BIN_EXE_anvil-orphan-editor"))
        .env("ANVIL_HOST_BIN", host_bin())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn orphan editor");
    let stdout = editor.stdout.take().expect("stdout");
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .expect("host pid");
    let host_pid = line.trim().parse::<i32>().expect("numeric host pid");

    unsafe {
        libc::kill(editor.id() as i32, libc::SIGKILL);
    }
    let _ = editor.wait();

    let deadline = Instant::now() + Duration::from_secs(3);
    while Path::new(&format!("/proc/{host_pid}")).exists() {
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(host_pid, libc::SIGKILL);
            }
            panic!("host {host_pid} survived editor SIGKILL");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

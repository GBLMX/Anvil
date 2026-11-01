#![cfg(target_os = "linux")]

use anvil_ipc::write_frame;
use anvil_plugin_host::{Token, spawn};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
fn host_loads_validated_plugin_entry() {
    let root = std::env::temp_dir().join(format!(
        "anvil-plugin-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("plugin root");
    std::fs::write(root.join("run.luau"), "return 42").expect("entry");
    std::fs::write(
        root.join("plugin.json"),
        br#"{"id":"demo","version":"0.1.0","entry":"run.luau","activation_events":["onStartup"],"commands":[{"id":"demo.run","title":"Run"}]}"#,
    )
    .expect("manifest");
    let mut host = spawn(
        host_bin(),
        &["--plugin".to_owned(), root.display().to_string()],
        Token([21; 32]),
    )
    .expect("spawn host");
    let hello = host.read_hello().expect("hello");
    assert!(!hello.is_empty());
    assert_eq!(host.wait().expect("wait").code(), Some(0));
    let _ = std::fs::remove_dir_all(root);
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
fn host_answers_sys_ping_and_rejects_sys_exec() {
    let path = std::env::temp_dir().join(format!("anvil-rpc-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("bind uds");
    let mut host = spawn(
        host_bin(),
        &["--socket".to_owned(), path.display().to_string()],
        Token([12; 32]),
    )
    .expect("spawn host");
    let _ = host.read_hello().expect("hello");
    let mut editor = listener.accept().expect("accept").0;

    write_frame(
        &mut editor,
        br#"{"jsonrpc":"2.0","id":1,"method":"sys.ping"}"#,
    )
    .expect("ping");
    let response = anvil_ipc::read_frame(&mut editor)
        .expect("read")
        .expect("response");
    let response: serde_json::Value = serde_json::from_slice(&response).expect("json");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"], "pong");

    write_frame(
        &mut editor,
        br#"{"jsonrpc":"2.0","id":2,"method":"sys.exec"}"#,
    )
    .expect("exec");
    let response = anvil_ipc::read_frame(&mut editor)
        .expect("read")
        .expect("response");
    let response: serde_json::Value = serde_json::from_slice(&response).expect("json");
    assert_eq!(response["error"]["code"], -32008);

    editor
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown");
    assert_eq!(host.wait().expect("wait").code(), Some(0));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn host_buffers_split_frames_across_reads() {
    let path = std::env::temp_dir().join(format!("anvil-split-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("bind uds");
    let mut host = spawn(
        host_bin(),
        &["--socket".to_owned(), path.display().to_string()],
        Token([13; 32]),
    )
    .expect("spawn host");
    let _ = host.read_hello().expect("hello");
    let mut editor = listener.accept().expect("accept").0;
    let body = br#"{"jsonrpc":"2.0","id":3,"method":"sys.ping"}"#;
    let mut frame = (body.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(body);
    editor.write_all(&frame[..2]).expect("partial header");
    std::thread::sleep(Duration::from_millis(150));
    editor.write_all(&frame[2..]).expect("remainder");
    let response = anvil_ipc::read_frame(&mut editor)
        .expect("read")
        .expect("response");
    let response: serde_json::Value = serde_json::from_slice(&response).expect("json");
    assert_eq!(response["result"], "pong");
    editor
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown");
    assert_eq!(host.wait().expect("wait").code(), Some(0));
    let _ = std::fs::remove_file(&path);
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
fn host_can_join_delegated_cgroup() {
    let name = format!("anvil-plugin-test-{}", std::process::id());
    let Ok(mut cgroup) = anvil_plugin_host::Cgroup::create("/sys/fs/cgroup", &name, 64, 256 << 20)
    else {
        return;
    };
    let mut host = spawn(host_bin(), &["--hold".to_owned()], Token([11; 32])).expect("spawn host");
    host.attach_cgroup(&cgroup).expect("attach");
    unsafe {
        libc::kill(host.id() as i32, libc::SIGKILL);
    }
    let _ = host.wait();
    cgroup.remove().expect("remove cgroup");
}

#[test]
fn sigkill_is_oom_only_with_memory_limit() {
    let mut limited =
        spawn(host_bin(), &["--hold".to_owned()], Token([9; 32])).expect("spawn limited");
    unsafe {
        libc::kill(limited.id() as i32, libc::SIGKILL);
    }
    assert_eq!(
        limited.wait_classified(true).expect("wait"),
        anvil_plugin_host::HostExit::OomKilled
    );

    let mut unlimited =
        spawn(host_bin(), &["--hold".to_owned()], Token([10; 32])).expect("spawn unlimited");
    unsafe {
        libc::kill(unlimited.id() as i32, libc::SIGKILL);
    }
    assert_eq!(
        unlimited.wait_classified(false).expect("wait"),
        anvil_plugin_host::HostExit::Signaled(libc::SIGKILL)
    );
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

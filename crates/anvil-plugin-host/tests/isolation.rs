#![cfg(target_os = "linux")]

use anvil_ipc::{read_frame, write_frame};
use anvil_plugin_host::{Cgroup, Token, spawn, spawn_in_cgroup};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
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
    assert!(hello.contains("\"api_version\":\"1.0.0\""), "{hello}");
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
fn host_denies_unix_sockets() {
    let mut host =
        spawn(host_bin(), &["--check-uds".to_owned()], Token([7; 32])).expect("spawn host");
    assert_eq!(host.wait().expect("wait").code(), Some(24));
}

#[test]
fn host_denies_hardening_bypasses() {
    let mut host = spawn(
        host_bin(),
        &["--check-hardening".to_owned()],
        Token([8; 32]),
    )
    .expect("spawn host");
    assert_eq!(host.wait().expect("wait").code(), Some(0));
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
fn host_forwards_sys_exec_to_gateway() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let gateway_path = std::env::temp_dir().join(format!(
        "anvil-gateway-rpc-{}-{suffix}.sock",
        std::process::id()
    ));
    let editor_path = std::env::temp_dir().join(format!(
        "anvil-editor-rpc-{}-{suffix}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&gateway_path);
    let _ = std::fs::remove_file(&editor_path);
    let gateway = UnixListener::bind(&gateway_path).expect("bind gateway");
    let editor_listener = UnixListener::bind(&editor_path).expect("bind editor");
    let gateway_thread = thread::spawn(move || {
        let (mut connection, _) = gateway.accept().expect("accept gateway");
        let hello = read_frame(&mut connection)
            .expect("read hello")
            .expect("hello frame");
        let hello: serde_json::Value = serde_json::from_slice(&hello).expect("hello json");
        write_frame(
            &mut connection,
            serde_json::to_string(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": hello["id"],
                "result": { "plugin_id": "demo" }
            }))
            .expect("hello response")
            .as_bytes(),
        )
        .expect("write hello");
        let request = read_frame(&mut connection)
            .expect("read exec")
            .expect("exec frame");
        let request: serde_json::Value = serde_json::from_slice(&request).expect("exec json");
        assert_eq!(request["method"], "sys.exec");
        assert_eq!(request["params"]["tool"], "true");
        write_frame(
            &mut connection,
            serde_json::to_string(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": { "ok": true }
            }))
            .expect("exec response")
            .as_bytes(),
        )
        .expect("write exec");
    });

    let mut host = spawn(
        host_bin(),
        &[
            "--socket".to_owned(),
            editor_path.display().to_string(),
            "--gateway-socket".to_owned(),
            gateway_path.display().to_string(),
        ],
        Token([14; 32]),
    )
    .expect("spawn host");
    let _ = host.read_hello().expect("hello");
    let mut editor = editor_listener.accept().expect("accept editor").0;
    write_frame(
        &mut editor,
        br#"{"jsonrpc":"2.0","id":9,"method":"sys.exec","params":{"tool":"true","args":[],"timeout_ms":1000}}"#,
    )
    .expect("exec");
    let response = read_frame(&mut editor).expect("read").expect("response");
    let response: serde_json::Value = serde_json::from_slice(&response).expect("response json");
    assert_eq!(response["result"]["ok"], true);

    editor
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown editor");
    assert_eq!(host.wait().expect("wait").code(), Some(0));
    gateway_thread.join().expect("gateway thread");
    let _ = std::fs::remove_file(gateway_path);
    let _ = std::fs::remove_file(editor_path);
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
    let Ok(root) = Cgroup::detect_root() else {
        return;
    };
    let Ok(cgroup) = Cgroup::create(&root, &name, 64, 256 << 20, 100) else {
        return;
    };
    let cgroup_path = cgroup.path().to_path_buf();
    let mut host = spawn_in_cgroup(host_bin(), &["--hold".to_owned()], Token([11; 32]), cgroup)
        .expect("spawn host");
    let membership =
        std::fs::read_to_string(format!("/proc/{}/cgroup", host.id())).expect("read host cgroup");
    assert!(
        membership.contains(cgroup_path.file_name().unwrap().to_str().unwrap()),
        "{membership}"
    );
    host.terminate();
    assert!(!cgroup_path.exists(), "cgroup was not removed");
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

fn plugin_root(entry: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "anvil-plugin-output-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("plugin root");
    std::fs::write(root.join("run.luau"), entry).expect("entry");
    std::fs::write(
        root.join("plugin.json"),
        br#"{"id":"demo","version":"0.1.0","entry":"run.luau","activation_events":["onStartup"],"commands":[{"id":"demo.run","title":"Run"}]}"#,
    )
    .expect("manifest");
    root
}

/// Host process with a captured stderr so plugin output can be audited.
struct CapturedHost {
    child: std::process::Child,
    stderr: std::process::ChildStderr,
}

impl CapturedHost {
    fn spawn(args: &[String], token: [u8; 32]) -> Self {
        use std::os::fd::FromRawFd;
        use std::os::unix::process::CommandExt;

        let mut pipe = [0; 2];
        // SAFETY: `pipe2` writes exactly two descriptors into `pipe`.
        assert_eq!(
            unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) },
            0
        );
        let (read_fd, write_fd) = (pipe[0], pipe[1]);
        // Fill the pipe before spawning; the host may read and exit immediately
        // after exec, so writing after `spawn` can race with its fd close.
        // SAFETY: `write_fd` is an owned pipe descriptor; `File` owns it now.
        let mut writer = unsafe { std::fs::File::from_raw_fd(write_fd) };
        writer.write_all(&token).expect("write token");
        drop(writer);
        let mut command = Command::new(host_bin());
        command
            .args(args)
            .arg("--token-fd")
            .arg("3")
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        // SAFETY: the closure runs in the forked child before exec and only
        // calls async-signal-safe `dup2` / `fcntl`.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(read_fd, 3) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // `read_fd` may already be 3; dup2(3, 3) is a no-op and leaves
                // the pipe's O_CLOEXEC flag set, which would close the token fd
                // before exec.
                if libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().expect("spawn host");
        // SAFETY: the child inherited `read_fd`; the parent closes its copy.
        unsafe {
            libc::close(read_fd);
        }
        let stderr = child.stderr.take().expect("stderr");
        Self { child, stderr }
    }

    fn finish(mut self) -> (std::process::ExitStatus, String) {
        let status = self.child.wait().expect("wait host");
        let mut stderr = String::new();
        self.stderr
            .read_to_string(&mut stderr)
            .expect("read stderr");
        (status, stderr)
    }
}

fn assert_no_raw_controls(stderr: &str) {
    assert!(
        !stderr.contains('\u{1b}'),
        "raw ESC reached stderr: {stderr:?}"
    );
    assert!(
        !stderr.contains('\u{7}'),
        "raw BEL reached stderr: {stderr:?}"
    );
}

#[test]
fn plugin_print_cannot_emit_osc_to_stderr() {
    let root = plugin_root(
        r#"print("marker " .. string.char(27) .. "]0;owned" .. string.char(7) .. " end")"#,
    );
    let args = ["--plugin".to_owned(), root.display().to_string()];
    let (status, stderr) = CapturedHost::spawn(&args, [42; 32]).finish();
    let _ = std::fs::remove_dir_all(root);
    assert!(status.success(), "host failed: {stderr:?}");
    assert_no_raw_controls(&stderr);
    assert!(stderr.contains(r"\x1b]0;owned\x07"), "{stderr:?}");
}

#[test]
fn plugin_load_error_cannot_emit_osc_to_stderr() {
    let root = plugin_root(
        r#"error("boom " .. string.char(27) .. "]0;owned" .. string.char(7) .. " end")"#,
    );
    let args = ["--plugin".to_owned(), root.display().to_string()];
    let (status, stderr) = CapturedHost::spawn(&args, [43; 32]).finish();
    let _ = std::fs::remove_dir_all(root);
    assert!(!status.success(), "plugin load should fail");
    assert!(stderr.contains("plugin-host: "), "{stderr:?}");
    assert_no_raw_controls(&stderr);
    assert!(stderr.contains(r"\x1b]0;owned\x07"), "{stderr:?}");
}

#[test]
fn host_event_handler_error_cannot_emit_osc_to_stderr() {
    let socket = std::env::temp_dir().join(format!(
        "anvil-event-esc-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).expect("bind uds");
    let root = plugin_root(
        r#"__anvil_event_handlers[7] = function()
    error("boom " .. string.char(27) .. "]0;owned" .. string.char(7) .. " end")
end"#,
    );
    let args = [
        "--plugin".to_owned(),
        root.display().to_string(),
        "--socket".to_owned(),
        socket.display().to_string(),
    ];
    let host = CapturedHost::spawn(&args, [44; 32]);
    let mut editor = listener.accept().expect("accept editor").0;
    write_frame(
        &mut editor,
        br#"{"jsonrpc":"2.0","method":"host.event","params":{"subscription_id":7,"sequence":1,"event":"test","payload":{}}}"#,
    )
    .expect("write event");
    editor
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown editor");
    let (status, stderr) = host.finish();
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_file(&socket);
    assert!(status.success(), "host failed: {stderr:?}");
    assert!(stderr.contains("host.event: "), "{stderr:?}");
    assert_no_raw_controls(&stderr);
    assert!(stderr.contains(r"\x1b]0;owned\x07"), "{stderr:?}");
}

#[test]
fn terminate_kills_held_host_without_blocking() {
    let mut host = spawn(host_bin(), &["--hold".to_owned()], Token([16; 32])).expect("spawn host");
    let pid = host.id();
    let started = Instant::now();
    host.terminate();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "terminate blocked for {:?}",
        started.elapsed()
    );
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "host {pid} survived terminate"
    );
}

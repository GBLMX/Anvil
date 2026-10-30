use std::io::{self, Write};
use std::process::ExitCode;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    pub fn main() -> ExitCode {
        match run() {
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                let _ = writeln!(io::stderr(), "plugin-host: {error}");
                ExitCode::FAILURE
            }
        }
    }

    fn run() -> io::Result<u8> {
        let mut token_fd = None;
        let mut check_read = None;
        let mut hold = false;
        let mut check_fds = false;
        let mut check_network = false;
        let mut check_exec = false;
        let mut check_write = false;
        let mut check_uds = false;
        let mut socket = None;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--token-fd" => {
                    let value = args
                        .next()
                        .ok_or_else(|| io::Error::other("missing --token-fd value"))?;
                    token_fd = Some(
                        value
                            .parse::<i32>()
                            .map_err(|_| io::Error::other("invalid --token-fd"))?,
                    );
                }
                "--check-read" => {
                    check_read = Some(
                        args.next()
                            .ok_or_else(|| io::Error::other("missing --check-read value"))?,
                    );
                }
                "--hold" => hold = true,
                "--check-fds" => check_fds = true,
                "--check-network" => check_network = true,
                "--check-exec" => check_exec = true,
                "--check-write" => check_write = true,
                "--check-uds" => check_uds = true,
                "--socket" => {
                    socket = Some(
                        args.next()
                            .ok_or_else(|| io::Error::other("missing --socket value"))?,
                    );
                }
                other => return Err(io::Error::other(format!("unknown argument: {other}"))),
            }
        }
        let token_fd = token_fd.ok_or_else(|| io::Error::other("--token-fd is required"))?;

        let _token = read_token(token_fd)?;
        close_extra_fds()?;
        if check_fds {
            let count = std::fs::read_dir("/proc/self/fd")?.count();
            if count != 4 {
                let _ = writeln!(io::stderr(), "unexpected fd count: {count}");
                return Ok(13);
            }
            return Ok(0);
        }
        unsafe {
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        apply_landlock()?;
        apply_seccomp()?;

        if let Some(path) = check_read {
            match std::fs::File::open(&path) {
                Ok(_) => {
                    let _ = writeln!(io::stdout(), "read unexpectedly allowed: {path}");
                    return Ok(10);
                }
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return Ok(11),
                Err(error) => {
                    let _ = writeln!(io::stderr(), "read failed unexpectedly: {error}");
                    return Ok(12);
                }
            }
        }

        if check_write {
            match std::fs::File::create("/tmp/anvil-plugin-host-write-denied") {
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return Ok(20),
                Ok(_) => return Ok(21),
                Err(_) => return Ok(22),
            }
        }

        if check_exec {
            match std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg("exit 0")
                .status()
            {
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return Ok(18),
                Ok(_) => return Ok(17),
                Err(_) => return Ok(19),
            }
        }

        if check_uds {
            match std::os::unix::net::UnixStream::pair() {
                Ok(_) => return Ok(23),
                Err(error) if error.raw_os_error() == Some(libc::EPERM) => return Ok(24),
                Err(_) => return Ok(25),
            }
        }

        if check_network {
            match std::net::TcpStream::connect("127.0.0.1:80") {
                Err(error) if error.raw_os_error() == Some(libc::EPERM) => return Ok(14),
                Ok(_) => return Ok(15),
                Err(_) => return Ok(16),
            }
        }

        send_hello()?;
        if let Some(path) = socket {
            watch_editor(&path)?;
            return Ok(0);
        }
        if hold {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
        Ok(0)
    }

    fn read_token(fd: i32) -> io::Result<[u8; 32]> {
        let mut token = [0u8; 32];
        let mut read = 0;
        while read < token.len() {
            let count =
                unsafe { libc::read(fd, token[read..].as_mut_ptr().cast(), token.len() - read) };
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "token pipe closed early",
                ));
            }
            if count < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            read += count as usize;
        }
        unsafe {
            libc::close(fd);
        }
        Ok(token)
    }

    fn close_extra_fds() -> io::Result<()> {
        let result = unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    const ACCESS_READ_FILE: u64 = 1 << 2;
    const ACCESS_READ_DIR: u64 = 1 << 3;
    const ACCESS_REFER: u64 = 1 << 13;
    const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;
    const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1;

    #[repr(C)]
    struct RulesetAttr {
        handled_access_fs: u64,
    }

    #[repr(C)]
    struct PathBeneathAttr {
        allowed_access: u64,
        parent_fd: i32,
    }

    fn apply_landlock() -> io::Result<()> {
        let abi = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<RulesetAttr>(),
                0usize,
                LANDLOCK_CREATE_RULESET_VERSION,
            )
        };
        if abi < 1 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Landlock is unavailable",
            ));
        }

        let mut handled = (1 << 0)
            | ACCESS_READ_FILE
            | ACCESS_READ_DIR
            | (1 << 1)
            | (1 << 4)
            | (1 << 5)
            | (1 << 6)
            | (1 << 7)
            | (1 << 8)
            | (1 << 9)
            | (1 << 10)
            | (1 << 11)
            | (1 << 12);
        if abi >= 2 {
            handled |= ACCESS_REFER;
        }
        if abi >= 3 {
            handled |= 1 << 14;
        }
        if abi >= 5 {
            handled |= 1 << 15;
        }
        let attr = RulesetAttr {
            handled_access_fs: handled,
        };
        let ruleset = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                &attr,
                std::mem::size_of::<RulesetAttr>(),
                0u32,
            )
        };
        if ruleset < 0 {
            return Err(io::Error::last_os_error());
        }
        let ruleset = ruleset as i32;

        for (path, allowed) in [
            ("/usr", ACCESS_READ_FILE | ACCESS_READ_DIR),
            ("/lib", ACCESS_READ_FILE | ACCESS_READ_DIR),
            ("/lib64", ACCESS_READ_FILE | ACCESS_READ_DIR),
            ("/etc/ld.so.cache", ACCESS_READ_FILE),
        ] {
            let c_path = std::ffi::CString::new(path)?;
            let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
            if fd < 0 {
                continue;
            }
            let rule = PathBeneathAttr {
                allowed_access: allowed,
                parent_fd: fd,
            };
            let added = unsafe {
                libc::syscall(
                    libc::SYS_landlock_add_rule,
                    ruleset,
                    LANDLOCK_RULE_PATH_BENEATH,
                    &rule,
                    0u32,
                )
            };
            unsafe {
                libc::close(fd);
            }
            if added != 0 {
                unsafe {
                    libc::close(ruleset);
                }
                return Err(io::Error::last_os_error());
            }
        }

        let restricted = unsafe { libc::syscall(libc::SYS_landlock_restrict_self, ruleset, 0u32) };
        unsafe {
            libc::close(ruleset);
        }
        if restricted != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[repr(C)]
    struct SockFilter {
        code: u16,
        jt: u8,
        jf: u8,
        k: u32,
    }

    #[repr(C)]
    struct SockFprog {
        len: u16,
        filter: *const SockFilter,
    }

    const BPF_LD: u16 = 0x00;
    const BPF_W: u16 = 0x00;
    const BPF_ABS: u16 = 0x20;
    const BPF_JMP: u16 = 0x05;
    const BPF_JEQ: u16 = 0x10;
    const BPF_JSET: u16 = 0x40;
    const BPF_K: u16 = 0x00;
    const BPF_RET: u16 = 0x06;
    const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
    const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;

    fn apply_seccomp() -> io::Result<()> {
        const NR: u32 = 0;
        const ARCH: u32 = 4;
        const ARG0: u32 = 16;
        #[cfg(target_arch = "x86_64")]
        const NATIVE_ARCH: u32 = 0xC000_003E;
        #[cfg(target_arch = "aarch64")]
        const NATIVE_ARCH: u32 = 0xC000_00B7;
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        const NATIVE_ARCH: u32 = 0;
        let filter = [
            SockFilter {
                code: BPF_LD | BPF_W | BPF_ABS,
                jt: 0,
                jf: 0,
                k: ARCH,
            },
            SockFilter {
                code: BPF_JMP | BPF_JEQ | BPF_K,
                jt: 1,
                jf: 0,
                k: NATIVE_ARCH,
            },
            SockFilter {
                code: BPF_RET | BPF_K,
                jt: 0,
                jf: 0,
                k: SECCOMP_RET_ERRNO | libc::EPERM as u32,
            },
            SockFilter {
                code: BPF_LD | BPF_W | BPF_ABS,
                jt: 0,
                jf: 0,
                k: NR,
            },
            SockFilter {
                code: BPF_JMP | BPF_JSET | BPF_K,
                jt: 0,
                jf: 1,
                k: 0x4000_0000,
            },
            SockFilter {
                code: BPF_RET | BPF_K,
                jt: 0,
                jf: 0,
                k: SECCOMP_RET_ERRNO | libc::EPERM as u32,
            },
            SockFilter {
                code: BPF_JMP | BPF_JEQ | BPF_K,
                jt: 0,
                jf: 3,
                k: libc::SYS_socket as u32,
            },
            SockFilter {
                code: BPF_LD | BPF_W | BPF_ABS,
                jt: 0,
                jf: 0,
                k: ARG0,
            },
            SockFilter {
                code: BPF_JMP | BPF_JEQ | BPF_K,
                jt: 1,
                jf: 0,
                k: libc::AF_UNIX as u32,
            },
            SockFilter {
                code: BPF_RET | BPF_K,
                jt: 0,
                jf: 0,
                k: SECCOMP_RET_ERRNO | libc::EPERM as u32,
            },
            SockFilter {
                code: BPF_JMP | BPF_JEQ | BPF_K,
                jt: 0,
                jf: 3,
                k: libc::SYS_socketpair as u32,
            },
            SockFilter {
                code: BPF_LD | BPF_W | BPF_ABS,
                jt: 0,
                jf: 0,
                k: ARG0,
            },
            SockFilter {
                code: BPF_JMP | BPF_JEQ | BPF_K,
                jt: 1,
                jf: 0,
                k: libc::AF_UNIX as u32,
            },
            SockFilter {
                code: BPF_RET | BPF_K,
                jt: 0,
                jf: 0,
                k: SECCOMP_RET_ERRNO | libc::EPERM as u32,
            },
            SockFilter {
                code: BPF_RET | BPF_K,
                jt: 0,
                jf: 0,
                k: SECCOMP_RET_ALLOW,
            },
        ];
        let program = SockFprog {
            len: filter.len() as u16,
            filter: filter.as_ptr(),
        };
        let result =
            unsafe { libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &program) };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn watch_editor(path: &str) -> io::Result<()> {
        let mut connection = anvil_ipc::Connection::connect(path)?;
        loop {
            match connection.recv() {
                Ok(None) => return Ok(()),
                Ok(Some(frame)) => handle_rpc(&mut connection, &frame)?,
                Err(error) => {
                    return Err(io::Error::other(format!("editor frame: {error}")));
                }
            }
        }
    }

    fn handle_rpc(connection: &mut anvil_ipc::Connection, frame: &[u8]) -> io::Result<()> {
        let Ok(request) = serde_json::from_slice::<serde_json::Value>(frame) else {
            return Ok(());
        };
        let id = request
            .get("id")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let response = match request.get("method").and_then(|value| value.as_str()) {
            Some("sys.ping") => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": "pong"
            }),
            Some("sys.exec") => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32008,
                    "message": "sys.exec is not implemented yet"
                }
            }),
            _ => return Ok(()),
        };
        let body = serde_json::to_vec(&response)
            .map_err(|error| io::Error::other(format!("encode rpc response: {error}")))?;
        connection
            .send(&body)
            .map_err(|error| io::Error::other(format!("send rpc response: {error}")))
    }

    fn send_hello() -> io::Result<()> {
        let body = br#"{"jsonrpc":"2.0","method":"session.hello","params":{"session_id":"empty","protocol":"anvil-ipc/1"}}"#;
        let mut stdout = io::stdout().lock();
        stdout.write_all(&(body.len() as u32).to_be_bytes())?;
        stdout.write_all(body)?;
        stdout.flush()
    }
}

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    linux::main()
}

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("plugin-host is Linux-only");
    ExitCode::FAILURE
}

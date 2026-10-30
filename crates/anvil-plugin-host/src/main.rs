use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
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

    send_hello()?;
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

    let mut handled = ACCESS_READ_FILE
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

fn send_hello() -> io::Result<()> {
    let body = br#"{"jsonrpc":"2.0","method":"session.hello","params":{"session_id":"empty","protocol":"anvil-ipc/1"}}"#;
    let mut stdout = io::stdout().lock();
    stdout.write_all(&(body.len() as u32).to_be_bytes())?;
    stdout.write_all(body)?;
    stdout.flush()
}

use std::io::{self, Write};
use std::process::ExitCode;

#[cfg(target_os = "linux")]
mod runtime;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use anvil_plugin_host::Token;
    use anvil_plugin_host::manifest::PluginManifest;
    use std::time::Duration;

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
        let mut allocate_mb = 0u32;
        let mut check_network = false;
        let mut check_exec = false;
        let mut check_write = false;
        let mut check_uds = false;
        let mut check_hardening = false;
        let mut lua_smoke = false;
        let mut plugin = None;
        let mut socket = None;
        let mut editor_socket = None;
        let mut editor_timeout_ms = 2_000u64;
        let mut gateway_socket = None;
        let mut activation_command = None;
        let mut plugin_id = "unknown".to_owned();
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
                "--allocate-mb" => {
                    let value = args
                        .next()
                        .ok_or_else(|| io::Error::other("missing --allocate-mb value"))?;
                    allocate_mb = value
                        .parse()
                        .map_err(|_| io::Error::other("invalid --allocate-mb"))?;
                }
                "--check-network" => check_network = true,
                "--check-exec" => check_exec = true,
                "--check-write" => check_write = true,
                "--check-uds" => check_uds = true,
                "--check-hardening" => check_hardening = true,
                "--lua-smoke" => lua_smoke = true,
                "--plugin" => {
                    plugin = Some(
                        args.next()
                            .ok_or_else(|| io::Error::other("missing --plugin value"))?,
                    );
                }
                "--plugin-id" => {
                    plugin_id = args
                        .next()
                        .ok_or_else(|| io::Error::other("missing --plugin-id value"))?;
                }
                "--socket" => {
                    socket = Some(
                        args.next()
                            .ok_or_else(|| io::Error::other("missing --socket value"))?,
                    );
                }
                "--gateway-socket" => {
                    gateway_socket = Some(
                        args.next()
                            .ok_or_else(|| io::Error::other("missing --gateway-socket value"))?,
                    );
                }
                "--editor-socket" => {
                    editor_socket = Some(
                        args.next()
                            .ok_or_else(|| io::Error::other("missing --editor-socket value"))?,
                    );
                }
                "--editor-timeout-ms" => {
                    let value = args
                        .next()
                        .ok_or_else(|| io::Error::other("missing --editor-timeout-ms value"))?;
                    editor_timeout_ms = value
                        .parse()
                        .map_err(|_| io::Error::other("invalid --editor-timeout-ms"))?;
                }
                "--activation-command" => {
                    activation_command =
                        Some(args.next().ok_or_else(|| {
                            io::Error::other("missing --activation-command value")
                        })?);
                }
                other => return Err(io::Error::other(format!("unknown argument: {other}"))),
            }
        }
        let plugin_root = plugin.map(std::fs::canonicalize).transpose()?;
        let token_fd = token_fd.ok_or_else(|| io::Error::other("--token-fd is required"))?;

        let token = read_token(token_fd)?;
        close_extra_fds()?;
        if check_fds {
            let count = std::fs::read_dir("/proc/self/fd")?.count();
            if count != 4 {
                let _ = writeln!(io::stderr(), "unexpected fd count: {count}");
                return Ok(13);
            }
            return Ok(0);
        }
        let mut gateway = match gateway_socket.as_deref() {
            Some(path) => Some(
                anvil_ipc::GatewayClient::connect(path, &token)
                    .map_err(|error| io::Error::other(error.to_string()))?,
            ),
            None => None,
        };
        let editor = match socket.as_deref() {
            Some(path) => Some(anvil_ipc::Connection::connect(path)?),
            None => None,
        };
        let mut editor_proxy = match editor_socket.as_deref() {
            Some(path) => Some(EditorProxy::connect(
                path,
                editor_timeout_ms,
                &plugin_id,
                Token(token),
            )?),
            None => None,
        };
        let mut editor_bridge = match editor_socket.as_deref() {
            Some(path) => Some(runtime::EditorClient::connect(
                std::path::Path::new(path),
                &plugin_id,
                Token(token),
            )?),
            None => None,
        };
        unsafe {
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::prctl(libc::PR_SET_DUMPABLE, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        apply_landlock(plugin_root.as_deref())?;
        apply_seccomp()?;
        if check_hardening {
            return Ok(run_hardening_checks());
        }
        if lua_smoke {
            match runtime::smoke() {
                Ok(42) => return Ok(0),
                Ok(_) => return Ok(31),
                Err(error) => {
                    let _ = writeln!(io::stderr(), "luau smoke failed: {error}");
                    return Ok(32);
                }
            }
        }
        let lua = if let Some(root) = plugin_root.as_deref() {
            let manifest = PluginManifest::load(root).map_err(io::Error::other)?;
            let entry = manifest.entry_path(root).map_err(io::Error::other)?;
            Some(
                runtime::run_file(&entry, editor_bridge.take(), activation_command.as_deref())
                    .map_err(|error| io::Error::other(error.to_string()))?,
            )
        } else {
            None
        };

        if allocate_mb > 0 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let bytes = (allocate_mb as usize).saturating_mul(1024 * 1024);
            let memory = vec![1u8; bytes];
            std::hint::black_box(&memory);
            // The cgroup memory.max path must remain the only caller of this
            // probe. Keep the pages resident until the kernel kills us.
            std::thread::sleep(std::time::Duration::from_secs(60));
        }

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
        if let Some(mut connection) = editor {
            watch_editor(
                &mut connection,
                gateway.as_mut(),
                editor_proxy.as_mut(),
                lua.as_ref(),
            )?;
            return Ok(0);
        }
        if hold {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
        Ok(0)
    }

    fn run_hardening_checks() -> u8 {
        let denied = |result: libc::c_int| {
            result == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
        };
        if !denied(unsafe { libc::kill(libc::getpid(), 0) }) {
            return 40;
        }
        if !denied(unsafe { libc::fcntl(0, libc::F_SETOWN, libc::getpid()) }) {
            return 41;
        }
        if !denied(unsafe { libc::fcntl(0, libc::F_SETFL, libc::O_ASYNC) }) {
            return 42;
        }
        if !denied(unsafe { libc::ioctl(0, libc::FIOASYNC, std::ptr::null::<u8>()) }) {
            return 43;
        }
        if !denied(unsafe {
            libc::syscall(libc::SYS_memfd_create, c"anvil".as_ptr(), 0) as libc::c_int
        }) {
            return 44;
        }
        if !denied(unsafe {
            libc::mount(
                c"none".as_ptr(),
                c"/tmp".as_ptr(),
                c"tmpfs".as_ptr(),
                0,
                std::ptr::null(),
            )
        }) {
            return 45;
        }
        if !denied(unsafe { libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0) as libc::c_int }) {
            return 46;
        }
        if !denied(unsafe { libc::unshare(libc::CLONE_NEWNS) }) {
            return 47;
        }
        if !denied(unsafe {
            libc::syscall(libc::SYS_clone3, std::ptr::null::<u8>(), 0) as libc::c_int
        }) {
            return 48;
        }
        if std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("exit 0")
            .status()
            .is_ok()
        {
            return 49;
        }
        match std::os::unix::net::UnixStream::connect("/run/dbus/system_bus_socket") {
            Err(error) if error.raw_os_error() == Some(libc::EPERM) => {}
            _ => return 50,
        }
        match std::os::unix::net::UnixStream::pair() {
            Err(error) if error.raw_os_error() == Some(libc::EPERM) => {}
            _ => return 51,
        }
        let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, libc::getpid(), 0) };
        if pidfd >= 0 {
            let result = unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    pidfd,
                    libc::SIGCONT,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                ) as libc::c_int
            };
            unsafe {
                libc::close(pidfd as libc::c_int);
            }
            if !denied(result) {
                return 52;
            }
        }
        0
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
    const LANDLOCK_SCOPE_SIGNAL: u64 = 1 << 1;

    #[repr(C)]
    struct RulesetAttr {
        handled_access_fs: u64,
    }

    #[repr(C)]
    struct RulesetAttrV6 {
        handled_access_fs: u64,
        handled_access_net: u64,
        scoped: u64,
    }

    #[repr(C)]
    struct PathBeneathAttr {
        allowed_access: u64,
        parent_fd: i32,
    }

    fn apply_landlock(plugin_root: Option<&std::path::Path>) -> io::Result<()> {
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
        let ruleset = if abi >= 6 {
            let attr = RulesetAttrV6 {
                handled_access_fs: handled,
                handled_access_net: 0,
                scoped: LANDLOCK_SCOPE_SIGNAL,
            };
            unsafe {
                libc::syscall(
                    libc::SYS_landlock_create_ruleset,
                    &attr,
                    std::mem::size_of::<RulesetAttrV6>(),
                    0u32,
                )
            }
        } else {
            let attr = RulesetAttr {
                handled_access_fs: handled,
            };
            unsafe {
                libc::syscall(
                    libc::SYS_landlock_create_ruleset,
                    &attr,
                    std::mem::size_of::<RulesetAttr>(),
                    0u32,
                )
            }
        };
        if ruleset < 0 {
            return Err(io::Error::last_os_error());
        }
        let ruleset = ruleset as i32;

        use std::os::unix::ffi::OsStrExt;

        let mut paths: Vec<(std::path::PathBuf, u64)> = vec![
            ("/usr".into(), ACCESS_READ_FILE | ACCESS_READ_DIR),
            ("/lib".into(), ACCESS_READ_FILE | ACCESS_READ_DIR),
            ("/lib64".into(), ACCESS_READ_FILE | ACCESS_READ_DIR),
            ("/etc/ld.so.cache".into(), ACCESS_READ_FILE),
        ];
        if let Some(root) = plugin_root {
            paths.push((root.to_path_buf(), ACCESS_READ_FILE | ACCESS_READ_DIR));
        }
        for (path, allowed) in paths {
            let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())?;
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
    const NR: u32 = 0;
    const ARCH: u32 = 4;
    const ARG0: u32 = 16;
    const ARG1: u32 = 24;
    const ARG2: u32 = 32;
    const F_SETOWN_EX: u32 = 15;
    const F_SETSIG: u32 = 10;

    fn apply_seccomp() -> io::Result<()> {
        #[cfg(target_arch = "x86_64")]
        const NATIVE_ARCH: u32 = 0xC000_003E;
        #[cfg(target_arch = "aarch64")]
        const NATIVE_ARCH: u32 = 0xC000_00B7;
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        const NATIVE_ARCH: u32 = 0;
        let mut filter = Vec::new();
        push(&mut filter, BPF_LD | BPF_W | BPF_ABS, ARCH);
        jump(&mut filter, BPF_JMP | BPF_JEQ | BPF_K, NATIVE_ARCH, 1, 0);
        ret(&mut filter, SECCOMP_RET_ERRNO | libc::EPERM as u32);
        push(&mut filter, BPF_LD | BPF_W | BPF_ABS, NR);
        jump(&mut filter, BPF_JMP | BPF_JSET | BPF_K, 0x4000_0000, 0, 1);
        ret(&mut filter, SECCOMP_RET_ERRNO | libc::EPERM as u32);

        for nr in [
            libc::SYS_kill,
            libc::SYS_tkill,
            libc::SYS_tgkill,
            libc::SYS_rt_sigqueueinfo,
            libc::SYS_rt_tgsigqueueinfo,
            libc::SYS_pidfd_send_signal,
            libc::SYS_execve,
            libc::SYS_execveat,
            libc::SYS_memfd_create,
            libc::SYS_mount,
            libc::SYS_umount2,
            libc::SYS_pivot_root,
            libc::SYS_chroot,
            libc::SYS_setns,
            libc::SYS_unshare,
            libc::SYS_open_tree,
            libc::SYS_move_mount,
            libc::SYS_fsopen,
            libc::SYS_fsconfig,
            libc::SYS_fsmount,
            libc::SYS_fspick,
            libc::SYS_mount_setattr,
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
            libc::SYS_kcmp,
            libc::SYS_pidfd_getfd,
            libc::SYS_open_by_handle_at,
            libc::SYS_io_uring_setup,
            libc::SYS_io_uring_enter,
            libc::SYS_io_uring_register,
            libc::SYS_bpf,
            libc::SYS_perf_event_open,
            libc::SYS_userfaultfd,
            libc::SYS_keyctl,
            libc::SYS_clone3,
            libc::SYS_socket,
            libc::SYS_socketpair,
            libc::SYS_connect,
        ] {
            deny_syscall(&mut filter, nr);
        }
        deny_arg0_mask(&mut filter, libc::SYS_clone, 0x7e02_0080, ARG0);
        push(&mut filter, BPF_LD | BPF_W | BPF_ABS, NR);
        deny_fcntl(&mut filter, libc::SYS_fcntl, ARG1, ARG2);
        push(&mut filter, BPF_LD | BPF_W | BPF_ABS, NR);
        deny_ioctl(&mut filter, libc::SYS_ioctl, ARG1);
        push(&mut filter, BPF_LD | BPF_W | BPF_ABS, NR);
        ret(&mut filter, SECCOMP_RET_ALLOW);
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

    fn push(filter: &mut Vec<SockFilter>, code: u16, k: u32) {
        filter.push(SockFilter {
            code,
            jt: 0,
            jf: 0,
            k,
        });
    }

    fn jump(filter: &mut Vec<SockFilter>, code: u16, k: u32, jt: u8, jf: u8) {
        filter.push(SockFilter { code, jt, jf, k });
    }

    fn ret(filter: &mut Vec<SockFilter>, k: u32) {
        push(filter, BPF_RET | BPF_K, k);
    }

    fn deny_syscall(filter: &mut Vec<SockFilter>, nr: libc::c_long) {
        jump(filter, BPF_JMP | BPF_JEQ | BPF_K, nr as u32, 0, 1);
        ret(filter, SECCOMP_RET_ERRNO | libc::EPERM as u32);
    }

    fn deny_arg0_mask(filter: &mut Vec<SockFilter>, nr: libc::c_long, mask: u32, arg0_offset: u32) {
        let start = filter.len();
        jump(filter, BPF_JMP | BPF_JEQ | BPF_K, nr as u32, 0, 0);
        push(filter, BPF_LD | BPF_W | BPF_ABS, arg0_offset);
        jump(filter, BPF_JMP | BPF_JSET | BPF_K, mask, 0, 1);
        ret(filter, SECCOMP_RET_ERRNO | libc::EPERM as u32);
        let end = filter.len();
        filter[start].jf = (end - start - 1) as u8;
    }

    fn deny_fcntl(
        filter: &mut Vec<SockFilter>,
        nr: libc::c_long,
        arg1_offset: u32,
        arg2_offset: u32,
    ) {
        let start = filter.len();
        jump(filter, BPF_JMP | BPF_JEQ | BPF_K, nr as u32, 0, 0);
        push(filter, BPF_LD | BPF_W | BPF_ABS, arg1_offset);
        for command in [libc::F_SETOWN as u32, F_SETOWN_EX, F_SETSIG] {
            jump(filter, BPF_JMP | BPF_JEQ | BPF_K, command, 0, 1);
            ret(filter, SECCOMP_RET_ERRNO | libc::EPERM as u32);
        }
        let setfl = filter.len();
        jump(
            filter,
            BPF_JMP | BPF_JEQ | BPF_K,
            libc::F_SETFL as u32,
            0,
            0,
        );
        push(filter, BPF_LD | BPF_W | BPF_ABS, arg2_offset);
        jump(
            filter,
            BPF_JMP | BPF_JSET | BPF_K,
            libc::O_ASYNC as u32,
            0,
            1,
        );
        ret(filter, SECCOMP_RET_ERRNO | libc::EPERM as u32);
        let end = filter.len();
        filter[setfl].jf = (end - setfl - 1) as u8;
        filter[start].jf = (end - start - 1) as u8;
    }

    fn deny_ioctl(filter: &mut Vec<SockFilter>, nr: libc::c_long, arg1_offset: u32) {
        let start = filter.len();
        jump(filter, BPF_JMP | BPF_JEQ | BPF_K, nr as u32, 0, 0);
        push(filter, BPF_LD | BPF_W | BPF_ABS, arg1_offset);
        jump(
            filter,
            BPF_JMP | BPF_JEQ | BPF_K,
            libc::FIOASYNC as u32,
            0,
            1,
        );
        ret(filter, SECCOMP_RET_ERRNO | libc::EPERM as u32);
        let end = filter.len();
        filter[start].jf = (end - start - 1) as u8;
    }

    struct EditorProxy {
        connection: anvil_ipc::Connection,
        next_id: u64,
        timeout: Duration,
        plugin_id: String,
        token: Token,
    }

    struct ProxyError {
        code: i32,
        message: String,
    }

    fn hex_token(token: Token) -> String {
        token.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn send_editor_hello(
        connection: &mut anvil_ipc::Connection,
        plugin_id: &str,
        token: Token,
    ) -> io::Result<()> {
        let body = serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session.hello",
            "params": {
                "plugin_id": plugin_id,
                "token": hex_token(token),
                "api_version": "1.0.0"
            }
        }))
        .map_err(|error| io::Error::other(format!("encode editor hello: {error}")))?;
        connection
            .send(&body)
            .map_err(|error| io::Error::other(format!("send editor hello: {error}")))
    }

    impl EditorProxy {
        fn connect(path: &str, timeout_ms: u64, plugin_id: &str, token: Token) -> io::Result<Self> {
            let mut connection = anvil_ipc::Connection::connect(path)?;
            send_editor_hello(&mut connection, plugin_id, token)?;
            Ok(Self {
                connection,
                next_id: 1,
                timeout: Duration::from_millis(timeout_ms),
                plugin_id: plugin_id.to_owned(),
                token,
            })
        }

        fn call(
            &mut self,
            control: &mut anvil_ipc::Connection,
            cancel_id: &serde_json::Value,
            method: &str,
            params: serde_json::Value,
        ) -> Result<serde_json::Value, ProxyError> {
            let request_id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params,
                "plugin_id": self.plugin_id,
                "token": hex_token(self.token),
            });
            let body = serde_json::to_vec(&request)
                .map_err(|error| ProxyError::internal(format!("encode proxy request: {error}")))?;
            self.connection
                .send(&body)
                .map_err(|error| ProxyError::unavailable(format!("editor send: {error}")))?;
            let deadline = std::time::Instant::now() + self.timeout;
            loop {
                let now = std::time::Instant::now();
                if now >= deadline {
                    return Err(ProxyError::timeout());
                }
                if control
                    .wait_readable(Duration::from_millis(10))
                    .map_err(|error| ProxyError::unavailable(format!("control poll: {error}")))?
                {
                    match control.recv() {
                        Ok(None) => return Err(ProxyError::cancelled()),
                        Ok(Some(frame)) => {
                            if is_cancel_request(&frame, cancel_id) {
                                return Err(ProxyError::cancelled());
                            }
                        }
                        Err(error) => {
                            return Err(ProxyError::cancelled_with(format!(
                                "control recv: {error}"
                            )));
                        }
                    }
                }
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err(ProxyError::timeout());
                }
                if !self
                    .connection
                    .wait_readable(remaining.min(Duration::from_millis(50)))
                    .map_err(|error| ProxyError::unavailable(format!("editor poll: {error}")))?
                {
                    continue;
                }
                let frame = match self.connection.recv() {
                    Ok(Some(frame)) => frame,
                    Ok(None) => {
                        return Err(ProxyError::unavailable("editor proxy closed".to_owned()));
                    }
                    Err(error) => {
                        return Err(ProxyError::unavailable(format!("editor recv: {error}")));
                    }
                };
                let Ok(response) = serde_json::from_slice::<serde_json::Value>(&frame) else {
                    continue;
                };
                if response.get("id") != Some(&serde_json::json!(request_id)) {
                    continue;
                }
                if let Some(error) = response.get("error") {
                    return Err(ProxyError {
                        code: error
                            .get("code")
                            .and_then(serde_json::Value::as_i64)
                            .and_then(|code| i32::try_from(code).ok())
                            .unwrap_or(-32603),
                        message: error
                            .get("message")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("editor proxy error")
                            .to_owned(),
                    });
                }
                return Ok(response
                    .get("result")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null));
            }
        }
    }

    impl ProxyError {
        fn cancelled() -> Self {
            Self {
                code: -32004,
                message: "cancelled".to_owned(),
            }
        }

        fn cancelled_with(message: String) -> Self {
            Self {
                code: -32004,
                message,
            }
        }

        fn timeout() -> Self {
            Self {
                code: -32005,
                message: "editor proxy timed out".to_owned(),
            }
        }

        fn unavailable(message: String) -> Self {
            Self {
                code: -32008,
                message,
            }
        }

        fn internal(message: String) -> Self {
            Self {
                code: -32603,
                message,
            }
        }
    }

    fn is_cancel_request(frame: &[u8], cancel_id: &serde_json::Value) -> bool {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(frame) else {
            return false;
        };
        value.get("method").and_then(serde_json::Value::as_str) == Some("$/cancelRequest")
            && value.get("params").and_then(|params| params.get("id")) == Some(cancel_id)
    }
    fn watch_editor(
        connection: &mut anvil_ipc::Connection,
        mut gateway: Option<&mut anvil_ipc::GatewayClient>,
        mut editor_proxy: Option<&mut EditorProxy>,
        lua: Option<&mlua::Lua>,
    ) -> io::Result<()> {
        loop {
            match connection.recv() {
                Ok(None) => return Ok(()),
                Ok(Some(frame)) => {
                    handle_rpc(
                        connection,
                        gateway.as_deref_mut(),
                        editor_proxy.as_deref_mut(),
                        lua,
                        &frame,
                    )?;
                }
                Err(error) => {
                    return Err(io::Error::other(format!("editor frame: {error}")));
                }
            }
        }
    }

    fn handle_rpc(
        connection: &mut anvil_ipc::Connection,
        gateway: Option<&mut anvil_ipc::GatewayClient>,
        editor_proxy: Option<&mut EditorProxy>,
        lua: Option<&mlua::Lua>,
        frame: &[u8],
    ) -> io::Result<()> {
        let Ok(request) = serde_json::from_slice::<serde_json::Value>(frame) else {
            return Ok(());
        };
        let id = request
            .get("id")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let Some(method) = request.get("method").and_then(|value| value.as_str()) else {
            return Ok(());
        };
        let params = request
            .get("params")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let response = match method {
            "provider.request" => match lua {
                Some(lua) => match runtime::call_provider(lua, &params) {
                    Ok(result) => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": result
                    }),
                    Err(message) => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32603, "message": message }
                    }),
                },
                None => serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32008, "message": "provider runtime unavailable" }
                }),
            },
            "sys.ping" => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": "pong"
            }),
            method if method.starts_with("sys.") => match gateway {
                Some(gateway) => match gateway.call(method, params) {
                    Ok(result) => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": result
                    }),
                    Err(error) => rpc_error(id, error),
                },
                None => serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32008,
                        "message": "gateway unavailable"
                    }
                }),
            },
            method if method.starts_with("editor.") => match editor_proxy {
                Some(proxy) => match proxy.call(connection, &id, method, params) {
                    Ok(result) => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": result
                    }),
                    Err(error) => rpc_proxy_error(id, error),
                },
                None => rpc_proxy_error(
                    id,
                    ProxyError::unavailable("editor proxy unavailable".to_owned()),
                ),
            },
            _ => return Ok(()),
        };
        let body = serde_json::to_vec(&response)
            .map_err(|error| io::Error::other(format!("encode rpc response: {error}")))?;
        connection
            .send(&body)
            .map_err(|error| io::Error::other(format!("send rpc response: {error}")))
    }

    fn rpc_error(id: serde_json::Value, error: anvil_ipc::ClientError) -> serde_json::Value {
        let (code, message) = match error {
            anvil_ipc::ClientError::Rpc { code, message } => (code, message),
            other => (-32000, other.to_string()),
        };
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message }
        })
    }

    fn rpc_proxy_error(id: serde_json::Value, error: ProxyError) -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": error.code, "message": error.message }
        })
    }

    fn send_hello() -> io::Result<()> {
        let body = serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session.hello",
            "params": {
                "session_id": "empty",
                "protocol": "anvil-ipc/1",
                "api_version": "1.0.0"
            }
        }))
        .map_err(|error| io::Error::other(format!("encode hello: {error}")))?;
        let mut stdout = io::stdout().lock();
        stdout.write_all(&(body.len() as u32).to_be_bytes())?;
        stdout.write_all(&body)?;
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

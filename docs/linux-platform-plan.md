# Linux Platform Plan

Status: planned. Cross-cuts `0.4.7` process/resource isolation, `0.4.8` plugin
I/O, and `0.6.1` package/install atomicity. Linux is the v1 platform; this plan
does not add Windows/macOS ports.

Scope: use Linux kernel primitives where they provide enforcement, atomicity, or
reliable lifecycle. It is not a catalogue of every available syscall.

## 1. Principles

1. **Enforce in the kernel, not only in application code.** Landlock, seccomp,
   cgroups, and `openat2` are the primary boundaries.
2. **Keep Linux code at the boundary.** Syscalls belong in
   `anvil-plugin-host`, `go/internal`, and terminal adapters, not in
   `anvil-api`, `anvil-core`, or `anvil-editor`.
3. **Fail closed for security features.** Missing Landlock/seccomp/cgroup
   capabilities disable the affected feature or the plugin; they never silently
   downgrade to an unsandboxed path.
4. **Feature-detect optional newer APIs.** The v1 floor is Linux 5.13; newer
   primitives are additive and gated by ABI/feature checks.
5. **Prefer atomic filesystem operations over cleanup on failure.** Stage,
   verify, fsync, then rename; never leave a half-installed package.
6. **Measure before adding event machinery.** Polling stays until inotify or
   another backend beats it on correctness and cost.
7. **No privileged daemons.** Per-session gateway and per-plugin host stay
   unprivileged; no global systemd service, eBPF, FUSE, or setuid helper.

## 2. Current baseline

| Primitive | Current use |
|---|---|
| `openat2` with `RESOLVE_BENEATH` | gateway workspace confinement |
| Landlock | plugin-host and exec-runner filesystem rules |
| seccomp | preconnected editor UDS; dangerous syscall and signal denial |
| `no_new_privs` | before Landlock/seccomp |
| cgroup v2 `memory.max` / `pids.max` | plugin-host limits |
| `PR_SET_PDEATHSIG` | child cleanup |
| `close_range` | FD inheritance removal |
| Unix socket + `SO_PEERCRED` | same-UID peer check |
| XDG base directories | config/keymap paths |
| termios/crossterm | raw mode and terminal lifecycle |

Known gaps:

- cgroup placement is now performed before `execve`; structured
  `memory.events`/`memory.peak`/`pids.peak` reporting is still missing.
- Host stop uses `pidfd` where available and falls back to `cgroup.kill` or the
  process group; reaping still goes through the `Child` handle.
- cgroup OOM/CPU accounting is not surfaced as structured state.
- Install/update paths are not implemented yet; there is no `O_TMPFILE`,
  `renameat2`, or parent-directory fsync contract.
- Landlock network rules are not used even on kernels that support them.
- Tree watching uses gateway polling, not inotify; this is intentional until
  measured.

## 3. Phases

### L1: Process and cgroup hardening (`0.4.7`)

Status: `0.4.7b` landed for cgroup-before-exec, `pidfd`/`cgroup.kill` stop,
core-dump suppression, mandatory limits, and idle reclaim. Structured
memory/CPU reporting remains.

- Move cgroup placement before plugin execution: attach in `pre_exec` before
  `execve`, or use `clone3(CLONE_INTO_CGROUP)` where the process API permits it.
- Add `pidfd_open`/`pidfd_send_signal` for stop and wait; fall back only when
  pidfd is unavailable, with a documented kernel floor.
- Use `cgroup.kill` where available; fall back to process-group termination.
- Read `memory.events`, `memory.peak`, `pids.peak`, and `cpu.weight` for
  structured status/OOM classification.
- Set `RLIMIT_CORE=0` and `PR_SET_DUMPABLE=0` for plugin hosts unless debug
  mode is explicitly enabled.
- Keep `PR_SET_PDEATHSIG` and close the parent-death race before sandbox setup.
- Tests: child never executes before cgroup attach; OOM classification;
  pidfd stop/wait; repeated reload leaves no process/cgroup; soft and hard limits
  fail closed.

### L1b: Seccomp closure for signal and composition bypasses (`0.4.7`)

Status: `0.4.7c-a/b` landed; the editor socket is connected before
Landlock/seccomp and the syscall denylist closes the listed bypass classes.

`LANDLOCK_SCOPE_SIGNAL` is not currently enabled by Anvil, so
CVE-2026-72183's exact scope path is not reachable today. The same authority is
still reachable because the current plugin-host seccomp filter allows `kill`,
`fcntl`, `memfd_create`, `execveat`, `mount`, and other dangerous syscalls.
Seccomp must close the class, not just the CVE.

- Deny signal delivery: `kill`, `tkill`, `tgkill`, `rt_sigqueueinfo`,
  `rt_tgsigqueueinfo`, and `pidfd_send_signal`.
- Deny async signal registration: `fcntl(F_SETOWN)`, `F_SETOWN_EX`, `F_SETSIG`,
  `F_SETFL` with `O_ASYNC`, and `ioctl(FIOASYNC)`.
- Deny memory execution: `execve`, `execveat`, `fexecve`, and
  `memfd_create` unless a measured runtime path requires it.
- Deny namespace/mount escapes: `mount`, new mount API calls, `umount2`,
  `pivot_root`, `chroot`, `setns`, and `unshare`.
- Reject namespace bits in `clone` flags (arg0) with a BPF mask. Classic seccomp
  cannot dereference `clone3`'s pointed-to flags: deny `clone3` after sandbox
  unless a measured runtime path needs it. If threads require it, use seccomp
  user notification with supervisor validation or pre-create the needed threads;
  do not claim BPF validates `clone3` flags.
- Deny introspection and kernel attack surface: `ptrace`,
  `process_vm_readv/writev`, `kcmp`, `pidfd_getfd`, `open_by_handle_at`,
  `io_uring_*`, `bpf`, `perf_event_open`, `userfaultfd`, and `keyctl`.
- For AF_UNIX, pre-connect the editor socket before sandboxing and pass the FD;
  then deny `socket`, `socketpair`, and `connect`, closing the D-Bus/abstract
  socket escape path. Until then, document the residual risk.
- Treat `LANDLOCK_SCOPE_SIGNAL` as defense-in-depth only, enabled only on patched
  kernels; seccomp remains the primary control. Never claim scope-signal
  protection based on a kernel version check alone.
- Tests: hostile plugin attempts `kill(getppid)`, SIGIO `fcntl`, `pidfd` signal,
  `memfd_create` + `execveat`, `mount`, `ptrace`, and D-Bus connect; every
  attempt returns `EPERM` and the supervisor remains alive.

### L2: Filesystem and install atomicity (`0.4.8` / `0.6.1`)

- Use `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS` for package staging
  and archive extraction.
- Stage installs with `O_TMPFILE` or a private temp directory on the same mount;
  verify digest/signature, `fsync` the file, then commit.
- Commit with `renameat2(RENAME_NOREPLACE)` for new versions and
  `RENAME_EXCHANGE` for rollback; never overwrite a live version in place.
- `fsync` the parent directory after commit/removal.
- Use `statx` inode/mount identity to reject cross-mount renames and to make
  selection/update identity stable.
- Keep workspace edits on the existing atomic save path; do not introduce a
  second save implementation.
- Tests: cross-mount rename rejected, symlink escape rejected, interrupted
  commit leaves the old version, rollback exchanges atomically, fsync failure
  reports an error.

### L3: Network and terminal edges (`0.4.8` / `0.5.7`)

- On kernels with Landlock network support, add network rules as
  defense-in-depth; seccomp remains authoritative on older kernels.
- Keep the preconnected plugin-host transport AF_UNIX-only.
- Installer fetch uses the fixed host allowlist and bounded body policy from
  [0.6.x-plan.md](0.6.x-plan.md); no arbitrary manifest URLs.
- Desktop notifications, if added, use an optional trusted XDG/D-Bus path from
  the editor process; plugins never get a notification or D-Bus handle.
- Tests: Landlock network ABI absent falls back safely; non-allowlisted hosts
  denied; notification failure is non-fatal.

### L4: Measurement-only backends (later)

- Benchmark inotify/fanotify against the current polling watcher before
  replacing it. Include watch limits, recursive create/delete/rename, and
  memory/fd cost.
- Do not add `io_uring` unless a measured file-I/O bottleneck cannot be solved
  with the current thread model.
- Revisit user/mount namespaces only if Landlock + seccomp + cgroup leave a
  concrete gap.
- Each alternative requires an ADR with rollback and failure behavior.

## 4. Placement

```text
anvil-tui / anvil-session
  -> trusted orchestration and presentation
  -> gateway / plugin-host / exec-runner
       -> Linux syscalls and kernel policy
```

- `anvil-api`, `anvil-core`, `anvil-editor`: portable types; no Linux syscalls.
- `anvil-plugin-host`: sandbox setup, cgroup attach, pidfd, dumpable/core.
- `go/internal/gateway`, `go/internal/execrunner`, `go/internal/fsroot`:
  `openat2`, policy, cgroup lifecycle, process cleanup.
- `anvil-tui`: terminal/XGD edge only; no privilege-bearing filesystem work.

## 5. Security invariants

1. A plugin host never runs before its cgroup limits apply.
2. A missing kernel security feature fails closed; it does not disable the
   sandbox claim.
3. A pidfd or equivalent identity is required for destructive process actions
   where available.
4. Install/update never mutates a live package in place.
5. A failed commit leaves the previous working version and grants intact.
6. Symlink and cross-mount escapes are rejected at the syscall boundary.
7. No plugin receives a network, D-Bus, or notification handle.
8. A failed cleanup is logged and bounded; it never blocks the editor.

## 6. Non-goals

- Windows/macOS support in v1.
- A global privileged daemon or setuid helper.
- eBPF, FUSE, or kernel-module dependencies.
- `io_uring` without a measured bottleneck.
- Replacing seccomp with Landlock, or Landlock with application checks.
- Recursive workspace watching before benchmark evidence.
- Exposing raw Linux syscalls through the plugin API.

## 7. Exit gates

Each phase follows [RELEASE.md](../RELEASE.md) and passes:

- `cargo test --workspace --no-fail-fast`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
- `go test ./...`
- installed-binary smoke test
- a failure-path test for every new syscall/feature gate
- no new unreferenced `unsafe` block without a safety comment

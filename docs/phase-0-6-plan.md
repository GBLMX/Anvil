# Phase 0.6 Plan: Security and Host Boundaries

Status: in progress. Phase 0.5 is complete.

## Order

1. exec sandbox (`Landlock`/seccomp)
2. fetch/SSRF policy
3. Go gateway routing
4. plugin-host isolation

## 1. Exec sandbox

Implemented:

- `go/cmd/anvil-exec-runner`: fixed absolute tool path, no shell, no PATH lookup;
- `go/internal/execrunner`: Landlock read-only workspace and runtime rules,
  `no_new_privs`, and a positive seccomp allowlist;
- seccomp rejects namespace/privilege/network/mount/ptrace syscalls outside the
  allowlist, returns `ENOSYS` for `clone3` so glibc falls back to `clone`, and
  filters `clone` namespace flags, anonymous `PROT_EXEC` mmap, and
  `mprotect(PROT_EXEC)`;
- Landlock grants execute only to the selected tool and its ELF interpreter;
  runtime directories remain read-only;
- the runner locks its OS thread before applying per-thread Landlock/seccomp
  state and execs on that same goroutine;
- `go/internal/execpolicy`: `rg`/`fd` argv parsers rejecting command-launching
  and config-loading flags;
- process-group supervision with timeout, cancellation, descendant kill,
  combined output cap, `Pdeathsig`, and `WaitDelay`;
- per-sandbox cgroup v2 `pids.max` when `ANVIL_CGROUP_PATH` is supplied,
  otherwise `RLIMIT_NPROC` reserves 64 tasks above the user's current count;
- extra descriptors are marked close-on-exec before applying the sandbox;
- tests for outside-file/symlink and workspace-write denial, concurrent
  symlink-swap races, non-allowlisted exec (including via `ld.so`),
  socket/permission denial, allowlist escape, clone3 fallback, tool startup,
  cgroup file writes, RLIMIT_NPROC verification, timeout descendant kill,
  output cap, fd cleanup, and exit-code capture.

Open:

- gateway must resolve the tool fd/policy, create/clean up
  `ANVIL_CGROUP_PATH`, and pass a bounded request;
- CI needs an explicit Landlock capability probe and skip path.

## 2. Fetch/SSRF

Not started.

## 3. Go gateway

Not started.

## 4. Plugin host

Not started.

## Exit gates

- exec runner denies outside reads, network, ptrace, and privilege changes;
- SSRF denies loopback/private/link-local/CGNAT and DNS rebinding;
- gateway logs every denied request with plugin id and trace id;
- plugin host survives SIGKILL, is denied outside workspace, and re-handshakes
  after reload;
- full-kernel tests run locally/nightly; PR CI skips only after an explicit
  capability probe.

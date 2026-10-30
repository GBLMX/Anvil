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
- `go/internal/execrunner`: Landlock read-only workspace/runtime rules,
  `no_new_privs`, and a positive seccomp allowlist;
- seccomp rejects namespace/privilege/network/mount/ptrace syscalls outside the
  allowlist, returns `ENOSYS` for `clone3` so glibc falls back to `clone`, and
  filters `clone` namespace flags, anonymous `PROT_EXEC` mmap, and
  `mprotect(PROT_EXEC)`;
- `go/internal/execpolicy`: `rg`/`fd` argv parsers rejecting command-launching
  and config-loading flags;
- process-group supervision with timeout, descendant kill, combined output cap,
  `Pdeathsig`, and `WaitDelay`;
- extra descriptors are marked close-on-exec before applying the sandbox;
- tests for outside-file/symlink and workspace-write denial, socket/permission
  denial, allowlist escape, clone3 fallback, tool startup, timeout descendant
  kill, output cap, fd cleanup, and exit-code capture.

Open:

- gateway must resolve the tool fd/policy and pass a bounded request;
- CI needs an explicit Landlock capability probe and skip path;
- a live symlink-swap race test and process-count limit remain;
- runtime execute permissions still cover the shared runtime roots; a
  per-tool execute rule needs a follow-up because a Landlock file rule alone did
  not permit `execve` on this kernel.

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

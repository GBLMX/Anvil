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
  `no_new_privs`, seccomp denial of network/ptrace/mount/privilege syscalls;
- `go/internal/execpolicy`: `rg`/`fd` argv parsers rejecting command-launching
  and config-loading flags;
- tests for outside-file denial and socket denial.

Open:

- seccomp is currently a denied-syscall filter, not the desired positive
  allowlist;
- gateway must resolve the tool fd/policy and pass a bounded request;
- CI needs an explicit Landlock capability probe and skip path;
- symlink-race, timeout/descendant, output-cap, and process-count tests remain.

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

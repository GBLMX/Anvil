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
- cgroup move happens before Landlock/seccomp and before the final execve;
  when cgroup is active the runner raises the soft `RLIMIT_NPROC` to the hard
  limit, leaving `min(pids.max, RLIMIT_NPROC hard)` as the effective bound;
- extra descriptors are marked close-on-exec before applying the sandbox;
- tests for outside-file/symlink and workspace-write denial, concurrent
  symlink-swap races, non-allowlisted exec (including via `ld.so`),
  socket/permission denial, allowlist escape, clone3 fallback, tool startup,
  cgroup file writes and stale-directory cleanup, RLIMIT_NPROC verification,
  timeout descendant kill, output cap, fd cleanup, and exit-code capture.

Open:

- gateway must resolve the tool fd/policy, create `ANVIL_CGROUP_PATH` before
  launch, remove it after wait, and call `CleanupStaleCgroups` for stale empty
  directories at startup;
- gateway needs write access to `/sys/fs/cgroup/anvil-exec/` via systemd
  `Delegate=pids` or a pre-created/chowned root; it must not run as root just
  for cgroup management;
- CI needs an explicit Landlock capability probe and skip path;
- gateway control-socket registration and inherited-fd token delivery remain.

## 2. Fetch/SSRF

Implemented:

- `go/internal/fetch`: exact host allowlist, DNS pinning, A/AAAA validation,
  private/loopback/link-local/CGNAT/documentation/transition-range rejection,
  HTTPS-only production policy, disabled redirects, timeout, and body cap;
- `sys.fetch` routing in the gateway with `sys.fetch` permission checks;
- allowlisted request headers (`Accept`, `Authorization`, `Content-Type`,
  `User-Agent`), with sensitive header names audited without values;
- TLS `ServerName` set from the original host with `InsecureSkipVerify`
  explicitly false, while the TCP dial uses the pinned IP;
- `httptest` coverage for pinning/host preservation, mixed DNS answers,
  blocked addresses, host denial, redirect denial, headers, and body truncation.

Open:

- add deployment documentation for the allowlist; the gateway currently requires
  explicit `-fetch-allow` hosts and has no wildcard support.

## 3. Go gateway

Implemented:

- `go/cmd/anvil-gateway` and `go/internal/gateway`: per-session Unix socket
  with mode `0600`, 4-byte length-prefixed JSON-RPC, `SO_PEERCRED` UID check,
  `session.hello` token-hash authentication, and `sys.exec` routing through
  `anvil-exec-runner`;
- the bootstrap token hash is supplied with `-token-sha256`; the raw token is
  only sent by the client in `session.hello`;
- `crates/anvil-ipc/examples/gateway_handshake.rs` performs a real Rust-to-Go
  handshake;
- Go tests cover handshake rejection and the `sys.exec` runner route.

Security boundaries:

- `-token-sha256` is a verifier, not a token. The gateway hashes the 32-byte
  token from `session.hello` and compares it; it does not accept the hash as a
  credential. The token is consumed by the first successful handshake, so a
  second connection is rejected. v1 has no HMAC/nonce challenge; this is safe
  only because the token is 32 random bytes, is sent only over the session UDS,
  and is one-shot.
- `SO_PEERCRED` supplies the same-UID check; the one-shot token supplies plugin
  identity. Both are required.
- `-allow-exec` is per gateway instance, and the Phase 0.6 deployment uses one
  gateway per plugin session. Per-session permission negotiation is a follow-up.
- A gateway is a per-session process, not a daemon. `Close` removes the socket;
  a crashed gateway leaves a stale socket and no authority.
- Closing the client connection cancels in-flight `sys.exec` work and kills the
  runner process group. A handshake is single-use per connection.

Open:

- replace the bootstrap `-token-sha256` flag with editor control-socket
  registration and inherited-fd token delivery; client must pin the gateway
  pid/start-time so a stale-socket replacement cannot impersonate it;
- add fetch/SSRF and gateway audit logging;
- wire per-session cgroup creation/cleanup into `Run`;
- add real-runner gateway integration coverage to CI.

## 4. Plugin host

Not started. Split into independently verifiable slices:

- **0.7a runtime**: add Luau (`mlua`) or QuickJS (`rquickjs`), load a script,
  execute a function, and return a value; no gateway dependency.
- **0.7b control registration**: editor-to-gateway control socket, gateway
  pid/start-time pinning, and inherited-fd token delivery replacing the
  bootstrap hash argument.
- **0.7c plugin-host process**: read the token fd, connect to the gateway,
  `session.hello`, and receive permissions.
- **0.7d hot reload supervisor**: SIGTERM the old host, start a new host, and
  repeat the handshake. Verify no process, fd, or socket leak.

## Exit gates

- exec runner denies outside reads, network, ptrace, and privilege changes;
- SSRF denies loopback/private/link-local/CGNAT and DNS rebinding;
- gateway logs every denied request with plugin id and trace id;
- plugin host survives SIGKILL, is denied outside workspace, and re-handshakes
  after reload;
- full-kernel tests run locally/nightly; PR CI skips only after an explicit
  capability probe.

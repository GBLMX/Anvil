# ADR-0012: Plugin-Host Isolation

- Status: Proposed; empty-host + FS/exec/network denial + host-side UDS EOF
  exit + cgroup attach/OOM classification + framed host dispatch (`sys.ping`,
  `sys.exec` mock) + TUI debug HostSession + sandboxed Luau runtime smoke
  implemented; plugin loading and the capability API remain pending
- Date: 2026-10-31
- Related: [ADR-0001](0001-plugin-isolation.md), [ADR-0002](0002-go-daemon.md),
  [ADR-0007](0007-jsonrpc-envelope-and-capability-token.md),
  [ADR-0011](0011-workspace-chunked-file-io.md)

## Context

The editor must never execute plugin code in-process. A plugin host needs its own
process, kernel sandbox, capability token, and clean lifecycle. The Go gateway
policy-checks requests but is not the plugin sandbox.

0.1.1 starts with this boundary. No plugin runtime is exposed until all acceptance
checks below pass.

## Decision

### 1. Process and trust boundary

- One host process per plugin; no host serves multiple plugins.
- The host is in a new process group/session and is not a child of the gateway.
- The editor owns the host lifecycle and never loads plugin code.
- Host crash, timeout, or sandbox setup failure disables that plugin only.
- No fallback to an unsandboxed process.

### 2. Kernel sandbox

Applied before plugin VM initialization:

- `no_new_privs`.
- Landlock: read-only runtime files needed by the VM; workspace access only for
  granted paths. Landlock network rules are used only on kernels that support
  them (>= 6.7); on 5.13-6.6 network denial is a seccomp responsibility, not a
  Landlock claim.
- seccomp: checks `seccomp_data.arch` against the native ABI; `socket`/
  `socketpair` allow only `AF_UNIX`; other domains return `EPERM`, so TCP/UDP
  cannot be created. Non-native ABIs and x32 (`nr & 0x4000_0000`) are
  rejected. UDS stream operations remain allowed.
  Broader namespace/mount/ptrace filtering is pending, except for the clone
  policy below.
- seccomp clone policy: only thread-style clones pass. A clone without
  `CLONE_THREAD` is rejected, so `fork`/`vfork`-like process creation cannot
  escape through `clone`; `SYS_fork` and `SYS_vfork` are denied where the
  architecture defines them (aarch64 has neither). `CLONE_PARENT`,
  `CLONE_PTRACE`, `CLONE_UNTRACED`, `CLONE_VFORK`, `CLONE_PIDFD`, and all
  namespace flags are denied. `CLONE_FS` and the other flags glibc sets for
  `pthread_create` stay allowed so the Luau runtime keeps normal thread
  creation. `clone3` returns `ENOSYS` (not `EPERM`) so glibc falls back to
  `clone`; the syscall itself remains unusable.
- Host stderr is terminal output, not a log file. Untrusted plugin text only
  reaches it through one escaping sink: ESC and every C0/C1 control character
  except `\n` and `\t` is rendered as `\xNN`, and each line is capped at 8 KiB.
  This covers `print`, `host.event` handler errors, and Lua load/runtime
  errors, so a plugin cannot emit ANSI/OSC sequences to the editor terminal.
- Landlock handles `EXECUTE`; no path is granted execute after startup.
- RLIMIT/cgroup pids, memory, fds, and output caps. cgroup setup verifies the
  cgroup2 superblock and opens existing control files without creating them.
- The host starts with a minimal environment; no inherited editor/gateway FDs.

### 3. Token delivery

- Editor generates a fresh 32-byte token per host.
- Editor registers `(plugin_id, sha256(token), workspace, grants)` with the
  gateway before spawn.
- Token is delivered through a dedicated inherited pipe/FD, never argv, env, or
  a world-readable file.
- The host reads and closes the FD before plugin code loads.
- Gateway token is one-shot and bound to the first successful handshake.
- A host cannot reuse another plugin's token; token hash and peer credentials are
  both checked.

### 4. Sockets

- `editor.sock` and `gateway.sock` live in a private per-session runtime directory
  with mode `0700`; socket mode `0600`.
- The host connects only after sandbox setup; editor and gateway verify
  `SO_PEERCRED`.
- Malformed frame, stale token, or wrong peer closes the connection and disables
  the plugin.

### 5. Lifecycle

Start:

1. Validate manifest and grants.
2. Register token hash with gateway.
3. Spawn host in a new process group.
4. Pass token FD.
5. Host applies sandbox, connects, handshakes.
6. Any failure: kill group, revoke token, delete runtime state.

Stop/reload:

1. Stop accepting plugin requests.
2. Cancel in-flight work.
3. Send EOF, wait with timeout, kill process group if needed.
4. Revoke token and delete host runtime state.
5. Start a new host with a new token if requested.

### 6. Acceptance tests

- Outside-workspace read/write is denied.
- Symlink and `..` escape are denied.
- Network, mount, namespace, ptrace, and unallowlisted exec are denied.
- Plugin cannot read editor/gateway token or reuse another plugin's token.
- Host cannot inherit gateway/editor FDs.
- Plugin crash does not terminate editor, gateway, or other plugins.
- Reload leaves no orphan process, socket, cgroup, or token.
- Sandbox setup failure never starts plugin code.

## Consequences

Positive:

- Plugin compromise is contained by process + kernel + capability boundaries.
- Gateway remains a policy point, not a false sandbox claim.

Negative:

- One process per plugin increases memory/startup cost.
- Lua/QuickJS runtime files must be packaged read-only.
- Reload requires explicit token revocation and process-group cleanup.

## Open

- First runtime: Lua or QuickJS (must not add ambient native modules).
- Whether the host is Rust-native or embeds an existing VM.
- Exact editor.sock `editor.*` schema remains a separate API decision.

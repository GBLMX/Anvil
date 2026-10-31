# Anvil Project Charter

Status: **0.2.10 / Phase 0.7 in progress. Pre-alpha.**
Supersedes: the V1.0 whitepaper draft where it conflicts with the decisions below.
Changes to this document require an ADR.

## 1. Scope

Anvil is a Linux-first terminal editor with:

- a Rust editor core and terminal UI;
- a small, capability-scoped plugin API;
- a separate plugin host for untrusted Lua/QuickJS code;
- a per-session policy gateway for file, network, and process access.

The editor must remain usable without plugins. Plugins are an extension surface,
not a prerequisite for editing.

## 2. Non-goals

- Windows and macOS in v1.
- A fully declarative widget tree in v1.
- V8/Deno in v1.
- A general-purpose shell inside the editor.
- An unbounded plugin API.
- Claiming "zero trust" while running plugin runtimes inside the editor process.

## 3. Product principles

1. **Capability over ambient authority.** A plugin gets only the operations the
   user granted for the current workspace.
2. **Data plane and control plane are separate.** The editor owns editing state;
   the gateway owns system policy.
3. **The isolation boundary is a process boundary.** A policy check inside the
   editor process is not a sandbox.
4. **Measured, not asserted.** Performance and security claims have executable
   gates in Phase 0.
5. **Small API, explicit versions.** The first plugin API is intentionally small
   and versioned independently from the editor binary.
6. **Linux first, honestly.** Full untrusted-plugin support requires kernel
   5.13+ for Landlock. Kernel 5.6-5.12 may run trusted plugins only.
7. **One native interaction model per phase.** Phase 0 is selection-first.
   Keymaps are plugins; motion and action semantics belong to the core.

## 4. Trust model

| Component | Trust | Owns | Must not own |
|---|---|---|---|
| Rust editor | Trusted | Buffer, input, rendering, snapshots, plugin lifecycle | Ambient file/network/process policy |
| Go gateway | Trusted policy component | `sys.*` policy, file/network/exec mediation | Plugin runtimes, editor state |
| Plugin host | Untrusted | Lua/QuickJS VM, plugin lifecycle, sandbox setup | Ambient filesystem/network access |
| Plugin code | Untrusted | Its own logic | Direct OS access, gateway bypass |
| Terminal/user | Trusted input | Keyboard, mouse, terminal capabilities | Plugin code |

### The critical invariant

> **The Go gateway is not the plugin sandbox. The plugin host process is.**

The gateway enforces capability policy for requests that reach it. It cannot
contain a plugin that runs in the editor process, calls a native Lua/QuickJS
function, or makes a direct syscall. Therefore:

- Lua/QuickJS never run in the editor process.
- Every plugin or trust group runs in a separate plugin-host process.
- The plugin host has no ambient filesystem/network access; its requests go
  through the gateway with a per-plugin capability token.
- If the host cannot be isolated, untrusted plugins are disabled. There is no
  silent fallback to "trusted mode".

## 5. Architecture

```text
┌──────────────────────────────────────────────┐
│ Rust editor (trusted)                        │
│ input · buffer · view · snapshots · commands │
└───────────┬───────────────────────┬──────────┘
            │ control UDS           │ editor/ui API
            │ (per plugin)          │
┌───────────▼───────────────┐       │
│ Plugin host (untrusted)   │───────┘
│ Lua / QuickJS             │
│ Landlock + seccomp + NNP  │
└───────────┬───────────────┘
            │ sys.* UDS + capability token
┌───────────▼───────────────┐
│ Go gateway (trusted)      │
│ per-session, unprivileged │
│ policy · openat2 · HTTP   │
└───────────┬───────────────┘
            │
      files / network / tools
```

- One gateway per editor session. No global daemon spanning workspaces.
- One plugin host per plugin, or per explicit trust group. Cross-plugin
  isolation is not implied by sharing a host.
- The plugin host receives a per-plugin token from the editor. The gateway
  binds that token to the granted permissions; `SO_PEERCRED` alone is not plugin
  identity.
- The gateway never executes plugin code. It is small enough to fuzz and audit.

## 6. IPC contract

- Transport: Unix domain socket, per-session directory under
  `$XDG_RUNTIME_DIR/anvil/`.
- Permissions: socket mode `0600`; directory mode `0700`.
- Framing: 4-byte big-endian length prefix followed by UTF-8 JSON-RPC 2.0.
- Maximum frame: 8 MiB. Larger file/network payloads are streamed or rejected.
- Authentication: `SO_PEERCRED` plus an unguessable per-plugin capability token.
- Concurrency: JSON-RPC request ids; cancellation by id; no shared mutable
  request state between plugins.
- Timeouts: every request has a deadline. A cancelled request must release
  buffers, sockets, and child processes.

## 7. Security invariants

These are release blockers, not aspirations.

1. **Workspace confinement.** Every `sys.read_file` path resolves relative to a
   workspace root fd with `openat2(RESOLVE_BENEATH | RESOLVE_NO_MAGICLINKS)`.
   `RESOLVE_NO_SYMLINKS` is a policy option, not an unconditional default.
2. **No ambient I/O in plugins.** Lua/QuickJS host environments expose no
   `io`, `os.execute`, `package.loadlib`, `debug` escape surface, or native C
   module loading. The host starts with an explicit capability table.
3. **No network except granted destinations.** HTTPS only, port 443 only,
   `GET`/`HEAD` by default, redirects disabled, every resolved A/AAAA address
   checked, and the connection pinned to the checked address while preserving
   Host and TLS SNI.
4. **No shell execution and no unsandboxed exec.** `sys.exec` accepts argv
   arrays only. No shell string or inherited environment is allowed. The
   gateway invokes a sandboxed `anvil-exec-runner` that applies Landlock,
   seccomp, and `no_new_privs` before the tool starts; a process group alone is
   not a filesystem boundary.
5. **No orphan processes.** `sys.exec` children run in their own process group;
   timeout/cancel/reload kills the whole tree and reaps it.
6. **No manifest self-escalation.** `Anvil.toml` permissions are requests. The
   user grants them per workspace; plugin updates re-request new permissions.
7. **No silent sandbox degradation.** If Landlock/seccomp setup fails, the plugin
   does not start. The editor may offer an explicit trusted-plugin mode, but it
   must not be confused with the sandbox.
8. **No stale snapshots.** Editor snapshots are immutable `Arc` values with a
   revision; plugins cannot mutate the editor buffer except through authorized
   edit operations.

## 8. Public API

The v1 contract is four modules and ten methods. Exact signatures are frozen by
ADR-0003; names below are the baseline.

| Module | Methods | Purpose |
|---|---|---|
| `editor` | `get_view`, `set_mode`, `apply_edit`, `register_command` | Read viewport/selection/interaction mode, switch the session mode, apply an authorized transaction, register a user-invoked command |
| `ui` | `notify`, `status` | Transient user feedback only |
| `event` | `subscribe` | Subscribe to an allowlisted event set |
| `sys` | `read_file`, `fetch`, `exec` | Capability-gated system access |

All transport calls are asynchronous. Lua gets a blocking wrapper on its own OS
thread; QuickJS uses Promises. Cancellation follows the request id and must
propagate to DNS, HTTP, file reads, and child processes.

## 9. Permissions

```toml
[plugins.permissions]
editor = ["read", "write", "commands", "mode"]
network = ["GET:https://api.github.com"]
fs = ["read:src/**"]
exec = ["rg", "fd"]
```

- Permissions are scoped to a workspace.
- `network` entries must include method and exact host; host wildcards are not
  supported in v1.
- `mode` permits changing the session interaction mode; it does not grant
  buffer or command authority.
- `fs` entries are interpreted under the workspace root; absolute paths and
  traversal are rejected before `openat2`.
- `exec` entries name executables; each executable also has an argument policy.
- Published plugins should ship a signed manifest and a permission hash. A
  changed hash invalidates the existing grant.

## 10. Performance gates

Definitions and measurement live in [docs/performance.md](docs/performance.md).

Phase 0 gates:

- malicious plugin cannot read outside the workspace;
- `openat2` symlink-race tests pass;
- `sys.exec` rejects `rg --pre` and `fd --exec`;
- `sys.exec` spawns through `anvil-exec-runner`, and a workspace symlink raced
  against `rg`/`fd` cannot read outside the workspace;
- 100 MB file opens within target;
- random edits at 1%, 50%, and 99% offsets have measured p99;
- first-frame cold start is measured with the gateway excluded and included;
- no hot-reload or child-process leak after repeated cycles.

## 11. Phase 0 exit criteria

Phase 0 is complete only when all of these are demonstrated in code or a
reproducible harness:

1. **Plugin isolation PoC**: separate host process, Landlock/seccomp/no_new_privs,
   and a malicious Lua test that cannot read outside its workspace.
2. **Path security suite**: traversal, absolute paths, symlink escape, magic
   links, workspace-root symlink, and `EAGAIN` retry.
3. **Exec sandbox PoC**: dangerous tool flags and inherited environment
   rejected; `anvil-exec-runner` applies Landlock/seccomp/no_new_privs; a
   workspace symlink raced during `rg`/`fd` cannot read outside; timeout kills
   descendants.
4. **SSRF suite**: private IPv4/IPv6, CGNAT, metadata, DNS rebinding simulation,
   redirect, compressed-body bomb, and wrong SNI.
5. **Text benchmark**: Gap Buffer versus Rope/piece table on 1/10/100 MB,
   including random edits, undo, long lines, and memory; PieceTable selected
   as the production backend in ADR-0009.
6. **IDL decision**: lightweight generated stubs accepted, or a Smithy spike
   proves its cost within one week.
7. **Performance harness**: reproducible first-frame, open, edit, scroll, and
   gateway latency measurements on the declared reference machine.

If a gate fails, stop and change the design. Do not mark it "Phase 1".

## 12. Delivery phases

| Phase | Outcome | Gate |
|---|---|---|
| 0 | Security and performance spikes | All Phase 0 exit criteria |
| 1 | Gateway and plugin-host vertical slice | `read_file` + sandboxed `exec` mediated end to end |
| 2 | Lua API, hot reload, cancellation | No escape, no zombies, no RSS growth |
| 3 | QuickJS, generated stubs, build | TS plugin passes the same security suite |
| Later | Optional static assembly, more platform support | Separate ADR |

Dates are planning estimates, not release commitments.

## 13. Governance

- Architecture decisions are recorded in `docs/adr/`.
- Plugin API versions independently from the editor binary.
- Breaking plugin API changes require at least six months or two minor versions
  of notice, whichever is longer.
- Security fixes may bypass the notice period.
- Dependency licenses are checked by `cargo-deny`/`go-licenses`; new native
  dependencies need an ADR.
- Every security invariant has a test; a failing security test blocks release.

## 14. License

Anvil is dual-licensed under Apache-2.0 or MIT, at the user's option. The
repository ships [LICENSE-APACHE](LICENSE-APACHE) and
[LICENSE-MIT](LICENSE-MIT). Contributions
are accepted under the same dual-license terms.

## 15. ADR index

- [0001: Plugin isolation](docs/adr/0001-plugin-isolation.md)
- [0002: Go gateway role](docs/adr/0002-go-daemon.md)
- [0003: IDL strategy](docs/adr/0003-idl.md)
- [0004: Text model](docs/adr/0004-text-model.md)
- [0005: Exec policy](docs/adr/0005-exec-policy.md)
- [0006: Upstream dependencies](docs/adr/0006-upstream-dependencies.md)
- [0007: JSON-RPC envelope and capability token](docs/adr/0007-jsonrpc-envelope-and-capability-token.md)
- [0008: Selection-first interaction](docs/adr/0008-selection-first-interaction.md)
- [0009: Text-model snapshot gate](docs/adr/0009-text-model-snapshot-gate.md)
- [0010: Cursor and snapshot ownership](docs/adr/0010-cursor-and-snapshot-ownership.md)

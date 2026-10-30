# Anvil Architecture

Status: Phase 0 baseline.

## 1. Process model

```text
┌──────────────────────────────────────────────┐
│ Editor (Rust, trusted)                       │
│ input · text buffer · view · snapshots        │
│ command registry · plugin lifecycle           │
└───────┬──────────────────────────────┬────────┘
        │ editor API UDS               │ control UDS
        │ (per plugin host)            │ (grants, health)
┌───────▼───────────────┐              │
│ Plugin host (host A)  │              │
│ Lua/QuickJS + sandbox │              │
└───────┬───────────────┘              │
        │ sys.* UDS + token            │
┌───────▼──────────────────────────────▼────────┐
│ Gateway (Go, trusted, per session)            │
│ policy · openat2 · fetch · exec · process     │
└───────────────────────┬───────────────────────┘
                        │
                files / network / child processes
```

`sys.exec` adds one hop: the gateway invokes `anvil-exec-runner`, which applies
Landlock/seccomp and then executes the tool. The gateway never execs a tool
directly from its unsandboxed address space.

A second plugin is a second host process. One host may serve multiple plugins
only when they are in the same explicitly declared trust group.

### Responsibilities

**Editor**

- Owns the text buffer, selection, viewport, input, command registry, and
  snapshots.
- Decides which plugins are enabled in a workspace.
- Issues a per-plugin capability token and registers the grant with the gateway.
- Serves `editor.*`, `ui.*`, and `event.*` requests from the plugin host.
- Never executes plugin code.

**Plugin host**

- Owns Lua/QuickJS and the plugin's lifetime.
- Starts with no ambient filesystem, network, or process authority.
- Applies Landlock/seccomp/`no_new_privs` before loading plugin code.
- Serves plugin calls; routes `editor/ui/event` to the editor and `sys.*` to the
  gateway.
- Is untrusted after compromise; it can only use its own token and sandbox.

**Gateway**

- Per-session, unprivileged, started by the editor.
- Enforces `sys.*` policy and performs the actual file/network/process I/O.
- Does not host plugin runtimes.
- Does not trust a plugin host merely because it is on a local socket.
- Fails closed on policy error, malformed frames, missing grants, or cancelled
  requests.

**Exec runner**

- Minimal trusted launcher used only by `sys.exec`.
- Receives a workspace dirfd, tool path fd, argv, and limits from the gateway.
- Applies read-only Landlock rules for the workspace and system runtime paths,
  seccomp (no network, ptrace, or privilege changes), and `no_new_privs`.
- Executes with `execveat`; the gateway does not pass path strings as authority.
- If the runner is unavailable or sandbox setup fails, `sys.exec` is disabled.

The gateway is **not** the plugin sandbox. It is the policy enforcement point for
requests that reach it. The kernel sandbox around the plugin host is the
isolation boundary.

## 2. Process lifecycle

### Session start

1. Editor creates `$XDG_RUNTIME_DIR/anvil/<session-id>/` with mode `0700`.
2. Editor starts the gateway; both open control UDS with `0600`.
3. Gateway creates its `sys` listener and reports readiness.
4. Editor starts a plugin host for each enabled plugin, with a fresh token.

### Plugin start

1. Editor validates manifest and permission grant for the workspace.
2. Editor registers `(plugin_id, token_hash, workspace_root, permissions)` with
   the gateway.
3. Editor spawns the host in a new process group.
4. Host applies sandbox rules, connects to editor and gateway, and reports its
   plugin id and token.
5. Gateway checks token, plugin id, workspace, and peer credentials before the
   first `sys.*` request.
6. If any step fails, the host is killed and the plugin is disabled.

### Reload

1. Stop accepting new plugin requests.
2. Cancel in-flight request ids.
3. Kill the host process group and wait for it.
4. Revoke token and unregister grants.
5. Start a new host, apply sandbox, issue a new token.

No resource is cleaned up only by a language-level `Drop`.

### Shutdown

- Editor closes control UDS.
- Gateway receives EOF, closes listeners, kills child process groups, exits.
- Plugin hosts receive EOF, stop the VM, and exit.
- A gateway crash must not crash the editor; plugin `sys.*` calls fail closed and
  the editor can restart the gateway.

## 3. IPC

### Framing

- Unix domain sockets only; no TCP, no abstract-namespace sockets in v1.
- 4-byte big-endian length + UTF-8 JSON-RPC 2.0 object.
- Max frame 8 MiB. Oversize frames close the connection with an error.
- Every request has an id and deadline.
- Cancellation uses the same id; a cancelled id never returns success later.

Envelope fields, `session.hello`, capability-token semantics, error codes, and
event notifications are defined by
[ADR-0007](adr/0007-jsonrpc-envelope-and-capability-token.md).

### Endpoints

| Endpoint | Client | Server | Purpose |
|---|---|---|---|
| `editor.sock` | plugin host | editor | `editor.*`, `ui.*`, `event.*` |
| `gateway.sock` | plugin host | gateway | `sys.*` |
| `control.sock` | editor | gateway | grants, health, shutdown |

`SO_PEERCRED` identifies the peer process. The capability token identifies the
plugin. Neither is sufficient alone.

### API routing

```text
plugin --editor.get_view-------> editor
plugin --editor.apply_edit-----> editor (permission checked)
plugin --editor.register_cmd---> editor
plugin --ui.notify/status------> editor
plugin --event.subscribe-------> editor
plugin --sys.read_file---------> gateway
plugin --sys.fetch-------------> gateway
plugin --sys.exec--------------> gateway -> exec-runner -> tool
```

The gateway never proxies editor state. The editor never performs ambient
filesystem/network I/O on behalf of a plugin.

## 4. Editor and text model

- Public `TextBuffer` trait separates the editor from the concrete storage.
- Phase 0 benchmark chooses Rope/piece table or Gap Buffer. Only one backend
  ships in v1; a second backend is not a parallel maintenance path.
- The snapshot contains `Arc<Document>` plus viewport/selection/revision.
  It must not clone 100 MB of text per frame.
- Rendering covers only visible lines; syntax highlighting is incremental and
  viewport-scoped.
- Byte, char, grapheme, and display-cell coordinates are explicit in the API.
  The editor owns conversions; plugins must not reimplement Unicode width rules.

## 5. Ferriso: reference, not a v1 runtime dependency

Anvil does not depend on `ferriso-core`, `ferriso-tui`, or `ferriso-editor` in
v1. Ferriso remains a useful MIT-licensed reference:

- `ferriso-editor` shows a gap buffer, segmented line index,
  grapheme/visual coordinates, undo, and atomic save.
- `ferriso-tui` shows a ratatui/crossterm terminal lifecycle.
- Copy or adapt only small, attributable pieces; do not expose Ferriso types in
  the public API.
- Ferriso 0.3.x and its Tokio actor assumptions are not a stable base for a
  100 MB editor with a separate Go gateway.
- `ferriso-editor` defaults to a 64 MiB open limit; Anvil must set its own limit
  and retest at 100 MB if its code is reused.

Revisit this decision only if the direct `ratatui` + Rope stack fails a Phase 0
gate.

## 6. Build and packaging

Proposed workspace:

```text
crates/
  anvil-core/          text model, commands, snapshots
  anvil-tui/           terminal frontend
  anvil-ipc/           UDS transport, JSON-RPC framing
  anvil-api/           public Rust types, schema, generated stubs
  anvil-test/          security and performance harnesses
  anvil-exec-runner/   sandboxed launcher for rg/fd
plugin-host/           Lua/QuickJS host + sandbox
go/                    Go module: cmd/anvil-gateway, internal/{policy,mediation,audit}
docs/
```

- Rust toolchain and Go toolchain are pinned.
- `cargo-deny` and Go license checks are CI gates.
- The gateway and editor are packaged together but versioned independently.
- No gateway is installed as a global service; it is per session.

## 7. Failure modes

| Failure | Behavior |
|---|---|
| Gateway crash | Plugin `sys.*` fails; editor survives; gateway restart allowed |
| Plugin host crash | Plugin disabled; editor and other plugins continue |
| Sandbox setup failure | Plugin not started; no fallback to ambient access |
| Editor crash | Gateway sees EOF, kills children, exits |
| Malformed frame | Connection closed; plugin disabled |
| Cancelled request | I/O and process tree killed; no success response |
| Stale token | Request rejected; plugin host restarted |
| Exec runner missing | `sys.exec` disabled; no unsandboxed fallback |

## 8. Deliberate exclusions

- No plugin runtime in the editor process.
- No global gateway shared by workspaces.
- No shell/pipeline execution in v1.
- No native Lua C modules in v1.
- No claim of cross-platform sandbox parity.

## 9. Open questions

- Landlock baseline: full support requires Linux 5.13; kernel 5.6-5.12 runs
  trusted plugins only.
- Whether the first plugin host is Rust-native or Wasm-hosted; ADR-0001 keeps
  Rust-native for Lua/QuickJS and defers Wasm.
- Whether a plugin host can serve a trust group; default is one host per plugin.

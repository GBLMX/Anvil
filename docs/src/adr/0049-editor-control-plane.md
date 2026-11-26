# ADR-0049: Editor Control Plane and MCP Adapter Boundary

- Status: Accepted; implemented 2026-11-17
- Version: 1.0.0
- Implementation: workspace-keyed `anvil-control` socket, protocol v3,
  `anvil-ctl`, and the thin `anvil-mcp` stdio adapter landed
- Date: 2026-11-17
- Related: [ADR-0007](0007-jsonrpc-envelope-and-capability-token.md),
  [ADR-0023](0023-internal-capability-closure.md),
  [ADR-0024](0024-tui-state-and-domain-boundaries.md),
  [ADR-0031](0031-desktop-url-opener-module.md),
  [ADR-0042](0042-selection-first-session-frontend.md)
- Supersedes:
- Superseded by:

## Context

Anvil has two independent local planes:

- the per-session gateway, which owns file, LSP, fetch, exec, and plugin
  `sys.*` policy;
- the editor control plane, which lets local clients inspect and drive the
  running TUI.

The control plane is implemented but had no ADR. `anvil-control`,
`anvil-ctl`, and `anvil-mcp` already depend on its socket path, protocol
version, capability model, and path policy. Without a recorded decision, later
clients can accidentally turn it into a second gateway, a remote shell, or a
second command manifest.

The control plane is not the plugin grant channel. A plugin reaches the editor
through `editor.sock` and the gateway through `sys.*`; it must never receive a
control socket or control grant.

## Decision

### 1. One editor-owned workspace socket

The control plane has exactly one server owner: the editor runtime. It creates
one workspace-keyed socket and lock pair under
`$XDG_RUNTIME_DIR/anvil/`, falling back to
`~/.local/state/anvil/runtime/anvil/` when `XDG_RUNTIME_DIR` is unset.

The socket is local Unix-domain only, mode `0600`, and keyed by the canonical
workspace path. There is no TCP listener, abstract-namespace socket, or global
control socket shared by workspaces.

### 2. Same-UID peers only

The server checks peer credentials before accepting a request. A peer whose UID
does not match the editor's effective UID is rejected. File mode and workspace
hash are not authentication.

The listening surface is bounded: at most eight concurrent control connections
and a five-second per-connection I/O deadline. Exhaustion rejects the new
connection rather than queueing without bound.

### 3. Every request runs through the TUI actor

Control clients never mutate editor state or workspace files directly. A typed
request is enqueued on the same TUI actor that owns input and rendering, and the
response is bounded and typed.

Protocol v3 exposes:

```text
control.ping
control.status
control.open
control.save
control.edit
control.query
control.exec
control.quit
```

`control.query` accepts line bounds and caps returned text at 256 KiB.
`control.edit` supports `set`, `insert`, and revision-guarded `replace`.
`anvil-ctl` caps `--stdin` payloads at 4 MiB. These are protocol bounds, not
client-side suggestions.

### 4. Capabilities come from the shared command manifest

Every control command resolves through the shared `anvil-control` command
manifest. The capability set is:

```text
read
edit
git
admin
```

`admin` is not allowed on the default remote surface. Plugin lifecycle,
package management, theme/settings persistence, and similar admin commands are
denied through `control.exec` and through the MCP adapter. Unknown commands are
denied rather than forwarded to the TUI.

`anvil-ctl` and `anvil-mcp` do not maintain separate command tables. MCP tools
and command names are derived from the same manifest and filtered by
`Capability::is_remote_allowed()`.

### 5. Path authority stays structured

The control plane does not accept arbitrary path-bearing Ex commands. Forms
such as `:e <path>`, `:w <path>`, and `:wq <path>` are rejected through
`control.exec`.

Clients use `control.open` and `control.save`, which enforce the workspace
boundary and route through the editor actor. `control.query` and
`control.edit` operate on the active buffer or an already-open buffer; they do
not read or write workspace files directly.

### 6. MCP is a thin adapter, not a second server

`anvil-mcp` is a stdio protocol adapter over the same control socket. It does
not read or write workspace files, does not own a gateway, and does not create
a second command manifest.

MCP tool calls become typed `anvil-control` requests. Tool listing is filtered
by manifest capability and never exposes `admin` tools. MCP connection failure,
timeout, or missing runtime directory maps to a bounded tool error, not a
fallback that writes files directly.

### 7. Failure and versioning are explicit

- Malformed frames, oversize payloads, unknown methods, unknown commands, and
  capability denials return explicit errors.
- A control timeout does not leave a second actor path or a partial write.
- Replacing or adding a control method requires a protocol-version review.
- Remote transport, multi-user access, and a second control server require a
  new ADR.
- The control plane remains separate from gateway grants and plugin tokens.

## Consequences

Positive:

- Local automation has one editor-owned path with one capability and path
  policy.
- `anvil-ctl` and MCP cannot drift into separate command or permission models.
- Plugin `sys.*` and control `control.*` remain independently auditable.
- Bounded connections, timeouts, query sizes, and edit revisions limit
  accidental resource growth.

Negative:

- Every new automation surface must update the shared manifest and both
  adapters.
- Admin automation requires a future explicit remote policy rather than a
  local socket shortcut.
- The five-second I/O and eight-connection limits may reject pathological
  clients; they must reconnect.

## Verification

- Focused tests:
  - same-UID peer acceptance and UID mismatch rejection;
  - workspace socket stability and distinction;
  - connection-count and I/O-timeout behavior;
  - query 256 KiB and CLI 4 MiB payload bounds;
  - path-bearing `:e`/`:w`/`:wq` rejection;
  - admin capability denial on the default remote surface;
  - revision-guarded `control.edit`;
  - MCP tool list derives from the shared manifest and omits admin tools.
- Failure/cancellation tests:
  - missing runtime directory and missing socket;
  - client timeout and malformed/unknown request;
  - admin command rejected before connecting.
- Real surface smoke:
  - `scripts/ctl-smoke.sh` drives the installed TUI through `anvil-ctl`,
    including save, undo/redo, Git staging, query, and forced quit.

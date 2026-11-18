# ADR-0056: MCP Dual-Era Protocol and Structured Tool Contract

- Status: Proposed
- Version: 1.0.0
- Date: 2026-11-19
- Related: [ADR-0006](0006-upstream-dependencies.md),
  [ADR-0023](0023-internal-capability-closure.md),
  [ADR-0037](0037-kernel-api-baseline.md),
  [ADR-0049](0049-editor-control-plane.md)
- Supersedes:
- Superseded by:

## Context

`anvil-mcp` is the thin stdio MCP adapter over the workspace-scoped
`anvil-control` socket. It currently advertises MCP `2024-11-05` and implements
only `initialize`, `ping`, `tools/list`, and `tools/call`. It returns tool
results as text content only and exposes no discovery metadata, structured
output, tool annotations, or revision guard for whole-buffer edits.

The current MCP revision is `2026-07-28`. It makes the protocol stateless,
removes the `initialize` handshake and `ping`, requires `server/discover` and
per-request `_meta`, requires `resultType`, and moves list-change delivery to
`subscriptions/listen`. It also standardizes structured tool output, tool
annotations, and cacheable list results.

A modern-only client against the current adapter either falls back to the
legacy handshake or fails. A legacy-only client against a naive
`2026-07-28`-only server fails. The adapter therefore needs an explicit
compatibility policy rather than a version-string bump.

The adapter must remain a consumer of the control contract. ADR-0049 keeps MCP
separate from the gateway, plugin grants, and arbitrary file access. ADR-0006
keeps Tokio out of the current dependency baseline; the official Rust SDK is
Tokio-based and cannot be adopted without a separate dependency decision.

## Decision

### 1. Serve both protocol eras

`anvil-mcp` serves:

- **modern**: MCP `2026-07-28`, using per-request metadata and
  `server/discover`;
- **legacy**: `initialize`-based clients, accepting `2024-11-05`,
  `2025-03-26`, `2025-06-18`, and `2025-11-25`.

`server/discover` advertises only `2026-07-28`. Legacy versions are negotiated
through `initialize` and are intentionally not offered to modern clients as
discoverable versions.

Era selection is deterministic:

- `initialize` selects legacy mode for that process and echoes the requested
  version when it is in the supported legacy set;
- a request carrying `_meta` with
  `io.modelcontextprotocol/protocolVersion` selects modern handling;
- `server/discover` is accepted before any mode is selected so a modern client
  can probe the adapter;
- a request that selects neither mode is rejected rather than guessed.

A process must not mix legacy and modern semantics.

### 2. Implement the modern wire contract directly

The adapter keeps its synchronous, single-request-at-a-time architecture and
adds a small typed protocol layer. It does not adopt the official Tokio-based
Rust SDK for this scope.

Modern behavior:

- `server/discover` returns `supportedVersions`, `capabilities`, `_meta`
  server info, `instructions`, `ttlMs`, and `cacheScope`;
- every modern result carries `resultType: "complete"`;
- every modern result carries
  `_meta["io.modelcontextprotocol/serverInfo"]`;
- `tools/list` is deterministic, uses `ttlMs = 60000` and
  `cacheScope: "private"`, and does not vary by connection;
- `ping` is legacy-only; modern `ping` is an unknown method;
- unknown methods return `-32601`;
- malformed JSON is `-32700`; a malformed JSON-RPC envelope is `-32600`;
  missing or invalid params and unknown tools are `-32602`;
- an unsupported modern version returns `-32022`
  (`UnsupportedProtocolVersion`) with the supported and requested versions;
- control-layer RPC errors remain tool execution errors and are never reused
  as MCP protocol error codes;
- no protocol-level session, connection-scoped tool list, or server-initiated
  request is introduced.

`server/discover` and `tools/list` must not require a running TUI. Tool calls
may return the existing `no_instance` error.

### 3. Make tool results structured and additive

Keep the existing seven tools. Do not add resources, prompts, subscriptions,
tasks, MRTR, HTTP, OAuth, or an `anvil_spawn` tool.

Every tool gains:

- a human-readable `title`;
- `annotations` with conservative read-only/destructive/idempotent hints;
- a success `outputSchema`;
- both text content and `structuredContent` on success.

Text content remains for clients that do not consume structured output.
Successful structured results must conform to the declared output schema.
Execution failures use `isError: true` with actionable text and omit
`structuredContent`.

`anvil_command` keeps the shared manifest allowlist and never exposes admin
commands. Tool descriptions state that the workspace boundary and admin policy
are enforced by the TUI actor.

### 4. Guard mutations with revisions

MCP `anvil_edit` accepts `expected_revision` on every mode. It is required for
`set` and `replace`, and optional for `insert`. The input schema expresses the
three modes with `oneOf` so a model sees the required fields per mode.

A stale revision fails without changing the buffer. The adapter does not retry
mutations automatically and must not emulate the guard with a query followed by
an unguarded edit. The TUI actor enforces the check atomically for `set`,
`insert`, and `replace`.

`anvil_save` and `anvil_quit` remain destructive-hinted tools. `anvil_quit`
keeps the explicit `force` flag.

### 5. Bring the CLI to control-contract parity

`anvil-ctl` gains:

```text
anvil-ctl [OPTIONS] edit replace <START:END> <TEXT|--stdin> --expected-revision <N>
```

The range uses the same one-based line and byte-column semantics as
`control.edit`. The CLI and MCP continue to share `anvil-control`; there is no
new MCP-only command table and no control protocol version bump.

### 6. Keep the control surface and wire shape unchanged

This decision adds no control method, no wire-shape change, no path bypass, and
no gateway change. The only control-plane behavior change is that the existing
optional `control.edit.expected_revision` is honored for `set` and `insert`;
`replace` already requires it. Callers that omit the field keep the previous
unguarded behavior, so protocol v3 remains readable by existing clients.

All MCP operations continue through the existing `control.status`,
`control.open`, `control.query`, `control.edit`, `control.save`,
`control.exec`, and `control.quit` calls.

If a future tool needs data the control plane does not expose, that is a
separate control-plane ADR. It is not permission to read workspace files inside
`anvil-mcp`.

### 7. Revisit the SDK decision only on measured need

The handwritten protocol layer is selected because the current adapter surface
is small and ADR-0006 rejects Tokio by default.

Revisit the official Rust SDK when any of the following is true:

- resources, prompts, subscriptions, MRTR, or tasks are added;
- dual-era protocol maintenance produces repeated compatibility defects;
- the protocol layer grows beyond the tool-only surface enough that an SDK is
  cheaper than local tests and maintenance.

Adopting the SDK requires an ADR-0006 amendment and a dependency/license
review.

## Consequences

Positive:

- Modern-only and legacy clients both work during the protocol transition.
- Structured tool output and schemas make the adapter easier to consume
  without removing text compatibility.
- Revision-guarded edits reduce accidental data loss from stale automation.
- The adapter stays synchronous, small, and independent of Tokio.
- The control protocol wire shape, gateway, and plugin capability boundaries
  remain unchanged; only existing edit revision semantics are enforced.

Negative:

- The adapter owns two protocol paths and must test both.
- The handwritten protocol layer must track future MCP revisions.
- Some clients ignore `structuredContent`, `annotations`, or cache fields.
- Requiring revisions for `set` and `replace` is a breaking change for unsafe
  callers; `insert` remains optional.
- A static tool list provides no push-based list-change notification; clients
  must re-list when they need freshness.

## Verification

- Focused tests:
  - modern `server/discover` without a running TUI;
  - modern `tools/list` order, cache fields, and metadata;
  - modern `tools/call` `resultType`, text, and `structuredContent`;
  - legacy `initialize` for every supported legacy version;
  - unsupported version returns `-32022` with version data;
  - malformed JSON is `-32700`; a malformed JSON-RPC envelope is `-32600`;
    missing or invalid params and unknown tools are `-32602`;
  - admin command remains denied before connecting;
  - stale `set`, `insert`, and `replace` revisions leave the buffer unchanged;
  - `tools/list` does not vary between connections.
- Failure/cancellation tests:
  - no runtime directory, no socket, and no running TUI;
  - control timeout and control RPC failure remain tool execution errors;
  - notifications do not produce responses.
- Real surface smoke:
  - `scripts/mcp-smoke.sh` runs both modern and legacy transcripts against a
    real TUI;
  - `scripts/ctl-smoke.sh` covers CLI `edit replace` and revision rejection;
  - Claude Code, Codex, and MCP Inspector are used for dogfood where available.

## References

- MCP 2026-07-28 key changes:
  <https://modelcontextprotocol.io/specification/2026-07-28/changelog.md> —
  stateless protocol, discovery, result types, subscriptions, and deprecated
  features.
- MCP versioning and legacy compatibility:
  <https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning.md>
  — modern/legacy era selection and the compatibility matrix.
- MCP tools:
  <https://modelcontextprotocol.io/specification/2026-07-28/server/tools.md> —
  tool schemas, annotations, structured output, errors, and list caching.
- MCP discovery:
  <https://modelcontextprotocol.io/specification/2026-07-28/server/discover.md>
  — mandatory `server/discover` and `DiscoverResult` fields.
- MCP stdio transport:
  <https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio.md>
  — line framing, shutdown, cancellation, and stdio backward-compatibility
  probing.
- Official Rust SDK:
  <https://github.com/modelcontextprotocol/rust-sdk> — supports
  `2026-07-28` and `2025-11-25`, and is the Tokio-based alternative considered
  in Decision 7.

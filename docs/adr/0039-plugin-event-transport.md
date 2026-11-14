# ADR-0039: Plugin Event Transport and Interaction Boundary

- Status: Proposed
- Implementation: `event.subscribe`, `host.event`, and `diagnostics.changed`
  landed with a typed registry, bounded command/event queues, delivery stats,
  sequence rejection, and latest-event buffering before handler registration.
  Other event families remain proposed.
- Date: 2026-11-14
- Related: [ADR-0003](0003-idl.md), [ADR-0012](0012-plugin-host-isolation.md),
  [ADR-0014](0014-plugin-api-v1-freeze.md),
  [ADR-0017](0017-plugin-activation-and-lifecycle.md),
  [ADR-0024](0024-tui-state-and-domain-boundaries.md),
  [ADR-0035](0035-plugin-wit-seams.md),
  [ADR-0037](0037-kernel-api-baseline.md),
  [ADR-0038](0038-project-settings-trust.md),
  [tactical interaction plan](../plugin-mouse-canvas-plan.md),
  [AI telemetry plan](../ai-telemetry-plan.md)

## Context

`event.subscribe` is present in the v1 method catalog and generated bindings,
but the editor dispatcher in `crates/anvil-tui/src/plugin_document.rs` has no
implementation. Plugins can request editor snapshots, edits, commands,
providers, notifications, and status, but cannot receive editor events.

The next proposed features need events:

- mouse capture for RTS-style multi-cursor selection;
- in-memory graph deltas for the heartbeat dependency graph;
- coalesced diagnostics for Alert System and graph error markers;
- optional metrics snapshots for the AI telemetry HUD.

Adding one RPC method per feature would grow the plugin contract ad hoc and
repeat queue, lifecycle, and stale-event handling. Adding a global mouse
subscription or a full-screen raw canvas would also let plugins bypass core
layout, input, and UI ownership.

The current v1 surface already demonstrates the maintenance risk: a method can
be declared and generated without a dispatcher. The contract must distinguish
“declared” from “implemented” before adding more events or interaction APIs.

## Decision

### 1. Complete `event.subscribe` before adding interaction methods

`event.subscribe` is an existing v1 method. Implementing it is contract
completion, not a version bump. No new mouse, canvas, overlay, or metrics
method may land while `event.subscribe` remains a no-op.

The v1 dispatcher must either implement a method or return a typed
`METHOD_NOT_FOUND`/`NOT_IMPLEMENTED` error consistently with the catalog.
Generated bindings alone do not count as implementation.

### 2. One typed, bounded event transport

The editor sends plugin events through the existing plugin-host RPC channel.
The transport is owned by `PluginSupervisor` and the host proxy, not by
individual TUI features.

Event envelope:

```json
{
  "jsonrpc": "2.0",
  "method": "host.event",
  "params": {
    "subscription_id": 7,
    "event": "diagnostics.changed",
    "sequence": 42,
    "payload": {}
  }
}
```

Rules:

- `host.event` is an editor/proxy-to-plugin-host notification only. It is a
  host-internal method, excluded from the plugin-callable method catalog.
  A plugin-originated `host.event` is rejected with `METHOD_NOT_FOUND`; it is
  never relayed back through the proxy.
- The event name is from a static allowlist; unknown names are rejected.
- `sequence` is monotonic per subscription. Stale/replayed sequences are
  dropped.
- Delivery is bounded per plugin. High-frequency events use latest-value
  coalescing or explicit deltas; the editor never blocks on a plugin queue.
- Subscription IDs are owned by the plugin host; plugin stop/unsubscribe removes
  them and drops queued events.
- Event payloads are fixed DTOs, not arbitrary editor internals or full LSP
  responses.
- A failed delivery is reported as a plugin lifecycle error; it does not crash
  the editor.

### 3. Event families

Initial allowed events:

| Event | Payload |
|---|---|
| `diagnostics.changed` | path, generation, error/warning counts, bounded first-error text |
| `workspace.graph_changed` | root, generation, bounded node/edge deltas, removed paths |
| `metrics.changed` | source-filtered metrics snapshot |
| `editor.mouse` | only to the active capture owner |

`event.subscribe` is not a global mouse subscription. Mouse events require an
exclusive `editor.begin_mouse_capture` lease from the same plugin command.
`event.subscribe` for `editor.mouse` without a capture returns a typed error.

### 4. Source and ownership

- Editor/session events carry an explicit source (`editor`, `mcp`, `plugin`)
  where the event has an actor; metrics and failure counters follow the same
  tagging. No inference from “MCP connected”.
- Plugins receive snapshots/deltas, not ownership of editor state.
- A plugin cannot mutate another plugin's overlay or graph state. Each plugin
  owns its own layer IDs and subscriptions.
- The TUI remains the input/render adapter. It may route mouse events to a
  capture owner, but does not execute graph layout, dependency parsing, or
  plugin algorithms.
- The core may rasterize bounded lines (`ui.draw_lines`) or composite sparse
  overlay cells; a raw full-screen cell matrix is not an event payload.

### 5. API versioning

- Completing `event.subscribe` stays within v1.
- New capture/overlay/selection methods are additive v1.1 APIs with capability
  gates and generated bindings.
- Breaking changes or removal require a new major contract, not silent edits to
  v1 DTOs.
- The plugin method catalog, Rust DTOs, JSON Schema, TypeScript, Luau, and
  dispatcher parity are verified in CI. `host.event` is deliberately absent
  from the plugin-callable catalog and has its own host-internal handler.

### 6. Backpressure and lifecycle

- Bounded queue per plugin; overflow policy is latest-value for progress-style
  events and drop-oldest for deltas only when the payload explicitly supports
  recovery by snapshot/generation.
- No event delivery from the editor hot path may allocate unbounded memory or
  wait on a lock held by the plugin host.
- Subscription cleanup runs on plugin stop, host crash, command cancellation,
  and editor shutdown.
- Shutdown order remains: stop TUI-dependent work, stop workers, stop plugin
  hosts, close gateway clients, then stop gateways.

## Rejected alternatives

1. **Global mouse subscription.** It lets any plugin observe/capture input
   without user intent and conflicts with input ownership.
2. **Raw full-screen cell matrices.** They bypass layout, bounds, status-line
   ownership, and performance limits.
3. **One RPC method per feature event.** It duplicates queues, generations, and
   cleanup, and makes the API catalog inconsistent.
4. **Direct plugin-to-plugin overlay mutation.** It creates hidden ownership
   and makes cleanup/crash behavior undefined.
5. **Sending full LSP diagnostics or graph snapshots on every change.** It
   amplifies storms and blocks the editor; summaries/deltas plus bounded
   snapshots are required.
6. **Adding v1.1 methods before completing v1.** It worsens contract drift.

## Consequences

Positive:

- New plugin capabilities share one transport, backpressure policy, and
  lifecycle.
- Mouse, graph, alerts, and telemetry can be implemented without cross-plugin
  state mutation.
- The v1 catalog becomes honest: declared methods have dispatchers or explicit
  unsupported errors.

Negative:

- Event DTOs and generation/sequence semantics add contract surface.
- The plugin host needs a push path in addition to its request/reply path.
- Some features must wait for the transport instead of shipping as isolated
  RPCs.

## Verification

- Method-catalog/dispatcher parity tests for every v1 method.
- Direction test: editor→host `host.event` is delivered; plugin-originated
  `host.event` returns `METHOD_NOT_FOUND` and is never relayed; the method is
  absent from the plugin-callable catalog.
- Subscription test: one event delivered once; unknown event rejected; stale
  sequence dropped; plugin stop removes the subscription.
- Overflow test: high-frequency diagnostics/graph events coalesce without
  blocking the editor; snapshot recovery works after dropped deltas.
- Lifecycle test: host crash, command cancellation, and editor shutdown leave no
  subscription or queued event.
- API generation parity: Rust/JSON Schema/TypeScript/Luau.
- TUI smoke: capture owner receives mouse events; non-owner receives none;
  plugin cannot mutate another plugin's overlay.

## Non-goals

- A stable Wasm/ABI plugin runtime.
- Arbitrary plugin audio/network access.
- Full-screen raw canvas or global input capture.
- Plugin-defined editor core state.
- Replacing the existing gateway JSON-RPC protocol.

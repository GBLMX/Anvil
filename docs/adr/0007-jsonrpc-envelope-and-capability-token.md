# ADR-0007: JSON-RPC Envelope and Capability Token Semantics

- Status: Proposed; Phase 0 contract
- Date: 2026-10-30
- Note: requested as ADR-0006, but ADR-0006 is the upstream dependency
  baseline. This is the next free number.

## Context

The editor, Go gateway, and plugin host must agree on:

- how a request is identified and traced;
- where a capability token lives;
- how errors are classified;
- how events differ from requests;
- how cancellation and timeouts are expressed;
- what a connection may do before authentication.

These decisions are cross-language and expensive to change after the Lua and
QuickJS hosts exist.

## Decision

### 1. Transport and envelope

The transport is a per-session Unix socket. Each frame is the 4-byte
length-prefixed JSON-RPC object from `anvil-ipc`.

A request is:

```json
{
  "jsonrpc": "2.0",
  "id": "01J...",
  "method": "sys.read_file",
  "params": {},
  "meta": {
    "trace_id": "01J...",
    "deadline_ms": 5000
  }
}
```

- `jsonrpc` MUST be the string `"2.0"`.
- `id` MUST be a string of 1-64 printable ASCII characters, unique per
  connection while the request is in flight. String ids avoid JavaScript's
  integer precision limit and make trace correlation explicit.
- `method` MUST be a non-empty string from the frozen API or a control method.
- `params` MUST be an object. Methods that take no arguments use `{}`.
- `meta` is an Anvil extension. Unknown members are ignored by this
  implementation; strict unknown-field rejection is a method-level schema
  choice.
- `trace_id` is optional; the receiver generates one when absent.
- `deadline_ms` is optional. It is a transport-level upper bound, not a
  replacement for method-specific operation timeouts. The gateway enforces the
  lower of the envelope deadline, method policy, and its own default.
- `token` is **not** an envelope field. It is sent once in `session.hello`.

The top-level `meta` object is an extension to the four standard JSON-RPC
members. Both peers are Anvil components, so no generic JSON-RPC client is
assumed.

### 2. Capability token

- The editor generates a 32-byte random token per plugin host.
- The token is opaque; it is not a signed bearer token in v1.
- The editor passes it to the plugin host through an inherited file descriptor,
  never through argv, environment variables, a log line, or the plugin's Lua/JS
  environment.
- The editor registers `hash(token)`, plugin id, workspace root, permissions, and
  peer expectations with the gateway before the host starts.
- The gateway stores only a hash, performs a constant-time comparison, and never
  logs the raw token.
- `SO_PEERCRED` supplies uid/gid/pid. The token supplies plugin identity and
  grants. Neither is sufficient alone.
- One token is one plugin-host session. Reload revokes the old token and issues
  a new one. Disconnect revokes it.
- A token is not per request and not reusable across workspaces. Per-request
  tokens were rejected because they increase leakage and logging exposure
  without improving revocation.
- Signed/stateless tokens are deferred. They require key distribution and
  rotation, which v1 does not need.

### 3. Handshake

Before any API method, each plugin connection sends:

```json
{
  "jsonrpc": "2.0",
  "id": "01J...",
  "method": "session.hello",
  "params": {
    "token": "<base64url>",
    "api_version": "0.1",
    "protocol": "anvil-jsonrpc-1"
  }
}
```

The receiver validates the token, `SO_PEERCRED`, plugin id, workspace, and
protocol version. It returns:

```json
{
  "jsonrpc": "2.0",
  "id": "01J...",
  "result": {
    "session_id": "...",
    "plugin_id": "...",
    "api_version": "0.1",
    "permissions": ["fs.read:src/**", "network:GET:https://api.github.com"]
  }
}
```

Before a successful handshake, every method other than `session.hello` fails
with `session_required`. A second `session.hello` on the same connection is
invalid.

`session.hello`, `anvil.cancel`, and `event.deliver` are control methods, not part
of the ten public API methods.

### 4. Errors

Errors use JSON-RPC's `error` object. `code` is a stable number; clients MUST NOT
parse `message`.

| Code | Kind | Meaning |
|---:|---|---|
| -32700 | `parse_error` | Invalid JSON |
| -32600 | `invalid_request` | Invalid envelope |
| -32601 | `method_not_found` | Unknown method |
| -32602 | `invalid_params` | Method schema failed |
| -32603 | `internal_error` | Receiver bug |
| -32001 | `unauthorized` | Missing/invalid token or peer |
| -32002 | `policy_denied` | Capability policy rejected |
| -32003 | `session_required` | Handshake missing |
| -32004 | `cancelled` | Cancelled by `anvil.cancel` |
| -32005 | `timeout` | Deadline exceeded |
| -32006 | `limit_exceeded` | Size/count/resource limit |
| -32007 | `not_found` | Path/object missing |
| -32008 | `unavailable` | Gateway/runner unavailable |
| -32009 | `conflict` | Revision/state conflict |

`error.data` may contain:

```json
{
  "kind": "policy_denied",
  "retryable": false,
  "trace_id": "01J...",
  "details": {}
}
```

`details` must not contain tokens, credentials, or raw file contents.

Transport/frame failures are not JSON-RPC errors. They close the connection or
return a transport error to the local caller.

### 5. Events

`event.subscribe` is a normal request. It returns a subscription id. Events are
**notifications** with no `id`:

```json
{
  "jsonrpc": "2.0",
  "method": "event.deliver",
  "params": {
    "subscription": "...",
    "sequence": 42,
    "event": "buffer.changed",
    "data": {}
  }
}
```

- Notifications never receive a response.
- Events use the same UDS connection and remain ordered per connection.
- `sequence` is monotonic per subscription.
- Delivery uses a bounded queue.
- On overflow, the receiver emits `event.lost` with the missed sequence range,
  then closes the subscription. It does not silently drop events.
- Events are not replayed across plugin-host crashes or reconnects. A host that
  restarts must subscribe again.
- Event payloads have their own size limit, lower than the 8 MiB frame limit.

### 6. Cancellation and deadlines

`anvil.cancel` is a notification:

```json
{
  "jsonrpc": "2.0",
  "method": "anvil.cancel",
  "params": { "request_id": "01J..." }
}
```

- Cancellation is idempotent.
- The receiver stops I/O and kills the request's child process group.
- A cancelled request never returns success.
- Cancellation does not revoke the capability token; only reload/disconnect
  does.
- A request that exceeds `deadline_ms` returns `timeout` and is cancelled using
  the same path.

### 7. Limits

- Frame body: 8 MiB (already enforced by `anvil-ipc`).
- One in-flight request per `id` per connection; duplicate ids are
  `invalid_request`.
- Bounded concurrent requests per plugin; excess is `limit_exceeded`.
- Method-specific limits (`fs` 5 MiB, fetch body, exec output) are lower and
  enforced by the gateway.
- Oversized notifications close the connection rather than allocating without
  bound.

## Consequences

Positive:

- Token leakage is limited to one handshake and one connection.
- String ids are portable across Rust, Go, TypeScript, and Lua.
- Error categories are stable before the gateway and plugin host are written.
- Events have explicit ordering, overflow, and restart semantics.
- Deadlines and cancellation share one transport mechanism.

Negative:

- `meta` is a JSON-RPC extension; generic clients that reject unknown members
  cannot be used without adaptation.
- A handshake and token table are required before the first request.
- Events are not durable; subscribers must handle re-subscription and gaps.
- Envelope and method validation are separate layers.

## Alternatives considered

### Token on every request

Rejected: increases logging/leak surface and repeats a secret on every frame.
Per-connection binding is sufficient for a per-session UDS.

### Signed token

Deferred: useful for distributed gateways, but requires key management and
rotation that v1 does not need.

### Separate event channel

Deferred: a second UDS or SSE-like channel adds ordering and authentication
problems. The framed connection already preserves order.

### Integer ids

Rejected: JavaScript consumers lose precision above `2^53`; string ids are
unambiguous.

### Method-specific envelope wrappers for deadlines

Rejected for the transport cap: `meta.deadline_ms` gives the gateway one
uniform enforcement point. Methods may still define a shorter semantic timeout.

## Required tests before Phase 1

- missing/wrong/replayed/cross-plugin token;
- `session.hello` required before any method;
- duplicate in-flight id;
- malformed envelope, unknown method, invalid params;
- notification without id receives no response;
- event order, gap/overflow behavior, and no replay after reconnect;
- cancel stops a blocked read and kills a child process tree;
- deadline returns `timeout`, not `cancelled`;
- raw token never appears in logs or error data;
- frame size and method size limits;
- `SO_PEERCRED` uid/pid checks.

## CI kernel note

Exec-sandbox tests depend on Landlock (Linux 5.13+). CI must detect and report
the kernel capability explicitly, run the full suite where supported, and run a
marked `openat2`-only degraded suite on older kernels. It must not hide the
difference behind a generic `#[ignore]`.

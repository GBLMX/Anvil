# AI Telemetry HUD Plan

Status: future P3 proposal. No implementation is part of the current polish
phase. The plugin API and the tactical interaction contracts are separate
gates; this plan only defines local, bounded, std-only telemetry.

Goal: expose the pace and risk of long MCP-driven editing sessions without
turning token throughput into a proxy for useful work.

## 1. Metrics

All metrics are local-only. No code, file contents, prompts, tokens, paths, or
network data leave the process.

| Metric | Definition | Source |
|---|---|---|
| EPM | Successful editor transactions per minute, with added/deleted line counts | `apply_transaction` success |
| Action APM | Completed MCP tool invocations per minute, by outcome | MCP/control router |
| Fumble Rate | Failed completed calls / all completed calls in the window | Router + executor policy |
| Burst Factor | Short-window edit rate relative to the trailing baseline | Metric buckets |
| Coverage | Milliseconds represented by the current window (0–60,000) | Metric window |

Rules:

- EPM counts **successful** transactions only. Rejected, cancelled, and
  partially applied transactions never count as edits.
- APM counts completed tool calls, not request bytes or token generation.
  In-flight calls are excluded until a terminal outcome exists.
- Fumble Rate excludes in-flight calls and reports `None` below a minimum
  sample count; it never divides by zero.
- Failed calls are classified once: policy/sandbox denial, timeout, parse
  error, user rejection, transport, or other.
- Burst Factor uses a short window (target 10 s) against a trailing 60 s
  baseline. It requires a minimum baseline and is not a raw line-count
  threshold.
- `Token/s` is not a gate and is not displayed by this HUD.

## 2. Measurement model

Do not use an unbounded `VecDeque` of every event. Use a fixed 600-bucket ring
at 100 ms resolution:

```text
600 buckets × 100 ms = 60 s
```

Each bucket is a fixed set of atomic counters. The hot path performs only
atomic increments; no allocation, no lock, no string formatting, and no
`unsafe`.

```text
MetricBucket
  tool_started
  tool_succeeded
  tool_failed[category]
  edits_applied
  lines_added
  lines_removed
```

Sampling:

- A sidecar task in `TaskGroup` wakes once per second, reads the 600 buckets,
  expires buckets older than 60 s, and publishes one `MetricsSnapshot`.
- The snapshot is coalesced; the TUI never redraws per operation.
- Bucket boundaries are based on a monotonic clock. Wall-clock jumps do not
  affect the window.
- `coverage_ms` is reported for the first 60 seconds after startup; early
  rates are labeled as partial-window estimates.
- 100 ms buckets bound boundary error to one bucket (0.17% of the 60 s
  window), below the 1% acceptance target.
- One snapshot per second is the only cross-thread UI update.

## 3. Ownership and integration

```text
anvil-session / anvil-editor
  apply_transaction success -> edit + line-count hooks

anvil-control / anvil-mcp
  completed tool call + source + outcome -> APM/failure hooks

anvil-task
  one metrics sidecar task -> one snapshot per second

anvil-tui
  snapshot -> status panel / sparkline

anvil-api / Lua
  optional metrics.changed subscription
```

- The MCP adapter tags requests with `source = "mcp"`; direct editor/plugin
  actions are not counted as AI APM.
- A request id is recorded once at completion. Retries are separate calls
  unless the transport explicitly reuses the same id.
- The metrics sidecar is observational only. It cannot delay editor input,
  cancel a tool call, or mutate document state.
- The TUI consumes the latest snapshot only; dropping an intermediate HUD
  update is allowed and must not affect counters.
- Metrics remain unavailable if the session is not MCP-backed; no synthetic
  zeros are displayed.

## 4. UI

### Base HUD

- A compact status-bar or right-edge panel: EPM, APM, Fumble Rate, and a
  60-second sparkline.
- Use the existing renderer/status surface. Prefer `ratatui`'s built-in
  Sparkline widget if it meets the gate; do not add a plotting dependency.
- Update at most once per second and only when the snapshot changes.
- No animation loop; the editor frame cadence remains input-driven.
- Sparkline values are bucketed rates, not raw event counts.

### Plugin link

- Optional `metrics.changed` subscription with the fixed snapshot schema.
- The Alert plugin may consume the snapshot for threshold alerts using the
  existing bounded `ui.alert`/`ui.bell` contract.
- The Graph plugin owns node highlighting; metrics must not mutate another
  plugin's overlay. Node-level bursts come from that plugin's graph deltas.
- Community-defined metrics are plugin-local and cannot add counters to the
  core hot path without a separate API review.

### Default and privacy

- Metrics are local-only; no external telemetry endpoint.
- HUD visibility is configurable. Counters may run when the HUD is hidden so a
  later toggle has history, but no content is captured.
- Reset clears counters and the visible window; no stale sparkline points.

## 5. Thresholds

Defaults are initial policy, not release claims:

| Condition | Default |
|---|---|
| Fumble alert | >= 30% over >= 10 completed calls |
| Burst warning | short-window edit rate >= 3x trailing baseline |
| Burst minimum | >= 10 edits and >= 10 s of baseline |
| Alert debounce | 30 s per category |
| HUD update | 1 Hz |

Thresholds are configurable. A threshold breach calls the Alert plugin/UI
once per debounce window; it must not create one alert per failed call.

## 6. Deliverable slices

### Slice 1: `MetricWindow`

- Add the fixed 600 × 100 ms atomic bucket ring.
- Implement edit, line-count, tool-outcome, and failure-category hooks.
- Tests: boundary placement, 60 s expiry, partial coverage, clock jumps,
  concurrent increments, and exact counts against a reference model.

### Slice 2: instrumentation

- Hook successful `apply_transaction` with added/deleted line counts.
- Tag MCP/control calls with source and record one completion outcome.
- Ensure rejected/cancelled/in-flight calls cannot enter APM/EPM.
- Tests: retry, timeout, denial, cancellation, parse failure, duplicate id.

### Slice 3: sidecar snapshot

- Add one `TaskGroup` metrics task with a 1 s tick.
- Publish a fixed `MetricsSnapshot` and a `metrics.changed` event.
- Bounded notification queue; dropped snapshots are harmless.
- Tests: no input blocking under synthetic event bursts, one snapshot/second,
  reset, no task leak on shutdown.

### Slice 4: HUD

- Add the status/right-edge HUD and sparkline.
- Update only when the snapshot changes; no per-event redraw.
- Tests: stable layout at 80x24, no flicker, hidden/reset state, partial
  coverage label.

### Slice 5: alerts

- Wire Fumble/Burst thresholds to the Alert plugin contract.
- Per-category debounce and one alert per window.
- Tests: storm does not alert repeatedly; threshold recovery re-arms.

### Slice 6 (optional): plugin metrics

- Expose the fixed `metrics.changed` schema to Lua.
- Plugin-local custom counters stay outside the core and cannot affect hot
  path cost.

## 7. Edge cases

- 10k line paste: one transaction, line counts included, not 10k events.
- Multi-file transaction: count once as a transaction and sum successful line
  changes; do not count per internal edit.
- Failed apply: zero EPM and no line delta.
- Timeout that later succeeds: classify one terminal outcome, not two.
- MCP client disconnect: completed calls retain outcomes; in-flight calls are
  failed once as transport/aborted.
- Binary/large/oversize refusal: failure category, never EPM.
- Idle > 60 s: EPM/APM/fumble return zero; coverage remains 60 s.
- Session restart: counters reset; no cross-session aggregation.
- Concurrent MCP clients: counters are shared per session; source/outcome
  remains per call.
- Clock change: monotonic buckets only; no negative deltas.

## 8. Performance and verification

- Hot path: no mutex, allocation, formatting, syscall, or `unsafe`.
- 60 fps input while a synthetic MCP workload emits high-frequency completions;
  HUD updates remain 1 Hz.
- Reference-machine benchmark: key-to-screen p99 must stay within the existing
  10% gate while metrics are enabled.
- Statistical test: snapshot counts match a reference ring/window model with
  less than 1% error over boundary and partial-window cases.
- `cargo test --workspace`, clippy, fmt, and Go tests/vet for MCP/control
  changes; no third-party metrics dependency.
- TUI smoke: sparkline updates without flicker; reset and hide/show work.

## 9. Exit criteria

- EPM/APM/Fumble/Burst have one documented definition and one terminal
  classification per operation.
- Hot-path instrumentation is lock-free, bounded, and allocation-free.
- Snapshot delivery is sidecar-only and cannot block editing.
- No cross-plugin overlay mutation; graph red nodes remain graph-owned.
- No external telemetry or content capture exists.
- Threshold alerts are debounced and do not fire on unchanged/repeated data.
- 60 s sparkline and HUD remain stable at 80x24.

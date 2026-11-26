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
| EPM | Successful source-tagged transactions per minute, with added/deleted line counts | `apply_transaction` success |
| Action APM | Completed MCP tool invocations per minute, by outcome | MCP/control router |
| Fumble Rate | Failed completed MCP actions / all completed MCP actions in the window | MCP router + action outcome |
| Burst Factor | Short-window edit rate relative to the trailing baseline | Source-tagged metric buckets |
| Coverage | Milliseconds represented by the current window (0–60,000) | Metric window |

Rules:

- Every hook carries `source = "mcp" | "editor" | "plugin"`. The actor is
  propagated explicitly from the request/command path; it is never inferred
  from “MCP is connected”.
- The AI HUD defaults to `source = "mcp"`. Human edits and plugin edits are
  stored separately and do not inflate AI EPM/APM/Fumble. An optional “all
  actors” view may show them side by side.
- EPM for the AI view counts successful MCP-sourced transactions only.
  Rejected, cancelled, and partially applied transactions never count.
- APM and Fumble are MCP-only by definition. Human/plugin failures do not
  enter the AI denominator.
- Fumble counts one terminal outcome per MCP action; an edit failure that is
  already an MCP action failure is not counted twice.
- Fumble reports `None` below the minimum sample count; it never divides by
  zero.
- Failed calls are classified once: policy/sandbox denial, timeout, parse
  error, user rejection, transport, or other.
- Burst Factor uses a short window (target 10 s) against a trailing 60 s
  baseline, filtered to the selected source. It requires a minimum baseline
  and is not a raw line-count threshold.
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
MetricSource = mcp | editor | plugin

MetricBucket
  tool_started[3]
  tool_succeeded[3]
  tool_failed[3][category]
  edits_applied[3]
  lines_added[3]
  lines_removed[3]
```

Sampling:

- A sidecar task in `TaskGroup` wakes once per second, reads the 600 buckets,
  expires buckets older than 60 s, and publishes one `MetricsSnapshot` with
  per-source counters and the selected HUD source.
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
  apply_transaction(source, success, line_delta) -> source-tagged edit hooks

anvil-control / anvil-mcp
  completed MCP action + source + outcome -> APM/fumble hooks

anvil-task
  one metrics sidecar task -> one snapshot per second

anvil-tui
  source-filtered snapshot -> status panel / sparkline

anvil-api / Lua
  optional metrics.changed subscription
```

- The command/control path supplies the source explicitly: UI edits are
  `editor`, MCP calls are `mcp`, plugin-host calls are `plugin`. No hook guesses
  the source from connectivity.
- `apply_transaction` and every failure/completion hook record that source.
  Human/plugin edits are retained for the optional all-actor view but excluded
  from the default AI HUD and its thresholds.
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
- The HUD source selector defaults to `mcp`; an optional all-actor view can
  include editor/plugin activity without feeding AI thresholds.
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
| Fumble alert | >= 30% over >= 10 completed MCP actions |
| Burst warning | short-window MCP-sourced edit rate >= 3x trailing baseline |
| Burst minimum | >= 10 edits and >= 10 s of baseline |
| Alert debounce | 30 s per category |
| HUD update | 1 Hz |

Thresholds are configurable. A threshold breach calls the Alert plugin/UI
once per debounce window; it must not create one alert per failed call.

## 6. Deliverable slices

### Slice 1: `MetricWindow`

- Add the fixed 600 × 100 ms atomic bucket ring with per-source counters.
- Implement edit, line-count, tool-outcome, and failure-category hooks.
- Tests: boundary placement, 60 s expiry, partial coverage, clock jumps,
  concurrent increments, exact counts against a reference model, and
  mixed editor/MCP/plugin events staying in separate source buckets.

### Slice 2: instrumentation

- Hook `apply_transaction(source, outcome, line_delta)`; direct editor calls
  pass `editor`, MCP calls pass `mcp`, plugin-host calls pass `plugin`.
- Tag MCP/control calls with source and record one completion outcome.
- Ensure rejected/cancelled/in-flight calls cannot enter APM/EPM, and a human
  edit/failure cannot enter the AI view.
- Tests: retry, timeout, denial, cancellation, parse failure, duplicate id,
  mixed human/MCP edits, mixed human/MCP failures.

### Slice 3: sidecar snapshot

- Add one `TaskGroup` metrics task with a 1 s tick.
- Publish a fixed `MetricsSnapshot` with per-source counters and the selected
  HUD source, plus a `metrics.changed` event.
- Bounded notification queue; dropped snapshots are harmless.
- Tests: no input blocking under synthetic event bursts, one snapshot/second,
  reset, no task leak on shutdown.

### Slice 4: HUD

- Add the status/right-edge HUD and sparkline, defaulting to `source = mcp`.
- Add an optional all-actor selector; it is display-only and does not change
  AI alert thresholds.
- Update only when the snapshot changes; no per-event redraw.
- Tests: stable layout at 80x24, no flicker, hidden/reset state, partial
  coverage label, switching between MCP and all actors.

### Slice 5: alerts

- Wire AI-only Fumble/Burst thresholds to the Alert plugin contract.
- Per-category debounce and one alert per window; editor/plugin failures are
  ignored by AI alerts.
- Tests: storm does not alert repeatedly; threshold recovery re-arms; a human
  failure does not trip the AI alert.

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
- A human edit increments only the `editor` bucket; it never changes AI EPM
  or the AI sparkline.
- A plugin-host edit increments only the `plugin` bucket; direct plugin
  failures never enter the MCP fumble denominator.
- A mixed human + MCP session keeps independent counters even when both write
  to the same file in the same second.
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
- Mixed-source test: interleaved editor, MCP, and plugin edits/failures keep
  independent counters; AI HUD and AI alerts ignore non-MCP events.
- `cargo test --workspace`, clippy, fmt, and Go tests/vet for MCP/control
  changes; no third-party metrics dependency.
- TUI smoke: sparkline updates without flicker; reset and hide/show work.

## 9. Exit criteria

- EPM/APM/Fumble/Burst have one documented definition and one terminal
  classification per operation.
- Every edit and completion hook is explicitly source-tagged; the default AI
  HUD and AI thresholds count only `mcp`, and human/plugin activity is kept in
  separate counters.
- Hot-path instrumentation is lock-free, bounded, and allocation-free.
- Snapshot delivery is sidecar-only and cannot block editing.
- No cross-plugin overlay mutation; graph red nodes remain graph-owned.
- No external telemetry or content capture exists.
- Threshold alerts are debounced and do not fire on unchanged/repeated data.
- 60 s sparkline and HUD remain stable at 80x24.

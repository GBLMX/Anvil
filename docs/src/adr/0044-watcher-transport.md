# ADR-0044: Keep the bounded polling watcher for 0.6.x

- Status: Superseded by [ADR-0046](0046-full-workspace-watch.md)
- Version: 1.1.0
- Date: 2026-11-16
- Related: [ADR-0018](0018-workspace-tree-watch.md)
- Superseded by: [ADR-0046](0046-full-workspace-watch.md)

## Context

The current watcher polls registered open-buffer paths through the gateway
every 100 ms and debounces metadata changes for 500 ms. Tree watching uses a
separate bounded directory watch set and the same burst debounce. The watch set
is capped at 256 directories, skips `.git` and `target`, and marks the tree
stale when watcher support is unavailable.

A reference-machine measurement on the i7-12700H workstation shows the
difference between the integrated path and the kernel primitive:

| Path | p50 |
|---|---:|
| Clean-buffer external reload, polling | 708 ms |
| Kernel inotify event delivery | 0.05–0.19 ms |

The polling number includes the 100 ms stat interval and the intentional 500 ms
stability debounce. It is not a synchronous stall, but it is noticeable after a
formatting run or `git checkout`. Inotify is three orders of magnitude faster
at the kernel boundary.

Inotify would also introduce cross-platform transports, per-directory watch
descriptor limits, recursive watch setup, event-coalescing rules, and move/delete
reconciliation. None of those are implemented today.

## Decision

1. Keep the bounded polling watcher as the integrated transport for `0.6.x`.
2. Keep these initial bounds for any future inotify comparison:
   - open-buffer watches: at most the configured buffer limit (1–99, default 9);
   - tree watches: at most 256 directories, `.git` and `target` excluded;
   - open-buffer poll interval: 100 ms; metadata debounce: 500 ms;
   - tree poll interval: 500 ms; tree debounce: 500 ms;
   - file-worker event queue: 1024 events.
3. Do not add an inotify dependency or a second watcher implementation without
   a new ADR and a reference-machine comparison.
4. Treat the table above as the decision input for any future inotify slice.
   The slice must preserve the bounded watch set, debounce, and stale-tree
   fallback, and may not replace polling on platforms without a native backend.

## Consequences

Positive:

- One transport works through the gateway on every supported platform.
- The bounded watch set and debounce behavior remain testable and predictable.
- The 708 ms worst-case reload is documented rather than hidden.

Negative:

- External edits after a checkout remain perceptible for roughly half a second.
- A future inotify implementation must solve watch limits and recursive setup
  before it can replace polling.

## Verification

- `python3 scripts/perf-gates.py --binary target/release/anvil` reports
  `watch_poll_ms` and `inotify_kernel_ms` under the gates in
  `docs/src/performance.md`.
- The clean-buffer reload path is covered by the watcher tests and the
  `reloaded` status assertion.

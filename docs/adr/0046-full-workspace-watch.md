# ADR-0046: Full Workspace Watch Coverage Through the Gateway

- Status: Proposed
- Version: 1.1.0
- Implementation: native gateway core and TUI event routing landed; watch
  settings, mode selection, and native coverage in `:watch-status` pending
- Date: 2026-11-16
- Related: [ADR-0002](0002-go-daemon.md),
  [ADR-0018](0018-workspace-tree-watch.md),
  [ADR-0023](0023-internal-capability-closure.md),
  [ADR-0045](0045-settings-patch-layers.md),
  [ADR-0044](0044-watcher-transport.md)
- Supersedes:
- Superseded by:

## Context

The integrated watcher is intentionally partial. Open buffers poll file
metadata every 100 ms, the visible tree polls directory signatures every
500 ms, and Git-enabled distributions poll `.git/HEAD` and `.git/index`. The
tree watch set is capped at 256 directories. An unexpanded directory, a search
index, or a workspace file that is not currently open is invisible to the
watcher until the user refreshes.

The current polling path costs roughly 708 ms at p50 for a clean-buffer
external reload, including the 100 ms stat interval and the 500 ms stability
debounce. Native inotify delivery on the same machine is in the
0.05–0.19 ms range at the kernel boundary. The latency gap is documented in
[ADR-0044](0044-watcher-transport.md), which requires a new ADR before adding a
second watcher implementation.

"Full coverage" must be defined precisely. It does not mean one watch per
file. It means:

- every directory under a trusted workspace root, subject to the documented
  excludes and budget;
- create, delete, rename/move, content, and metadata changes for entries in
  those directories;
- consumers receive bounded, coalesced events and can invalidate their own
  state without a full tree scan per event;
- when native coverage cannot be achieved, the watcher reports a degraded
  mode instead of silently claiming completeness.

The design must preserve these boundaries:

- the TUI never performs direct workspace filesystem observation; the Go
  gateway owns path confinement and read permissions
  ([ADR-0002](0002-go-daemon.md), [ADR-0018](0018-workspace-tree-watch.md));
- no permanent global daemon, Watchman requirement, or new runtime service is
  introduced;
- Linux is the supported native platform for this slice; other platforms keep
  polling;
- external settings hot reload remains out of scope; project-config events
  may mark the config dirty, but application still requires `:config-reload`
  ([ADR-0023](0023-internal-capability-closure.md));
- the watcher request and event surfaces remain bounded by the gateway's
  existing path and permission model.

The relevant platform behavior is well documented:

- inotify watches a directory and reports events for children, but does not
  recursively watch subdirectories; `IN_Q_OVERFLOW` means events were lost and
  a rescan is required.
- fsnotify's Go implementation is cross-platform but also non-recursive, has
  watch-count limits, requires parent-directory watches for atomic saves, and
  does not work reliably on NFS/SMB/FUSE.
- Parcel's watcher demonstrates the desirable shape: recursive subscriptions,
  ignore rules, throttled/coalesced events, and multiple native backends,
  including inotify on Linux.
- Zed's filesystem watcher keeps native and polling modes, and cools down
  native registration after watch-limit failures.

## Decision

### 1. Define coverage as directory coverage with explicit limits

A workspace root reaches `coverage = "full"` only after the initial directory
walk and watch registration complete, and only when all of the following hold:

1. Every directory under the trusted root, except the fixed default excludes
   and user-added excludes, has a native watch.
2. No native watch registration failed and no watch was skipped.
3. No kernel event overflow has occurred since the last successful rescan.
4. The root is on a filesystem for which the native backend is supported.

The initial walk is asynchronous. Until it completes, the root reports
`coverage = "starting"`; it must not report `full` from the request that starts
the walk. A completed walk reports `full` or `limited` through
`watch.status` and `sys.watch_status`.

The fixed default excludes are `.git`, `target`, `node_modules`, and `.cache`.
User settings may add excludes; project settings may add excludes but cannot
remove the fixed defaults. `.git` itself gets two special non-recursive watches
for `HEAD` and `index`; its object/ref subtrees are not watched.

If the budget or filesystem prevents full coverage after the walk, the root
enters `limited` or `poll` mode and reports:

```text
mode: native | poll | limited | off
coverage: starting | full | limited | off
watched_dirs
skipped_dirs
overflow_count
last_rescan
```

The editor must never display or internally assume `full` while any of the
conditions above is false. Visible open buffers in skipped directories remain
covered by the existing per-buffer polling fallback.

### 2. Implement Linux inotify in the Go gateway

Use the already-present `golang.org/x/sys/unix` dependency. Do not add
`fsnotify`, `notify`, Watchman, or a second watcher process for this slice.

The gateway owns:

- the inotify instance;
- the recursive directory walk;
- watch registration/removal;
- the watch-descriptor-to-path map;
- event coalescing;
- overflow recovery;
- notifications to the current connection.

The initial walk runs asynchronously after `sys.watch_start`; startup does not
block the editor on a large repository. The request returns a generation and
`coverage = "starting"`. A later `watch.status` notification reports `full` or
`limited` after registration completes.

Linux native watch is preferred. On unsupported or remote filesystems, and on
non-Linux platforms, use the existing polling watcher. A watch that registers
but later produces no events is still subject to overflow/status reporting; it
must not be silently treated as reliable on a known-remote filesystem.

### 3. Add a bounded watch RPC and event notification

Add these gateway methods:

```text
sys.watch_start
sys.watch_stop
sys.watch_status
```

`sys.watch_start` accepts a workspace-relative root, an additive exclude list,
a directory budget, and a mount-crossing flag:

```json
{
  "root": ".",
  "exclude": [".idea", "dist"],
  "max_dirs": 8192,
  "cross_mounts": false,
  "mode": "auto"
}
```

It returns the generation and a `starting` status. It must not wait for the
initial walk:

```json
{
  "generation": 9,
  "mode": "native",
  "coverage": "starting",
  "watched_dirs": 0,
  "skipped_dirs": 0,
  "overflow_count": 0
}
```

When the walk finishes, the gateway pushes `watch.status`:

```json
{
  "jsonrpc": "2.0",
  "method": "watch.status",
  "params": {
    "generation": 9,
    "root": ".",
    "mode": "native",
    "coverage": "full",
    "watched_dirs": 139,
    "skipped_dirs": 0,
    "overflow_count": 0,
    "reason": null
  }
}
```

`sys.watch_status` returns the same shape on demand. A later rescan or error
also emits `watch.status`; `watch.events` is used only for file/directory
changes and the overflow/rescan signal.

The gateway also pushes a bounded event notification:

```json
{
  "jsonrpc": "2.0",
  "method": "watch.events",
  "params": {
    "generation": 9,
    "root": ".",
    "events": [
      {"path": "src/main.rs", "kind": "modified"},
      {"path": "src/new", "kind": "created", "is_dir": true},
      {"path": "src/old", "kind": "removed"},
      {"path": "src/old", "kind": "renamed", "to": "src/new"}
    ],
    "overflow": false
  }
}
```

`watch.events` uses the existing server-notification write path. Events are
dropped/coalesced rather than growing without bound. A notification batch is
capped at 1024 events; the pending map is capped at 16,384 paths. Exceeding
either cap collapses the pending state into one `rescan` event.

The gateway coalesces for 100 ms before emitting a batch. A new directory event
is processed before the next batch so its watch is established before events
inside it are relied upon.

### 4. Watch directories, not individual files

Use directory watches with at least:

```text
IN_CREATE | IN_DELETE | IN_MODIFY | IN_ATTRIB | IN_CLOSE_WRITE
IN_MOVED_FROM | IN_MOVED_TO | IN_DELETE_SELF | IN_MOVE_SELF
IN_ONLYDIR | IN_DONT_FOLLOW | IN_EXCL_UNLINK
```

Rules:

- Watch the parent directory and filter by `Event.Name`; do not watch an
  individual file. Atomic save through `rename(2)` replaces the file's inode,
  so a file watch would be lost.
- Add a watch for a newly created directory, then scan that directory to catch
  entries created before watch establishment.
- On directory rename, use the inotify cookie when both halves are observed;
  otherwise emit a remove plus create and schedule a rescan.
- Remove watches for deleted or moved-out subtrees.
- Do not recursively follow directory symlinks. Symlinks that resolve outside
  the root are not watched and cannot inject events.
- Do not cross mount points unless `cross_mounts=true`.
- Ignore access/open events; they are not editor-relevant and create noise.

### 5. Recover by rescanning, never by reconstructing lost events

On `IN_Q_OVERFLOW`, `IN_IGNORED` for an unknown descriptor, a watch failure,
or a detected event gap:

1. discard the pending path set;
2. increment the generation;
3. mark the root `stale`;
4. stop/restart the affected watch set;
5. walk the root again within the same budget;
6. emit one `rescan` event with the new generation.

A full rescan is bounded by the same directory budget and exclude rules. If it
cannot restore full coverage, the root becomes `limited` and status reports the
reason. The editor keeps working with polling fallback.

`ENOSPC` or `EMFILE` while adding watches stops further native registration,
preserves established watches, enters `limited` mode, and starts a cooldown
before retrying. It must not retry in a tight loop.

### 6. Keep the existing polling watcher as fallback

Polling remains the transport for:

- non-Linux platforms;
- known remote/virtual filesystems where inotify is unreliable;
- roots that cannot meet the native watch budget;
- skipped directories;
- native watcher startup failure or cooldown.

The fallback uses the existing `FileWorker` open-buffer and visible-directory
loops. It is not a second implementation of tree state. Native events and
polling update the same bounded dirty sets and generation checks.

### 7. Route events to consumers with separate coalescing policies

The gateway emits one event stream; the TUI fans it out:

| Consumer | Policy |
|---|---|
| Open buffers | Match parent-directory events to open paths; debounce stats at 100 ms; clean reload or dirty prompt |
| File tree | Mark affected directories dirty; debounce a local snapshot at 150 ms; preserve expansion/selection |
| Workspace index | Invalidate affected file or directory; rebuild lazily on the next query |
| Project search | Mark results stale; rerun only on the next explicit search |
| Git | Keep special `.git/HEAD`/`.git/index` watches; coalesce status refresh at 300 ms |
| Project `.anvil.toml` | Mark config dirty; require explicit `:config-reload` |
| User settings | Not part of workspace watch; separate path watcher needed later |

Directory creation/rename triggers a local subtree snapshot, not a full-tree
snapshot. A `rescan` event is the only path that requests a full root snapshot.

### 8. Keep the trust and transport boundaries

- Watch roots are workspace-relative and confined through the existing
  `openat2`/`RESOLVE_BENEATH` path layer.
- Watching requires the root's existing read permission; no new arbitrary
  filesystem capability is added.
- Only trusted primary roots are watched. Secondary roots are watched by their
  own trusted gateway, with root-tagged events.
- Event paths are normalized and filtered before leaving the gateway; path
  length and batch size are bounded.
- The watcher stops and closes its fd when the connection closes.
- `AF_UNIX`/IPC notification behavior remains the existing transport; no
  watcher notification is accepted from plugins.

### 9. Make the mode configurable without changing the default guarantee

Add a watch settings group through the settings registry:

```toml
[watch]
mode = "auto"        # auto | native | poll | off
max_dirs = 8192
exclude = []
cross_mounts = false
```

`auto` prefers native and falls back to polling. `native` reports `limited`
rather than silently switching to polling. `poll` is explicit compatibility
mode. `off` disables workspace observation.

`editor.file_watching = false` maps to `mode = "off"` for compatibility.
`editor.file_watching = true` maps to `mode = "auto"` unless an explicit
`[watch] mode` is present.

`:watch-status` and the settings source display report the active mode,
coverage (including `starting` before the walk completes), skipped
directories, overflow count, last rescan, and fallback reason. Open buffers in
skipped directories continue to use per-buffer polling.

## Consequences

Positive:

- Unopened workspace files and directories become visible to tree/index/search
  consumers instead of requiring a manual refresh.
- A directory-based native watch avoids per-file watch explosion and survives
  atomic saves.
- Event bursts are coalesced; overflow results in a deterministic rescan
  rather than an unbounded queue.
- The TUI and plugin processes still never bypass the gateway boundary.
- Degraded coverage is explicit and actionable instead of a hidden failure.
- Linux support needs no new dependency; non-Linux behavior remains stable.

Negative:

- The gateway gains a large state machine: recursive registration, rename
  reconciliation, budget enforcement, and overflow recovery.
- Native coverage is filesystem- and kernel-limit-dependent; `full` cannot be
  promised for every workspace.
- Large repositories still require excludes and a budget, and a rescan can be
  expensive.
- Startup watch establishment is asynchronous and reports `starting` until the
  walk completes; it reports `limited` only after a skip or registration
  failure.
- A second event path (native plus polling fallback) requires generation and
  deduplication discipline.
- User configuration outside the workspace remains unwatched until a separate
  path-watcher design.

## Verification

- Focused gateway tests:
  - `sys.watch_start` returns `coverage = "starting"` and never reports `full`
    before registration completes; the completion `watch.status` and
    `sys.watch_status` report the final coverage and directory count;
  - directory watches receive create, delete, rename, modify, and close-write
    events for children;
  - a new directory is watched before its nested events are relied upon;
  - rename cookies update paths; unmatched moves produce remove/create plus a
    rescan;
  - symlink escapes and mount crossings are rejected;
  - exclude matching is relative, bounded, and additive;
  - watch budget, `ENOSPC`, `EMFILE`, and unknown descriptor handling enter the
    documented limited/rescan states;
  - connection close releases all watches and the inotify fd.
- Overflow and race tests:
  - simulate `IN_Q_OVERFLOW` and verify one generation-scoped rescan;
  - burst events beyond the pending/batch cap and verify coalescing, not
    unbounded memory;
  - atomic save via temp-file rename reloads the open buffer;
  - directory create/delete during an in-flight scan converges to a consistent
    status.
- TUI integration tests:
  - tree refresh follows a native directory event;
  - open clean buffer reloads after an external change;
  - dirty buffer prompts rather than reloading;
  - workspace index invalidates and lazily rebuilds;
  - Git status refresh is coalesced across a checkout-like burst;
  - polling fallback remains functional when native start fails.
- Stress/performance:
  - 10,000-directory fixture with a configurable budget;
  - 100,000-event burst;
  - `git checkout` and package-install-like churn;
  - event-to-notification p99, startup registration time, peak RSS, skipped
    directory count, and tree refresh latency.
- Real surface smoke:
  - run the installed release with `:watch-status`;
  - create, rename, delete, and modify files in collapsed directories;
  - run `git checkout` across branches;
  - verify coverage changes on a remote filesystem/fallback fixture.

## Alternatives considered

### Keep bounded polling only

Rejected for full coverage. Polling every directory scales as
`directories × poll_frequency`, and the current 100/500 ms policy still gives a
perceptible delay. It remains the correct fallback, not the primary transport.

### Add fsnotify to the TUI

Rejected. It would move workspace filesystem observation out of the gateway,
violate path confinement, and still require recursive registration and watch
limit handling. fsnotify also does not recursively watch subdirectories.

### Require Watchman

Rejected as a default. It adds an external daemon, deployment requirements, and
a second trust/permission surface. It may remain an optional future backend if
measured workloads justify it.

### Watch every file individually

Rejected. It multiplies watch descriptor usage, breaks on atomic `rename`
saves, and duplicates the same directory event for every file.

### Use `fanotify` for mount-wide coverage

Rejected for this slice. It requires stronger privileges, has different
filesystem constraints, and is not necessary for workspace-confined coverage.

### Fail closed when full coverage is unavailable

Rejected. Disabling all watching because a large directory cannot be watched
would make normal editing worse. Use explicit `limited` mode, keep polling
fallback for open/visible paths, and surface the exact loss of coverage.

### Auto-reload project settings from watch events

Rejected for this slice. Config events may mark the project file dirty, but
automatic application remains excluded by ADR-0023.

## References

- inotify(7), Linux man-pages:
  https://man7.org/linux/man-pages/man7/inotify.7.html — directory watches,
  event masks, rename cookies, and `IN_Q_OVERFLOW` recovery semantics.
- fsnotify README:
  https://github.com/fsnotify/fsnotify — non-recursive watches, Linux
  `max_user_watches` behavior, atomic-save file-watch caveat, and unsupported
  network filesystems.
- Parcel watcher README:
  https://github.com/parcel-bundler/watcher — recursive subscriptions, ignore
  rules, event throttling/coalescing, scalable watch behavior, and native
  backend choices.
- Zed filesystem watcher:
  https://github.com/zed-industries/zed/blob/b0911ccc9e8bb2b9eab4cc53ec2c98b0471e2759/crates/fs/src/fs_watcher.rs
  — native/poll watcher modes, registration limits, cooldown, and path
  registration handling.
- Go `x/sys/unix` package:
  https://pkg.go.dev/golang.org/x/sys/unix — the existing dependency used for
  the Linux inotify syscalls.

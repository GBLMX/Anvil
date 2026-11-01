# ADR-0018: Workspace Tree Watching

- Status: Accepted
- Date: 2026-11-01
- Related: [ADR-0002](0002-go-daemon.md), [ADR-0011](0011-workspace-chunked-file-io.md)

## Context

The open-buffer watcher polls regular-file metadata through the gateway and
reloads or prompts for external edits. The workspace tree had no equivalent
watcher, so external create, delete, rename, or checkout left it stale until a
manual refresh.

Tree watching must not use direct TUI filesystem access, must not add a
permanent inotify/instrumentation daemon, and must stay bounded on large
repositories. The gateway already owns path confinement and `fs.read`
permission checks.

## Decision

### 1. Poll directory signatures through the gateway

Add `sys.read_dir_signature` with a workspace-relative `path`. The gateway:

- resolves the path through the existing `openat2`/`RESOLVE_BENEATH` boundary;
- requires `fs.read` permission;
- requires the target to be a directory;
- returns the directory `mtime` and `ctime` as `modified_ns` and `changed_ns`.

Directory entry creation, removal, and rename update these timestamps. File
contents do not, so editor saves do not trigger a tree refresh.

The TUI never calls `std::fs` or inotify for workspace observation. The existing
direct-filesystem tree path remains only as the no-gateway fallback and is
marked stale.

### 2. Watch only visible tree directories

`FileWorker` owns the watched-directory set and polls it every 500 ms. The set
contains the workspace root plus directories currently expanded in the tree.
Expand adds a path; collapse or deletion removes it. `MAX_TREE_WATCH_DIRS` caps
the set at 256 and the tree title reports a limited watch set.

Signatures are debounced for 500 ms after the last change. All ready
directories produce one `TreeChanged { paths }` event, not one event per path.

### 3. Refresh is generation-scoped and state-preserving

A refresh requests one snapshot containing the root listing and every expanded
directory listing. The TUI:

- permits one snapshot in flight; events during it queue one follow-up;
- discards responses whose generation is stale;
- rebuilds the flattened tree from the snapshot;
- preserves expanded paths and the selected path when it still exists;
- falls back to the nearest surviving parent, then the nearest index;
- leaves tree visibility and focus unchanged for background refreshes;
- rewrites the watch set after applying the new tree.

If the gateway watcher or a listing is unavailable, the old tree is retained and
the title becomes `Files · stale`; buffers and file operations remain usable.

### 4. Rename is observed as an entry change

Polling identifies the affected parent directory, not the old/new name pair.
If the selected entry was renamed by an external actor, the original path no
longer exists and selection falls back to its nearest surviving parent. The
refreshed tree still shows the new entry.

## Consequences

- No new dependency or background daemon; the Go gateway remains the only
  filesystem policy boundary.
- Tree refresh latency is bounded by the 500 ms poll plus 500 ms debounce.
- Directory timestamps are required to move on entry changes. This holds for
  ext4, tmpfs, XFS, and btrfs; network filesystems with coarse or disabled
  timestamps may appear stale and must surface the stale state.
- A high-churn directory can postpone its quiet window indefinitely; the tree
  remains bounded and the UI stays responsive instead of refreshing per event.
- A full expanded-tree snapshot is sent per refresh. The watch cap bounds this
  work; affected-subtree-only snapshots are a future optimization.

# ADR-0047: File tree navigation and incremental refresh

- Status: Accepted; implemented 2026-11-16
- Version: 1.2.0
- Implementation: natural ordering, generation-scoped listings, incremental
  refresh, ancestor-expanding reveal, scrolloff/wheel viewport, and tree find
  landed; multi-selection, nesting, drag-and-drop, and undo remain deferred
- Date: 2026-11-16
- Related: [ADR-0018](0018-workspace-tree-watch.md),
  [ADR-0044](0044-watcher-transport.md),
  [ADR-0045](0045-settings-patch-layers.md),
  [ADR-0046](0046-full-workspace-watch.md)

## Context

The workspace tree is functional: it supports create, rename, copy, move,
recursive delete, multi-root roots, untrusted-root rejection, Git decorations,
diagnostic rollups, unsaved markers, and a bounded polling watcher. The current
256-directory tree-open gate is 105 ms p50 on the reference machine.

The remaining problems are navigation and refresh semantics, not missing basic
file operations.

### Ordering

The gateway already filters `.git`, `target`, and the atomic-write temporary
prefix in `go/internal/fsroot/fsroot.go`. It also sorts each directory
directories-first and then by raw name:

```go
sort.Slice(result, func(i, j int) bool {
    if result[i].IsDir != result[j].IsDir {
        return result[i].IsDir
    }
    return result[i].Name < result[j].Name
})
```

This is deterministic, but it is case-sensitive lexicographic ordering. The TUI
does not apply a second comparator in `handle_tree_listing` or
`build_tree_entries`. `file2` sorts after `file10`, and uppercase names sort
before lowercase names.

The no-gateway fallback `tree_entries` has its own case-sensitive lexicographic
sort and filters only `.git` and `target`, not the atomic-write prefix. That
fallback must not become a second ordering or visibility policy.

The gateway order is also used by command-line path completion. Natural
ordering is a tree-browsing policy, not a transport invariant.

### Visibility

The gateway filtering is authoritative: the TUI cannot reveal `.git` or
`target` merely by changing `TreeState::entry_visible`. Adding a show-ignored
toggle would require a gateway request option and a deliberate policy change.

The TUI currently hides dot directories except `.github` and `.cargo`, while
dotfiles remain visible. `H` toggles hidden directories. The fuzzy picker uses
a separate `rg --files` hidden flag. This ADR does not redefine those policies.

### Reveal

`reveal_breadcrumb_path` selects a path only if it is already present in the
flattened tree. Otherwise it records `pending_select`; the next refresh selects
the nearest existing ancestor. It does not expand collapsed ancestors. A deep
file under a collapsed directory therefore cannot be revealed reliably.

### Viewport and mouse

`render_tree` derives the first visible row as:

```text
selected - visible_height + 1
```

The selected row stays pinned to the bottom with no scrolloff. Mouse wheel
events always drive the editor; hovering the tree does not scroll it. A single
click selects a row but cannot expand, collapse, or open it.

### Refresh

`FileEvent::TreeChanged { paths }` carries the changed watched directories, but
the TUI discards the paths and calls `refresh_tree_background()`. That sends a
`TreeSnapshot` for the root and every expanded directory, even when one
directory changed. `TreeChanged` is emitted from a bounded 256-directory watch
set every 500 ms, so a full listing can be repeated frequently during a
checkout or formatter run.

### Search

There is no in-tree find. `Space f` is a separate workspace file picker with
fuzzy scoring and preview. It is not a substitute for locating a row in the
currently loaded tree.

External implementations treat these as separate layers:

- VS Code's Explorer supports tree-local find/filter with highlight and fuzzy
  modes, multi-selection, file nesting, and file operations.
- Zed's project panel has auto-reveal, sticky scroll, auto-folded single-child
  directories, multi-selection, sorting modes, and file-operation undo.
- Helix explicitly recommends pickers for fast file navigation while retaining
  an optional file explorer with separate ignore settings.

## Decision

### 1. Keep the flat tree projection

Keep `TreeState.entries` as the ordered list of visible tree rows. Do not
introduce a node arena, synthetic DOM, second tree model, or asynchronous tree
framework in this decision.

New pure helpers may be added around it:

```text
tree_order       // deterministic row comparison
tree_viewport    // scroll offset, scrolloff, hit-testing
tree_reveal      // pending reveal + ancestor expansion
tree_find        // loaded-row matching and navigation
```

The flat projection already supports sorting, reveal, viewport movement,
incremental directory replacement, and highlight-based find. Revisit the data
model only when multi-selection, nested-file synthetic rows, or measured
virtualization requires it.

### 2. Apply case-insensitive natural sort in the TUI

Sort directory listings in the TUI before they become tree rows:

1. directories before files;
2. case-insensitive natural order for names;
3. exact name as the final tie-break;
4. stable ordering for duplicate names (which should not occur in one
   directory).

Natural order treats runs of ASCII digits numerically, so `file2` precedes
`file10`. Case-insensitive comparison must not depend on the current locale.
Non-ASCII names fall back to lowercase comparison and then exact-name
comparison.

Sort each directory independently. Do not reorder the synthetic secondary-root
nodes at depth 0; those remain a separate root group. Untrusted-root labels and
root ordering are unchanged.

All directory-listing consumers must route through one `tree_order` helper:

- `handle_tree_listing`;
- `TreeSnapshot` / `build_tree_entries`;
- `TreeListings`;
- the no-gateway `tree_entries` fallback.

No caller may sort locally. The fallback must also use the gateway's
`.git`/`target`/`.anvil-write-` filtering so worker-unavailable behavior does
not expose temporary files or change ordering.

Keep the gateway's lexicographic directories-first order. Do not move natural
sorting into `sys.read_dir`, because completion and other non-tree consumers
share that result.

This decision fixes the default behavior without adding a sort setting. A
sort-mode setting is a separate settings-registry change if users ask for it.

### 3. Make reveal expand ancestors

Add an explicit reveal path operation, exposed as `:tree-reveal` and through
breadcrumb path activation:

1. Resolve the target through its owning root.
2. Expand each ancestor directory from the root to the target.
3. Wait for each listing to be applied.
4. Select and scroll to the target.
5. If the target is filtered, ignored, missing, or outside a trusted root,
   report a concrete status instead of silently selecting an ancestor.

`pending_reveal` holds at most one target. A new reveal supersedes the old one.
Root removal, tree close, or a new full refresh cancels it.

Bound the operation to `MAX_TREE_REVEAL_STEPS = 128` ancestor transitions. A
deeper path fails with a bounded status error rather than looping.

When the tree is already visible, opening a file reveals it. Do not open the
tree automatically for this purpose. There is no auto-reveal setting in this
decision.

### 4. Make the viewport explicit

Add a tree viewport state and use the same mapping for rendering, selection, and
mouse hit-testing.

- `TREE_SCROLLOFF_ROWS = 3`.
- Selection movement keeps the selected row visible with the scrolloff margin.
- Mouse wheel over the tree moves the tree selection by `TREE_WHEEL_ROWS = 3`.
- Mouse wheel outside the tree keeps its current editor behavior.
- A left click selects a row. Expansion, collapse, and file opening remain
  keyboard actions (`h`, `l`, `Enter`) in this decision.

Do not add double-click detection in this slice; crossterm does not provide a
reliable click count and adding timing state is separate interaction work.

Do not change whether opening a file hides the tree. That is a separate focus
and layout decision.

### 5. Refresh changed directories instead of the whole tree

Use the `TreeChanged.paths` information already emitted by the watcher.

Add a bounded batch request and event:

```text
FileRequest::TreeListings {
    generation: u64,
    paths: Vec<PathBuf>,
}

FileEvent::TreeListings {
    generation: u64,
    listings: Vec<(PathBuf, Result<Vec<DirEntry>, String>)>,
}
```

The worker reads each path through its owning gateway and returns one event.
The TUI applies the event only when `generation` matches the current tree
generation. Stale batches are discarded.

For each returned listing:

- if the path is still an expanded directory, replace only its children;
- preserve expansion, selection, pending reveal, and scroll position;
- if the listing fails, remove the stale node and schedule one refresh of its
  nearest loaded ancestor;
- ignore paths that are no longer expanded or no longer belong to the root.

`TreeSnapshot` remains for explicit `:tree-refresh` and full recovery.
`TreeChanged` must not call `refresh_tree_background()`.

Bound the batch to `MAX_TREE_REFRESH_DIRS = MAX_TREE_WATCH_DIRS` (256). If more
paths are reported, take one full snapshot for that root instead of issuing an
unbounded batch.

Also stop dropping the paths in `FileEvent::TreeChanged`. An event with no
usable paths refreshes only the affected root, not every root.

Tree listing requests must be generation-scoped. Add a generation to `List` and
`Listed` as a mechanical part of this decision. `TreeListings` is reserved for
the bounded changed-directory batch. A stale listing must never splice children
into a newer tree.

### 6. Add tree-local find as highlight and jump, not a second fuzzy picker

Add a loaded-row find mode for the tree:

- activated by `/` or `Ctrl+f` while the tree is focused;
- searches the currently loaded visible-row projection, not collapsed
  directories and not the filesystem;
- uses the existing `search.case_sensitive` setting;
- highlights matching rows and reports the match count;
- `n` and `N` move to the next and previous match;
- `Esc` cancels find before it closes the tree.

Bound find state:

- `MAX_TREE_FIND_QUERY = 256` bytes;
- `MAX_TREE_FIND_MATCHES = 1024`.

If more than 1024 rows match, show `1024+` and keep navigation bounded. Do not
silently drop the count.

This is not a filter: it does not hide non-matching rows and does not expand
directories. `Space f` remains the workspace-wide fuzzy file picker. A
filter/highlight toggle and scoped project search are separate decisions.

Recompute matches after every tree replacement; never retain raw row indices
across a refresh.

### 7. Keep `.git` and `target` filtering in the gateway

Do not add a client-only show-ignored or show-hidden toggle.

A future show-ignored control must add an explicit gateway option, preserve the
existing trust boundary, and define whether `.git` internals, `target`,
`node_modules`, and ignored files are included. It must not imply that changing
`TreeState::entry_visible` can reveal entries the gateway never sends.

This ADR does not change dotfile or dot-directory visibility. The current
policy remains: dotfiles are visible; dot directories are hidden unless `H`;
`.github` and `.cargo` remain visible exceptions.

## Bounds and invariants

- Tree rows remain workspace-relative or absolute paths already resolved by the
  owning root; no new arbitrary path input is introduced.
- Only trusted roots are expandable. Untrusted roots remain labeled and cannot
  be revealed into.
- Multi-root sorting happens within each root; root groups do not interleave.
- Refresh generations are per tree/root and are checked before applying any
  listing.
- A failed listing must not delete a node outside the affected directory.
- Find operates only on loaded rows; it must not trigger filesystem walks.
- A tree find query is not persisted to settings or session state.
- Existing create, rename, copy, move, delete, Git rollup, diagnostic rollup,
  and watch-limit behavior must not regress.

## Consequences

Positive:

- Tree order becomes predictable and human-friendly across filesystems and
  platforms.
- Deep files can be revealed without manually expanding every ancestor.
- The tree stops rebuilding every expanded directory for one watcher change.
- Selection and mouse behavior become consistent with the rendered viewport.
- Tree-local find locates rows without introducing a second fuzzy picker.

Negative:

- The TUI gains viewport, reveal, find, and per-root refresh state.
- Incremental replacement must handle stale generations and missing
  directories carefully.
- Tree find does not search collapsed or ignored directories; `Space f` remains
  the complete workspace search path.
- `.git` and `target` remain invisible until a separate gateway visibility
  decision changes the filter.

## Alternatives considered

### Sort in the gateway

Rejected for this decision. Natural order is a tree-browsing policy and the
gateway result is shared with command-line completion. A gateway change would
also require deciding whether completion should use the same order.

### Rewrite the tree as a node arena

Rejected. The flat projection is sufficient for sorting, reveal, viewport,
find, and incremental replacement. A rewrite adds state transitions and
selection complexity without a measured problem.

### Use `TreeSnapshot` for every watcher change

Rejected. It rereads the root and every expanded directory for a one-directory
change and defeats the purpose of carrying `TreeChanged.paths`.

### Add a full in-tree filter now

Rejected for this slice. It duplicates the existing fuzzy picker and requires a
projection that hides rows while preserving hierarchy. Start with highlight and
jump; add filtering only after dogfood evidence.

### Add multi-selection, file nesting, drag-and-drop, or undo now

Deferred. Each changes the tree contract:

- multi-selection needs batch operations and failure/rollback semantics;
- file nesting creates synthetic rows and changes path ownership;
- drag-and-drop needs a drop-target contract and root-boundary rules;
- undo/trash needs a durable operation history and deletion policy.

## Verification

Focused TUI tests:

- directories sort before files;
- `file2` sorts before `file10`;
- case-insensitive ordering is stable;
- secondary-root nodes do not move into primary root ordering;
- stale `TreeListings` and `Listed` generations are ignored;
- reveal expands multiple collapsed ancestors and selects the target;
- reveal of an untrusted, missing, or filtered path reports an error;
- scrolloff keeps the selected row inside the viewport;
- the no-gateway `tree_entries` fallback returns the same natural order as the
  gateway path;
- the fallback hides `.git`, `target`, and `.anvil-write-*` entries;
- wheel-over-tree changes tree selection, not editor scroll;
- tree find respects `search.case_sensitive`, caps at 1024 matches, and
  recomputes after refresh;
- a failed changed-directory listing removes only the affected subtree.

Integration tests:

- one changed directory dispatches one bounded `TreeListings` request and does
  not call `TreeSnapshot`;
- more than 256 changed paths falls back to one full snapshot;
- a second-root change refreshes only the second-root worker;
- create, rename, copy, move, delete, Git status, and diagnostics still work;
- watch-limit and stale-tree status remain visible;
- no path is revealed across an untrusted root.

Performance and real-surface checks:

- 256-directory tree open remains within the existing p50 <= 1 s gate;
- 100k-file workspace search and open paths do not regress;
- `scripts/perf-gates.py` passes on the reference machine;
- run the installed release with `:tree-reveal`, `/`, `n`/`N`, a collapsed
  deep path, a checkout burst, and a second root; inspect the tree title and
  status for stale/limited state.

## References

- VS Code Explorer documentation:
  https://code.visualstudio.com/docs/src/editing/getting-started/userinterface.md#explorer-view
  - Explorer file operations, advanced tree find/filter modes, multi-selection,
    file nesting, and the existing separate quick-open path. Informs the
    non-goals and the decision to keep fuzzy search separate from tree find.
- Zed Project Panel:
  https://zed.dev/docs/src/project-panel.md
  - Auto-reveal, sticky scroll, auto-folded directories, multi-selection,
    sorting modes, file operations, and undo/redo. Informs reveal and the
  deferred file-management scope.
- Helix Pickers:
  https://docs.helix-editor.com/master/pickers.html
  - The case for pickers as the primary fast file-navigation surface. Informs
    keeping `Space f` rather than duplicating fuzzy search in the tree.
- Helix file-explorer settings:
  https://docs.helix-editor.com/master/editor.html#editorfile-explorer-section
  - Explorer-specific ignore and flatten-directory options. Informs treating
    visibility as an explicit policy instead of a client-side toggle.

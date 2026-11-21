# ADR-0064: Multi-root Routing and Ownership

- Status: Accepted; routed file, tree, search, and Git surfaces implemented
- Version: 1.0.0
- Date: 2026-11-20
- Related: [ADR-0018](0018-workspace-tree-watch.md),
  [ADR-0038](0038-project-settings-trust.md),
  [ADR-0046](0046-full-workspace-watch.md),
  [ADR-0048](0048-per-root-language-tooling-and-lsp-workspace.md),
  [ADR-0061](0061-workspace-navigation-and-root-boundary.md)
- Supersedes:
- Superseded by:

## Context

A session can have one primary workspace root and additional trusted roots.
Each trusted root has its own gateway and file worker. Operations must not
silently fall back to the primary gateway when a path belongs to another root;
doing so would cross a trust, path, and process boundary.

LSP has its own per-root decision in ADR-0048. This ADR covers file, tree,
quick-open, search, mutation, and Git ownership.

## Decision

### 1. `WorkspaceRoots` owns path ownership

The deepest matching root owns a path. `WorkspaceRoots` supplies the canonical
owning root; callers do not compare string prefixes or maintain a parallel root
map.

### 2. Per-root gateway and worker ownership

Every trusted root with a live gateway has its own file worker and watch set.
An untrusted root has no gateway, no worker, and no watch. Operations on an
untrusted root fail with an explicit trust error.

Root removal/untrust drops that root's worker, watches, cached state, and
in-flight work. Late events from a dropped root are ignored.

### 3. Route by owning root, never by primary root

File reads/writes, tree listing/expansion, quick-open enumeration, preview,
project search, mutations, and file-scoped Git operations resolve the owning
root and use that root's gateway. There is no primary-root fallback for a
path owned by another root.

Path-bearing events crossing worker boundaries are absolute. Results are
tagged with their root and a shared generation so stale responses can be
dropped.

### 4. Global budgets and merged views

Fan-out operations merge results with deduplication and global caps. Project
search cancellation cancels every participating root, not only the primary
worker. The tree splits its watch budget across roots without exceeding the
per-worker cap.

### 5. Reject cross-root mutation

Rename, move, copy, and equivalent mutations must stay within one root.
Cross-root operations are rejected before mutation; they are not implemented as
two independent gateway writes with no transaction.

### 6. LSP follows ADR-0048

LSP workers are root-scoped and request routing uses the file's owning root.
This ADR does not define a shared LSP server across roots. The historical
primary-root LSP clause in the multi-root plan is superseded by ADR-0048.

## Consequences

Positive:

- Trust and gateway boundaries match path ownership.
- A secondary root cannot accidentally read or write through the primary
  gateway.
- Root removal and cancellation have deterministic cleanup.

Negative:

- The model maintains maps of gateway sessions, file workers, and root-scoped
  state rather than one worker.
- Fan-out operations need global generation, deduplication, and cap handling.
- Cross-root moves/copies remain unavailable by design.

## Verification

- Deepest-root ownership tests.
- Two-root worker and gateway tests for list, preview, read/write, search,
  mutations, and Git.
- Untrusted roots fail without a fallback.
- Root removal cancels pending search and drops late events.
- Cross-root rename/copy is rejected before mutation.
- Per-root LSP behavior is covered by ADR-0048 tests.

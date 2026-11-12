# Multi-Root File Tree and Search Routing Plan

Status: phase 1 plus quick-open and project-search fan-out landed. Trust, root
registration, per-root gateway lifecycle, open/save routing, trusted-root
quick-open enumeration/preview, and project-search merge/cancel are
implemented. File tree and mutation routing still assume one primary workspace.

## 1. Scope

Make every workspace file operation route through the owning
`WorkspaceRoots` root:

- file tree list/expand/watch
- quick-open file enumeration and preview
- project search and cancellation
- create/rename/remove/copy
- Git gutter/diff/blame/conflict operations

Out of scope for this plan:

- LSP multi-root servers. Current session still uses the primary root for LSP.
- Cross-root rename/copy. These are rejected, not copied between gateways.
- Remote workspaces over the network.

## 2. Current State

- `WorkspaceRoots` owns canonical root paths, trust flags, and deepest-owner
  lookup.
- `Model.root_gateways` owns `(GatewaySession, GatewayClient)` per trusted root.
- `read_file` / `write_file` already select the owning root gateway.
- `Model.file_worker: Option<FileWorker>` is bound to one gateway and one
  workspace.
- `FileWorker` requests use workspace-relative paths.
- `FileEvent::Files` is generation-scoped; root workers are polled and merged
  into quick-open as absolute paths.
- `FileEvent::ProjectSearch` is merged under a shared generation; secondary
  paths are converted to absolute paths before insertion.
- `Model.tree` is a flat list of primary-root children.
- `FuzzyState.entries` mixes primary-relative and secondary-root absolute
  paths. Project-search results are stored separately with a global cap.

## 3. Decision: worker per trusted root

Use one existing `FileWorker` per trusted root instead of making
`FileWorker` multi-root internally.

Why:

- Existing worker behavior, watches, and tests stay single-root.
- Each worker already owns one authenticated connection and one watch set.
- Root removal/trust changes can drop one worker without rebuilding all state.
- Primary root behavior remains unchanged when only one root exists.

Cost:

- Model owns `BTreeMap<PathBuf, FileWorker>`.
- Every worker event must be tagged with its root.
- Fan-out operations must merge results and enforce global caps.

## 4. Data Model

```rust
struct Model {
+    file_workers: BTreeMap<PathBuf, FileWorker>,
-    file_worker: Option<FileWorker>,
}
```

Worker creation:

```rust
fn ensure_file_workers(&mut self) -> bool
fn ensure_file_worker_for_root(&mut self, root: &Path) -> bool
fn drop_file_worker_for_root(&mut self, root: &Path)
```

Rules:

- Primary root uses `gateway_session.connect_lsp()` and `self.gateway`.
- Additional roots use their `root_gateways` session's `connect_lsp()`.
- Only trusted roots with a live gateway get a worker.
- Untrusted roots have no worker and no watch; the UI shows a trust action.
- A root removal/untrust drops its worker and all root-scoped state.

## 5. Path and Event Contract

All worker paths crossing the Model boundary become absolute.

```rust
enum FileEvent {
    Listed(PathBuf, Result<Vec<DirEntry>, String>),
    Files(Result<Vec<PathBuf>, String>),
...
}
```

For `Files`, the worker returns absolute paths by joining its root.
For `ProjectSearch`, `project_search` must return absolute paths; the worker
prefixes its root before emitting.

Model polling:

```rust
fn poll_file_workers(&mut self) {
    for (root, worker) in &self.file_workers {
        for event in worker.poll() {
            self.handle_file_event(root, event);
        }
    }
}
```

`handle_file_event` remains root-aware only where paths/relative conversions
matter.

## 6. File Tree

Current model:

```text
root children at depth 0
```

New model:

```text
depth 0: root node for each trusted/untrusted root
depth 1: children of each root
depth N: descendants
```

Changes:

- Add `TreeEntry.root: Option<PathBuf>` or derive the owning root from
  `WorkspaceRoots`.
- Root nodes are always visible; untrusted roots render disabled and require
  `:workspace-trust`.
- `List(path)` routes by `owning_root`.
- `WatchDirs` sends the expanded set for each root to that root's worker.
- Split `MAX_TREE_WATCH_DIRS` across roots; never exceed the per-worker cap.
- `TreeSnapshot` is requested per root; merge snapshots by root.

Acceptance:

- two roots render as two top-level nodes
- expanding a second-root directory lists through the second gateway
- untrusted root cannot expand until trusted
- removing a root removes its node and worker

## 7. Quick Open

Changes:

- `FuzzyState.entries` becomes `Vec<PathBuf>` absolute.
- `Files` fans out to every trusted root worker with a shared generation.
- Merge results, deduplicate absolute paths, sort, apply
  `MAX_QUERY_TEXT`/result caps.
- Label display:
  - primary root: existing relative label
  - other roots: `<root-name>/<relative>`
- Preview routes by `owning_root`.
- Enter opens the absolute path through the normal open path, which already
  routes reads by root.

Acceptance:

- fuzzy list contains files from both roots
- duplicate basenames remain distinguishable
- preview comes from the correct root
- opening a second-root file saves through its root

## 8. Project Search

Status: landed. Search fans out to every trusted root, waits for every worker,
merges/deduplicates absolute locations under the global cap, and ignores stale
generations. Opening a secondary-root location resolves through
`WorkspaceRoots` and the owning gateway.

Changes:

- `start_project_search` sends `ProjectSearch { query, generation }` to each
  trusted root worker.
- Track `pending_roots: BTreeSet<PathBuf>` for the generation.
- Merge `PipeLocation` results; convert to absolute paths.
- Global result cap `MAX_PROJECT_RESULTS`; per-root output cap must leave room
  for all roots.
- Cancel bumps generation and ignores late results from every worker.
- Deduplicate `(path, line, column, message)`.

Acceptance:

- one query returns matches from both roots
- cancel stops/correctly ignores both workers
- root with no matches does not block completion
- result count is capped globally
- search highlights/open jump to the correct root

## 9. Mutations

Route by owning root:

- `create`: root-relative create through the owning gateway
- `rename`: same root only; otherwise `E: cannot move across workspace roots`
- `copy`: same root only; otherwise `E: cannot copy across workspace roots`
- `remove`: owning root
- save/write: already routed
- tree refresh: only affected root worker

Do not implement cross-root move/copy in this slice; it needs a staged
copy-verify-delete protocol and is outside the current trust model.

Acceptance:

- create/rename/remove in second root works
- cross-root rename/copy is rejected before mutation
- no primary-root fallback for second-root paths
- failed second-root operation leaves both roots untouched

## 10. Git

- Route `GitDiff`, `GitDiffText`, `GitApplyHunk`, `GitConflictsText`,
  `GitConflictStageText`, `GitResolveConflict`, and `GitBlameText` by owning
  root.
- Git status/branch state becomes per-root:
  `BTreeMap<PathBuf, GitRootState>`.
- Reject conflict stage/resolve operations outside a Git root.
- Do not merge index operations across roots.

Acceptance:

- gutter marks use the owning root
- conflict list is root-qualified
- applying a hunk in root B uses root B's gateway

## 11. LSP Boundary

LSP remains primary-root for now.

Reason:

- one language server per root would need server discovery, capability
  merging, diagnostics keyed by root, and workspace symbol scoping.
- `WorkspaceRoots` trust applies to file/Git operations first.

Document in `docs/ide-capability-plan.md` that multi-root LSP is a separate
follow-up; do not silently start one server against the wrong root.

## 12. Tests

Unit:

- `WorkspaceRoots::owning_root` deepest match
- root-relative path conversion for primary and secondary roots
- worker registry creation/drop on trust/untrust/remove
- fuzzy merge deduplication and display label
- project search merge/cancel/cap

Integration with fake gateways:

- two roots, two file workers
- `List` routes to the correct worker
- `Files` returns both roots
- `ProjectSearch` returns both roots and cancels both
- `Preview` reads from the correct root
- save/create/rename/remove route correctly
- cross-root rename/copy rejected

Regression:

- single-root behavior unchanged when `workspace_roots.len() == 1`
- untrusted root has no worker/gateway
- root removal drops worker and watchers

## 13. Rollout

1. Add worker registry; keep `file_worker` as primary alias temporarily.
2. Route List/Preview/read/write by root.
3. Add absolute-path `Files` and fuzzy merge.
4. Add project-search fan-out and cancellation.
5. Add tree root nodes/watch split.
6. Route mutations and Git.
7. Remove the single-worker field and fallback paths.
8. Update `docs/ide-capability-plan.md` and add one smoke test with two roots.

## 14. Risks

- **Path ambiguity**: absolute paths are required across worker boundaries.
- **Watch budget**: per-root watchers can exceed descriptor limits; split caps.
- **Cancellation**: generation must be global, not per root.
- **Root removal during in-flight request**: worker drop must ignore late
  events; events carry root and are dropped if the root is gone.
- **Primary root fallback**: never fall back to primary for a path owned by
  another root.
- **Untrusted root**: no gateway, no worker, no watch; UI must show why.

# ADR-0030: Workspace Index Module Extension Points

- Status: Accepted for implementation; bounded in-memory index, gateway-backed
  build/search, cancellation/stale tests, module registration, quick-open
  adapter, opt-in feature builds, and installed-binary smoke landed
- Date: 2026-11-13
- Related: [ADR-0028](0028-module-contract.md), [ADR-0029](0029-git-module-extension-points.md)

## Context

`0.7.2` adds an optional trusted workspace index/search service. The existing
quick-open path enumerates files through per-root gateway workers and the
project search runs bounded ripgrep through the same gateway boundary. A module
must not open a second filesystem path, own a second event loop, or force the
TUI to depend on module internals.

The index is performance-sensitive and can become unbounded. It therefore
needs explicit caps and cancellation before it is wired into quick-open.

## Decision

### 1. Service boundary

`anvil-module` defines a typed extension slot for a UI-neutral workspace index
service. The service receives the owning root and a gateway/session handle per
call; it does not own a global gateway.

Initial operations:

```rust
pub struct WorkspaceIndexSnapshot {
    pub generation: u64,
    pub paths: Vec<String>,
    pub truncated: bool,
}

pub trait WorkspaceIndexService: Send + Sync {
    fn build(
        &self,
        client: &mut GatewayClient,
        root: &Path,
        hidden: bool,
        generation: u64,
    ) -> Result<WorkspaceIndexSnapshot, WorkspaceIndexError>;

    fn search(
        &self,
        root: &Path,
        query: &str,
        limit: usize,
    ) -> Vec<String>;
}
```

The exact return types are refined with the implementation, but they remain
plain data and do not expose `anvil-tui` state.

### 2. Bounds

The index must enforce all of these before exposing results:

- maximum indexed files per root;
- maximum total path bytes per root;
- maximum result count for one query;
- maximum accepted query length;
- explicit truncation signal.

Starting caps are implementation decisions and must be tested at their limits.
The module must not silently grow past a cap.

### 3. Cancellation and stale results

- Every build has a generation.
- A new build invalidates older results for that root.
- Cancellation must return promptly without publishing a partial snapshot.
- Stale generations are ignored by the consumer.
- Root removal and trust changes invalidate the root's index and cancel pending
  builds.

### 4. I/O boundary

- Enumeration uses the owning root's gateway/`anvil-session` file APIs.
- The module must not read the filesystem directly.
- Index persistence, if added, needs a separate bounded/corruption test slice;
  the first implementation is in-memory only.
- No direct network, process, or plugin-token access.

### 5. Consumer boundary

The first consumer is quick-open path enumeration. The TUI requests an index for
the active workspace root and falls back to the existing gateway enumeration if
the module is absent.

Project search remains on the existing gateway/ripgrep path in 0.7.2. Replacing
it requires a separate ADR because result semantics and cancellation differ.

The TUI remains the only renderer and owner of the focus/overlay stack. The
module does not add UI widgets or a second event loop.

### 6. Feature and failure policy

- Module id: `official.workspace-index`.
- Cargo feature: `official-workspace-index`.
- Standard edition does not enable it by default until the memory/cancellation
  gates are measured; the feature is opt-in for 0.7.2.
- The minimal fixture does not enable it.
- Missing module: no index requests, existing quick-open behavior remains.
- Module registration/initialization failure fails closed.
- Runtime method errors are reported through the existing status surface; a
  failed build leaves the previous snapshot intact or absent, never partial.

### 7. Registration

The module registers through `ModuleRegistry` like `official.git`, using a
typed `set_workspace_index_service` slot. No untyped `Any` registry and no
runtime module discovery.

## Migration slices

1. Add contract-sized pure index data structure and bound tests.
2. Add the typed service slot and `anvil-mod-workspace-index` crate.
3. Implement gateway-backed build/search with cancellation and stale generation
   tests.
4. Wire quick-open to prefer the service and fall back when absent.
5. Add feature-matrix builds and installed-binary smoke.

Slices 1-5 are landed.

Each slice must leave the existing quick-open path working.

## Consequences

- The index is optional and can be excluded from a distribution.
- Caps and cancellation are contract requirements, not later optimizations.
- Quick-open has a measured migration path without changing project search
  semantics.
- The module cannot bypass the gateway boundary.

## Non-goals

- Replacing project search in this slice.
- On-disk index persistence.
- Background module-owned UI.
- A second gateway or direct filesystem walk.
- Enabling the index by default before caps are measured.

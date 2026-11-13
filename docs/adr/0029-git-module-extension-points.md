# ADR-0029: Git Module Extension Points

- Status: Accepted for implementation; helper extraction and `FileWorker`
  service injection, optional startup selection, and the Cargo feature matrix
  landed; command dispatch/completion gating and `official.git` registration
  landed
- Date: 2026-11-13
- Related: [ADR-0027](0027-distribution-composition-crate.md), [ADR-0028](0028-module-contract.md)

## Context

`0.7.1` moves the trusted Git integration behind an optional
`anvil-mod-git`. That cannot be a blind crate move: the existing integration is
split between TUI rendering/state, `FileWorker` requests, and gateway-backed Git
commands. A module must not depend on `anvil-tui` private state, and the TUI must
remain the only renderer and owner of focus/overlay behavior.

## Decision

### 1. Keep rendering in the TUI

`anvil-mod-git` does not render gutter marks, panels, or popups. It provides
Git data and operations. `anvil-tui` consumes the service and owns rendering.

### 2. Use one typed Git service extension point

`anvil-mod-git` exposes a UI-neutral trait, conceptually:

```rust
pub trait GitService: Send + Sync {
    fn status(&self, root: &Path) -> Result<Vec<GitStatusEntry>, GitError>;
    fn diff_text(&self, root: &Path, relative: &str, staged: bool) -> Result<GitText, GitError>;
    fn blame_text(&self, root: &Path, relative: &str) -> Result<GitText, GitError>;
    fn conflicts_text(&self, root: &Path) -> Result<GitText, GitError>;
    fn conflict_stage(&self, root: &Path, relative: &str, stage: u8) -> Result<GitText, GitError>;
    fn apply_hunk(&self, root: &Path, patch: &str, reverse: bool) -> Result<(), GitError>;
    fn resolve_conflict(&self, root: &Path, relative: &str, stage: u8) -> Result<(), GitError>;
}
```

The implemented service receives a per-call gateway/session handle; it does not
own a global gateway or a second process policy. Types returned by the trait
are plain data, not `anvil-tui` state.

### 3. Registration

`anvil-mod-git` registers:

- module id `official.git`;
- its command metadata;
- one `GitService` extension through the module registry.

The composition crate asks the registry for the `GitService` implementation and
passes an optional `Arc<dyn GitService>` into the TUI startup options. The TUI
does not discover modules or names.

The registry needs a typed extension slot rather than a generic untyped
`Any` bag; the exact slot API is added with the implementation, not assumed by
this ADR.

### 4. Feature matrix

- `anvil-distro-standard`: `official-git` enabled by default.
- minimal fixture: feature-free and has no Git service.
- feature-free `anvil-tui`: Git commands and gutter integration are absent, not
  silently broken.
- enabling `official-git` without the required kernel/gateway feature is a
  startup validation error.

### 5. Command behavior

Git command IDs remain stable. When no Git service is installed:

- the commands are not advertised as executable;
- a direct invocation reports `E: git module is not installed`;
- no fallback to a second Git implementation is allowed.

When a service is installed, behavior must match the pre-module Git path for
status marks, diffs, hunk staging, blame, conflict lists, previews, stage
selection, and ours/theirs resolution.

### 6. Failure policy

- Service construction failure fails the binary closed.
- A service method error is reported through the existing status/error surface.
- A panic in a service follows ADR-0028: fail closed by default.
- The module does not own the TUI event loop or a second renderer.

## Migration slices

1. Move pure Git helpers/parsers into `anvil-mod-git`; keep tests at parity.
2. Add the typed `GitService` registry slot and TUI startup option.
3. Route gutter/status/diff/blame/conflict operations through the service.
4. Add the feature matrix, no-module errors, and collision/version tests.
5. Verify standard and minimal binaries; remove the legacy hardcoded Git path.

No slice may leave both the old and new Git paths active at the same time.

## Consequences

- The first module exercises a real typed extension point, not metadata-only
  registration.
- `anvil-tui` remains the renderer and keeps Git UI behavior.
- `anvil-mod-git` can be excluded without breaking editing, LSP, tree, search,
  or startup.
- Adding another module requires a similarly typed extension point or a
  command/service contribution; an untyped `Any` registry is rejected.

## Non-goals

- A second gateway or direct `git` process bypass.
- Git UI widgets inside the module.
- Runtime module discovery.
- Moving core text/selection/undo semantics into the module.

# ADR-0061: Workspace Navigation and Root Boundary

- Status: Accepted; current navigation behavior documented 2026-11-20
- Version: 1.0.0
- Date: 2026-11-20
- Related: [ADR-0047](0047-file-tree-navigation-and-refresh.md),
  [ADR-0048](0048-per-root-language-tooling-and-lsp-workspace.md),
  [ADR-0055](0055-start-dashboard-actions-and-context.md),
  [ADR-0057](0057-tui-surface-model-and-focus-contract.md)
- Supersedes:
- Superseded by:

## Context

Anvil uses “workspace” in two different senses:

1. the workspace selected for one Anvil process/session;
2. additional roots attached to that session for multi-root editing.

The TUI exposes both through adjacent features:

- `Space w` opens the start page with the recent-workspace list focused;
- `:recent-workspaces` prints recent workspace paths in the status line;
- `:workspace-add`, `:workspace-remove`, `:workspace-trust`,
  `:workspace-untrust`, and `:workspace-list` manage roots inside the current
  session.

Without an explicit boundary, “workspace” can be read as a synonym for
“root”, and `Space w` can be mistaken for a root editor. This matters because
recent workspaces have different lifecycle and trust rules from roots.

## Decision

### 1. Keep session workspaces and session roots distinct

A **session workspace** is the canonical path used to start and scope one Anvil
instance. It owns the control socket, session state, and recent-workspace
entry.

A **root** is an additional directory attached to the current session for
tree, search, open/save, and service routing. Roots do not become recent
workspaces automatically.

### 2. `Space w` is the recent-workspace surface

`Space w` opens the start page with `WORKSPACES` focused. It:

- lists recent session workspaces;
- marks the current workspace;
- lets the current workspace close the page;
- reports missing or untrusted workspaces without mutating state;
- does not add, remove, trust, or untrust a root.

A non-current workspace selection currently reports that restarting Anvil at
that path is required. True in-process workspace switching is not part of this
decision; it needs a separate process-handoff design.

### 3. `:workspace-*` is the root-management surface

The `:workspace-add`, `:workspace-remove`, `:workspace-trust`,
`:workspace-untrust`, and `:workspace-list` commands operate only on roots of
the current session. They must not edit the recent-workspace list.

Root mutations remain admin-capability operations. Root listing is read-only.
`Space w` remains a read-only navigation surface.

### 4. `:recent-workspaces` remains a read-only textual view

`:recent-workspaces` and `Space w` read the same recent-workspace data but have
different presentation. A future change may make the command open the same
surface, but it must not turn the command into a root-management operation.

## Consequences

Positive:

- “workspace” and “root” have stable, testable meanings.
- `Space w` cannot accidentally change the current session’s root topology.
- Missing/untrusted workspace selection has an explicit failure state.

Negative:

- Users still need a restart to move to another workspace.
- Two recent-workspace entry points currently have different presentation.
- True workspace switching remains a separate lifecycle feature.

## Verification

- `Space w` from an editor opens the start page with `WORKSPACES` focused.
- Selecting the current workspace closes the page without mutating roots.
- Missing and untrusted workspace entries report an error and leave roots
  unchanged.
- `:workspace-add` adds a root without adding a recent-workspace entry.
- `:workspace-remove`, trust, and untrust affect only the current session’s
  root set.
- `:recent-workspaces` remains read-only.

# ADR-0061: Workspace Navigation and Root Boundary

- Status: Accepted; process handoff implemented 2026-11-20
- Version: 1.2.0
- Date: 2026-11-20
- Related: [ADR-0047](0047-file-tree-navigation-and-refresh.md),
  [ADR-0048](0048-per-root-language-tooling-and-lsp-workspace.md),
  [ADR-0055](0055-start-dashboard-actions-and-context.md),
  [ADR-0057](0057-tui-surface-model-and-focus-contract.md),
  [ADR-0062](0062-workspace-process-handoff.md)
- Supersedes:
- Superseded by:

## Context

Anvil uses “workspace” in two different senses:

1. the workspace selected for one Anvil process/session;
2. additional roots attached to that session for multi-root editing.

The TUI exposes both through adjacent features:

- `Space w` opens the start page with the recent-workspace list focused;
- `:recent-workspaces` opens the same start-page workspace list;
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

A non-current workspace selection starts the process handoff governed by
ADR-0062. It is not an in-process model mutation: the old process image is
replaced after the session is saved.

### 3. `:workspace-*` is the root-management surface

The `:workspace-add`, `:workspace-remove`, `:workspace-trust`,
`:workspace-untrust`, and `:workspace-list` commands operate only on roots of
the current session. They must not edit the recent-workspace list.

Root mutations remain admin-capability operations. Root listing is read-only.
`Space w` never edits roots; an available workspace selection performs a
process handoff instead.

### 4. `:recent-workspaces` opens the same navigation surface

`:recent-workspaces` and `Space w` open the same start-page workspace list.
The command is navigation, not root management: it must not add, remove,
trust, or untrust a root. Selecting an available workspace may initiate the
ADR-0062 handoff.

## Consequences

Positive:

- “workspace” and “root” have stable, testable meanings.
- `Space w` cannot accidentally change the current session’s root topology.
- Missing/untrusted workspace selection has an explicit failure state.
- Available workspace selection performs a process handoff after saving the
  session and releasing the old services.

Negative:

- The handoff replaces process state; dirty buffers, editor mode, and stdin
  mode refuse the switch.
- The process image is replaced while the PID is preserved; transient
  in-memory UI state does not carry across.

## Verification

- `Space w` from an editor opens the start page with `WORKSPACES` focused.
- Selecting the current workspace closes the page without mutating roots.
- Selecting a clean available workspace stages a process handoff.
- Modified buffers, editor mode, and stdin mode refuse the handoff.
- Missing and untrusted workspace entries report an error and leave roots
  unchanged.
- `:workspace-add` adds a root without adding a recent-workspace entry.
- `:workspace-remove`, trust, and untrust affect only the current session’s
  root set.
- `:recent-workspaces` opens the start page focused on `WORKSPACES`.

# ADR-0062: Workspace Process Handoff

- Status: Accepted; implemented 2026-11-20
- Version: 1.0.0
- Date: 2026-11-20
- Related: [ADR-0055](0055-start-dashboard-actions-and-context.md),
  [ADR-0061](0061-workspace-navigation-and-root-boundary.md)
- Supersedes:
- Superseded by:

## Context

ADR-0061 keeps session workspaces separate from multi-root directories and
defers true workspace switching to a process-handoff design. The start page
previously reported that the user must restart Anvil manually at the selected
workspace.

Anvil owns a gateway child, LSP workers, plugin state, a control socket, session
state, and the terminal raw-mode/alternate-screen lifecycle. Reinitializing all
of that in place would duplicate a large part of startup and risk leaving one
workspace's services attached to another workspace.

## Decision

### 1. Switch by replacing the process

Selecting an available recent workspace stages a pending handoff. The terminal
loop does not mutate the current `Model` into another workspace. It leaves the
loop, saves the current session, drops the terminal, model, and control server,
then replaces the process image with the same Anvil executable:

```text
anvil --workspace <canonical-path>
```

`--read-only` is preserved when the current session is read-only. Environment
variables such as `ANVIL_TRUECOLOR` and the sandbox profile are inherited.

The process replacement keeps the terminal session and the terminal window
attached. The new process records the workspace and restores its own session
normally.

### 2. Refuse unsafe handoffs

A handoff is refused when any buffer is modified:

```text
E: save or discard modified buffers before switching workspace
```

The user must save or discard changes first. Editor mode and stdin mode are also
unsupported because their lifetime is tied to the current file or pipe.

Missing and untrusted workspaces remain refused by the start-page state machine
before a handoff is staged.

### 3. Shut down the old ownership boundary

Before `exec`, the loop explicitly drops:

- the terminal session, restoring raw mode and the alternate screen;
- the model, stopping the gateway, root gateways, LSP workers, plugins, file
  workers, and installation state;
- the control server, removing its socket.

The new process must not inherit a live gateway or a second control server for
the old workspace.

### 4. Do not silently fall back

If process replacement fails, the error is returned to the distribution
entrypoint. Anvil does not start a second in-process model or silently keep
editing the old workspace after the user selected another one.

## Consequences

Positive:

- Workspace switching is a real handoff, not a status-line suggestion.
- The old workspace's services and control socket are released before the new
  process starts.
- Dirty buffers cannot be lost through an implicit switch.

Negative:

- The process PID is replaced; macros, pending requests, and unsaved UI state
  do not carry across the switch.
- Windows remains unsupported; the workspace handoff is a Unix `exec` path.
- Switching requires a clean buffer set.

## Verification

- Selecting a clean available workspace stages a pending handoff and closes the
  start page.
- Selecting a workspace with a modified buffer leaves the page open and keeps
  `pending_switch` empty.
- Editor and stdin modes refuse the switch.
- A real TUI smoke switch changed `Anvil · one` to `Anvil · two`, left one Anvil
  process and one gateway for the new workspace, and showed the new workspace as
  current.
- The previous workspace's gateway is gone after the handoff;
  `ControlServer`'s existing drop test covers socket removal.

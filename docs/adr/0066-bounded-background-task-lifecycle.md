# ADR-0066: Bounded Background Task Lifecycle

- Status: Accepted; task, file, LSP, DAP, and PTY lifecycle implemented
- Version: 1.0.0
- Date: 2026-11-20
- Related: [ADR-0002](0002-go-daemon.md),
  [ADR-0005](0005-exec-policy.md),
  [ADR-0012](0012-plugin-host-isolation.md),
  [ADR-0058](0058-sandbox-capability-profiles-and-trust-boundaries.md)
- Supersedes:
- Superseded by:

## Context

Anvil runs file enumeration, project search, LSP requests, formatters, tests,
tasks, DAP adapters, PTY shells, gateway ownership, and the plugin host on
background threads or child processes. Unbounded channels, detached children,
or shutdown by dropping handles can leak processes, lose errors, or hang exit.

## Decision

### 1. One bounded lifecycle

Background work follows:

```text
spawn -> running -> cancelling -> joined
                  -> failed
                  -> panicked
```

Every task has one owner and one join/cancel path. A worker cannot be dropped
as a substitute for cancellation.

### 2. Budgeted mailboxes

Task queues use global budget caps in addition to per-queue caps. A task cannot
reserve unbounded events or queued payload bytes. Budget exhaustion is returned
to the caller as an explicit error or applies the documented bounded
backpressure policy; it is never silent unbounded growth.

Draining is bounded per loop iteration. A busy worker cannot starve the editor
by producing an unbounded drain in one frame.

### 3. Cancellation and process ownership

Cancellation is cooperative for Rust threads and process-group based for
children. A cancelled task reports `cancelled`; a failed task reports the error;
a panic is observed and joined.

The owner that creates a child process or process group owns its termination.
Dropping a handle does not orphan a live child. The plugin host keeps its
cgroup/pidfd owner because it needs cgroup placement and kill-before-exec.

### 4. Deterministic shutdown

Shutdown order is explicit:

1. stop accepting new work;
2. cancel and join in reverse dependency order;
3. terminate child process groups;
4. close gateways and sockets;
5. report unfinished or leaked tasks.

Timeouts are explicit. Shutdown does not wait forever for an unresponsive
adapter.

### 5. No Tokio in the editor path

The Rust editor, TUI, and plugin path keep blocking threads and bounded
channels. Tokio is not introduced on the keystroke path by this decision.

## Consequences

Positive:

- One task cannot exhaust memory through an unbounded queue.
- Child processes and PTYs have an owner that terminates them.
- Shutdown is deterministic and reports leaks instead of silently dropping
  them.

Negative:

- Tasks must carry explicit cancellation and budget state.
- Some adapters need process-group handling rather than a simple join.
- The plugin host retains a specialized ownership path.

## Verification

- Queue budget and overflow tests.
- Cancellation and join tests for thread and process tasks.
- Process-group shutdown tests for DAP and PTY.
- Gateway graceful-shutdown fallback test.
- Plugin-host cgroup/pidfd ownership tests.
- Shutdown-order and unfinished-task reporting tests.

# Task Lifecycle Plan

Status: proposed
Target: keep the Rust editor on blocking threads and channels; no Tokio in the
editor, TUI, or plugin path.

## 1. Goal

One bounded, observable lifecycle for background work:

```text
spawn -> running -> cancelling -> joined
                  -> failed
                  -> panicked
                  -> restarted
```

The layer must:

- own every spawned thread or child process;
- cancel cooperatively and kill owned process groups on timeout;
- bound event delivery and coalesce high-frequency updates;
- report final state even when an event was dropped;
- shut down in reverse dependency order with per-task timeouts;
- remain usable without an async runtime.

Non-goals:

- a futures executor;
- Tokio in `anvil-core`, `anvil-editor`, `anvil-session`, or the TUI loop;
- replacing Go's internal gateway concurrency;
- a generic supervision tree with arbitrary restart policies.

## 2. Single Ownership Rule

Every `Child` and every `JoinHandle` has exactly one owner.

```text
Owner A: GatewaySession
  owns gateway Child, socket, token
  Drop/stop is the only gateway shutdown path

Owner B: TaskGroup
  owns LSP/DAP/file/task/plugin-host tasks in the first migration
```

The first implementation keeps `GatewaySession` as the sole gateway owner.
`TaskGroup` must not spawn a `ProcessTask` for the same child. This prevents:

- double `kill`/`wait`;
- double socket removal;
- a graceful close being bypassed by a second owner.

`TaskGroup` may expose gateway health as an observed state only. It must not
own or stop the gateway child until the ownership handoff is done in one
atomic change:

1. remove `GatewaySession::Drop`/`stop` ownership;
2. move `Child`, socket cleanup, and token cleanup into a `ProcessTask`;
3. update all callers to use the task handle.

Do not implement both halves in separate changes.

## 3. TaskGroup Shape

```rust
struct TaskGroup {
    states: HashMap<TaskId, TaskState>,       // authoritative
    dirty_states: BTreeSet<TaskId>,
    mailboxes: HashMap<TaskId, BoundedMailbox>,
    progress: HashMap<TaskId, ProgressState>, // latest only
    order: Vec<TaskId>,
    next_task: usize,
    config: TaskGroupConfig,
}
```

```rust
struct TaskGroupConfig {
    global_event_capacity: usize, // default 1024
    per_task_capacity: usize,     // default 256
    max_events_per_poll: usize,   // default 256
    poll_time_budget: Duration,   // default 2 ms
}
```

A task has a bounded mailbox; the group has a global cap. `poll` drains
lifecycle state changes first, then progress, then data in round-robin order.

## 4. Event Classes

| Class | Examples | Policy |
|---|---|---|
| Lifecycle | started, finished, failed, panicked, cancelled, restarted | State table is authoritative; event is a coalesced notification |
| Progress | index progress, task progress | One latest value per task; never queue every update |
| Data | LSP response, DAP event, file result | Bounded mailbox; overflow must fail the request or task |
| Coalescible | file changes, `didChange`, PTY output | Merge by key/path; overflow becomes `Rescan` or `Truncated` |
| Log | diagnostics, debug output | Rate limit or bounded ring buffer |

### Required policies

- Lifecycle: final state is queryable even if its notification is dropped.
- Progress: overwrite older values.
- LSP response: no silent drop; overflow fails the request with an explicit
  error or applies bounded backpressure.
- File watcher: collapse to one `Rescan(root)`.
- PTY: keep the latest bounded bytes plus a `Truncated` marker.
- Task output: bounded output plus `OutputTruncated`.

## 5. Polling

```rust
fn poll(&mut self) -> PollBatch {
    let deadline = Instant::now() + self.config.poll_time_budget;
    let mut events = Vec::with_capacity(self.config.max_events_per_poll);

    self.drain_dirty_states(&mut events);
    while events.len() < self.config.max_events_per_poll
        && Instant::now() < deadline
    {
        self.drain_one_round_robin(&mut events);
    }

    PollBatch {
        events,
        more: self.has_pending(),
    }
}
```

No unbounded drain in one frame. `more` means drain again on the next tick.

## 6. Cancellation

Rust threads cannot be forcibly aborted. Each task gets:

```rust
struct CancelToken { /* AtomicBool */ }
```

Rules:

- `CancelToken` is checked at safe points.
- Blocking receives use timeouts.
- Blocking file/socket reads use timeouts or close the descriptor.
- Child processes are killed as a process group.
- A thread that ignores cancellation is reported as `Leaked`, not waited on
  forever.
- `TaskGroup::shutdown` has a per-task timeout, then a global timeout.

## 7. Shutdown Order

### Phase 1 order

```text
1. stop accepting new commands
2. cancel TUI-dependent workers
3. cancel LSP/DAP/PTY/file workers
4. close gateway client/socket
5. stop plugin hosts
6. join TaskGroup tasks with timeouts
7. GatewaySession::stop (sole owner in phase 1)
```

Phase 1 does not promise a graceful gateway shutdown. The current
`GatewaySession::stop` path is:

```text
Child::kill
Child::wait
remove socket
```

It is immediate kill, not close-then-wait. The plan must not describe it as a
graceful close until Slice 5 implements that behavior.

### Target order after Slice 5

```text
1. stop accepting new commands
2. cancel TUI-dependent workers
3. cancel LSP/DAP/PTY/file workers
4. close gateway client/socket and start graceful shutdown
5. wait for gateway exit with a timeout
6. kill the gateway process group if the timeout expires
7. stop plugin hosts
8. join TaskGroup tasks with timeouts
```

The graceful-close sequence, timeout, and kill fallback are required Slice 5
work with their own tests. They are not assumed behavior of phase 1.

## 8. Gateway Boundary

`GatewaySession` remains the sole owner of:

```text
Child
socket path
tokens
```

TaskGroup observes gateway state, but does not own it:

```text
GatewayStarting
GatewayReady
GatewayExited
GatewayFailed
```

For phase 1, the TUI keeps `gateway_session: Option<GatewaySession>` and calls
the existing `stop` path. That path currently kills immediately; no graceful
close is claimed.

Slice 5 adds:

```text
GatewaySession::shutdown_gracefully(timeout)
  close client/socket
  request/allow the Go process to exit
  wait with timeout
  kill + wait as fallback
```

`Drop` remains a kill-based safety net until the ownership handoff is complete.
A later ownership handoff must be a dedicated change, not a parallel
ProcessTask.

## 9. Failure and Restart

Initial policies:

| Task | Policy |
|---|---|
| file worker | ReportOnly; recreate on next operation |
| task runner | OneShot |
| LSP worker | Restart with intensity limit |
| DAP worker | Restart or ReportOnly |
| plugin host | Restart with process cleanup |
| PTY | ReportOnly / user-visible error |
| control server | StopApp |
| gateway process | Owner-controlled restart; never double-owned |

Restart requires:

```text
max_restarts, period, backoff
```

No unlimited restart loop.

## 10. Migration Slices

### Slice 1: task layer tests

- `TaskId`, `CancelToken`, `TaskState`, `BoundedMailbox`.
- Lifecycle state table.
- Poll budget and coalescing.
- Fake thread task tests.

### Slice 2: short-lived workers

Wrap:

- `TaskWorker`
- `FileWorker`

Acceptance:

- no dropped final state;
- bounded event queues;
- shutdown timeout;
- no detached `JoinHandle`.

### Slice 3: process-backed workers

Wrap:

- DAP
- PTY
- plugin host

Acceptance:

- process-group kill;
- reaping;
- restart intensity.

### Slice 4: LSP

- bounded response queue;
- progress coalescing;
- request overflow policy;
- restart with state notification.

### Slice 5: gateway observation and graceful shutdown

- add read-only gateway state;
- use it from TaskGroup/UI;
- implement `GatewaySession::shutdown_gracefully(timeout)`;
- close the client/socket before waiting;
- wait for exit, then kill and wait as fallback;
- do not transfer Child ownership in this slice;
- keep `Drop` as the kill-based safety net.

Acceptance:

- graceful exit within the timeout is observed and does not call kill;
- timeout expiry falls back to kill and wait;
- gateway socket is removed exactly once;
- the gateway is stopped exactly once.

## 11. Tests

```text
[ ] lifecycle final state survives full mailbox
[ ] ten thousand progress updates produce one notification
[ ] LSP data overflow fails the request explicitly
[ ] file watcher overflow becomes one Rescan
[ ] a noisy task does not starve another task
[ ] poll budget stops and resumes next tick
[ ] shutdown with a full queue still joins
[ ] process task timeout kills its group
[ ] unresponsive thread is reported as Leaked
[ ] gateway is stopped exactly once
[ ] gateway graceful close waits for exit before fallback
[ ] gateway timeout fallback kills and waits
[ ] gateway socket is removed exactly once
[ ] a TaskGroup cannot spawn a second gateway owner
```

## 12. Acceptance

The slice is complete only when:

- all task handles are owned;
- no unbounded event `mpsc::channel` remains in the migrated path;
- every task exposes a final state;
- if graceful gateway shutdown is claimed, close/timeout/kill fallback is
  implemented and tested, not only documented;
- shutdown order and timeout are tested;
- gateway has exactly one shutdown owner;
- full workspace tests, clippy, and fmt pass.

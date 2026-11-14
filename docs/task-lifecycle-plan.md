# Task Lifecycle Plan

Status: Slice 1 core landed in `crates/anvil-task`; `TaskWorker`, `FileWorker`,
and `LspWorker` migrated; DAP and PTY migrated to `ProcessTask`; gateway
shutdown is graceful with a kill fallback; the plugin host keeps its
cgroup/pidfd owner because it needs cgroup placement and cgroup kill before
exec.
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
    data_budget: Arc<GlobalDataBudget>,        // shared by all mailboxes
    progress: HashMap<TaskId, ProgressState>, // latest only
    order: Vec<TaskId>,
    next_task: usize,
    config: TaskGroupConfig,
}
```

```rust
struct TaskGroupConfig {
    max_tasks: usize,                   // default 64
    global_data_event_capacity: usize,  // default 1024
    global_data_byte_capacity: usize,   // default 16 MiB
    per_task_capacity: usize,           // default 256
    max_events_per_poll: usize,         // default 256
    poll_time_budget: Duration,         // default 2 ms
}
```

A task has a bounded mailbox; every mailbox shares one
`GlobalDataBudget`, so `N tasks × per_task_capacity` cannot exceed
`global_data_event_capacity`, and queued payload bytes cannot exceed
`global_data_byte_capacity`. Push reserves from both budgets; a failed
reservation returns the data to the caller as `TaskFull`, `GlobalFull`, or
`GlobalBytesFull`. `spawn` refuses work beyond `max_tasks`. `poll` drains
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

### Current order

Model teardown follows this order:

1. stop accepting new commands
2. stop TUI task and PTY workers
3. drop file, LSP, and DAP workers
4. stop plugin hosts and revoke their grants
5. drop gateway clients
6. call `GatewaySession::shutdown_gracefully(250ms)` for the primary, root,
   and installer gateways
7. remaining `TaskGroup` and PTY drops use their own timeouts

Gateway shutdown sends `SIGTERM`, waits for exit, and only falls back to
kill-and-wait when the timeout expires. The socket path is removed once.
`Drop` remains a kill-based safety net for ungraceful teardown.

## 8. Gateway Boundary

`GatewaySession` remains the sole owner of:

```text
Child
socket path
tokens
```

`TaskGroup` neither owns nor observes the gateway. Model teardown cancels
workers, drops gateway clients, and then calls `shutdown_gracefully`; the
`GatewaySession` remains the sole owner of the child and socket.

Slice 5 implementation:


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

### Slice 5: gateway graceful shutdown

- keep `Child` and socket ownership in `GatewaySession`;
- implement `GatewaySession::shutdown_gracefully(timeout)`;
- close gateway clients before waiting;
- wait for exit, then kill and wait as fallback;
- keep `Drop` as the kill-based safety net.

Acceptance:

- graceful exit within the timeout is observed and does not call kill;
- timeout expiry falls back to kill and wait;
- gateway socket is removed exactly once;
- the gateway is stopped exactly once.

## 11. Tests

```text
[x] lifecycle final state survives full mailbox
[x] ten thousand progress updates produce one notification
[x] per-task and global data capacities are both enforced
[x] max tasks rejects excess spawns
[x] global byte budget rejects oversized events
[x] file worker event costs include heap payload
[x] oversized file event becomes one Oversized notification
[x] LSP command and event queues are bounded
[x] LSP diagnostics are coalesced by path
[x] LSP data overflow fails the request explicitly
[x] tree watch cap is reported by the tree model
[x] a noisy task does not starve another task
[x] poll budget stops and resumes next tick
[x] file worker drop interrupts blocked gateway I/O
[x] shutdown with a full queue still joins
[x] process task timeout kills its group
[x] unresponsive thread is reported as Leaked
[x] gateway is stopped exactly once
[x] gateway graceful close waits for exit before fallback
[x] gateway timeout fallback kills and waits
[x] gateway socket is removed exactly once
[x] GatewaySession is the sole gateway owner
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

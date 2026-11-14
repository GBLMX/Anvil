//! Bounded task lifecycle and event delivery.
//!
//! The group owns every thread and exposes one authoritative state table.
//! Progress is coalesced, data mailboxes are bounded per task and globally,
//! and polling has an event/time budget. No async runtime is involved.

use std::{
    any::Any,
    collections::{BTreeMap, VecDeque},
    fmt, io,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Stable identity of one task in a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(u64);

impl fmt::Display for TaskId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "task-{}", self.0)
    }
}

/// Authoritative lifecycle state for a task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskState {
    /// The task is still executing.
    Running,
    /// The task returned successfully.
    Finished,
    /// The task returned an error.
    Failed(String),
    /// The task panicked; the wrapper caught it.
    Panicked(String),
    /// The task observed cancellation.
    Cancelled,
    /// Shutdown timed out before the task could be joined.
    Leaked,
}

impl TaskState {
    /// Whether the task has reached a final state.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Running)
    }
}

/// Cooperative cancellation shared by a task and its owner.
#[derive(Clone, Debug, Default)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
}

impl CancelToken {
    /// Create an uncancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Latest progress value for one task.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Completed units.
    pub done: u64,
    /// Optional total units.
    pub total: Option<u64>,
    /// Optional human-readable detail.
    pub message: Option<String>,
}

/// One event produced by a task or by lifecycle reconciliation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskEvent<D> {
    /// A task reached a final state or was reconciled by the group.
    State { id: TaskId, state: TaskState },
    /// The latest coalesced progress for a task.
    Progress { id: TaskId, progress: Progress },
    /// One bounded data item from a task.
    Data { id: TaskId, data: D },
}

/// One bounded poll result.
#[derive(Debug)]
pub struct PollBatch<D> {
    /// Events delivered within the configured budget.
    pub events: Vec<TaskEvent<D>>,
    /// Whether more events remain for the next poll.
    pub more: bool,
}

/// Why a data item was rejected.
#[derive(Debug, PartialEq, Eq)]
pub enum DataError<D> {
    /// This task's mailbox is full.
    TaskFull(D),
    /// The group-wide data budget is full.
    GlobalFull(D),
    /// The group-wide byte budget is full.
    GlobalBytesFull(D),
    /// The group or mailbox has closed.
    Closed(D),
}

/// Configuration for one task group.
#[derive(Clone, Debug)]
pub struct TaskGroupConfig {
    /// Maximum live tasks in the group.
    pub max_tasks: usize,
    /// Maximum queued data items across every task in the group.
    pub global_data_event_capacity: usize,
    /// Maximum queued data bytes across every task in the group.
    pub global_data_byte_capacity: usize,
    /// Maximum queued data items in one task's mailbox.
    pub per_task_capacity: usize,
    /// Maximum events returned by one `poll`.
    pub max_events_per_poll: usize,
    /// Maximum wall time spent collecting one `poll`.
    pub poll_time_budget: Duration,
}

impl Default for TaskGroupConfig {
    fn default() -> Self {
        Self {
            max_tasks: 64,
            global_data_event_capacity: 1024,
            global_data_byte_capacity: 16 * 1024 * 1024,
            per_task_capacity: 256,
            max_events_per_poll: 256,
            poll_time_budget: Duration::from_millis(2),
        }
    }
}

impl TaskGroupConfig {
    fn normalized(mut self) -> Self {
        self.max_tasks = self.max_tasks.max(1);
        self.global_data_event_capacity = self.global_data_event_capacity.max(1);
        self.global_data_byte_capacity = self.global_data_byte_capacity.max(1);
        self.per_task_capacity = self.per_task_capacity.max(1);
        self.max_events_per_poll = self.max_events_per_poll.max(1);
        self
    }
}

/// Why a task could not be spawned.
#[derive(Debug)]
pub enum SpawnError {
    /// The group already owns `max_tasks` live tasks.
    AtCapacity,
    /// The OS thread could not be created.
    Thread(io::Error),
}

impl fmt::Display for SpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AtCapacity => formatter.write_str("task group is at capacity"),
            Self::Thread(error) => write!(formatter, "task thread failed: {error}"),
        }
    }
}

impl std::error::Error for SpawnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Thread(error) => Some(error),
            Self::AtCapacity => None,
        }
    }
}

/// Shared group-wide data-event and byte budget.
struct GlobalDataBudget {
    event_capacity: usize,
    event_used: AtomicUsize,
    byte_capacity: usize,
    byte_used: AtomicUsize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BudgetError {
    Events,
    Bytes,
}

impl GlobalDataBudget {
    fn new(event_capacity: usize, byte_capacity: usize) -> Self {
        Self {
            event_capacity,
            event_used: AtomicUsize::new(0),
            byte_capacity,
            byte_used: AtomicUsize::new(0),
        }
    }

    fn try_add(counter: &AtomicUsize, capacity: usize, amount: usize) -> bool {
        let mut current = counter.load(Ordering::Acquire);
        loop {
            let Some(next) = current.checked_add(amount) else {
                return false;
            };
            if next > capacity {
                return false;
            }
            match counter.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }

    fn try_acquire(&self, bytes: usize) -> Result<(), BudgetError> {
        if bytes > 0 && !Self::try_add(&self.byte_used, self.byte_capacity, bytes) {
            return Err(BudgetError::Bytes);
        }
        if !Self::try_add(&self.event_used, self.event_capacity, 1) {
            if bytes > 0 {
                self.byte_used.fetch_sub(bytes, Ordering::AcqRel);
            }
            return Err(BudgetError::Events);
        }
        Ok(())
    }

    fn release(&self, bytes: usize) {
        self.event_used.fetch_sub(1, Ordering::AcqRel);
        if bytes > 0 {
            self.byte_used.fetch_sub(bytes, Ordering::AcqRel);
        }
    }
}

struct DataItem<D> {
    data: D,
    bytes: usize,
}

struct DataMailbox<D> {
    capacity: usize,
    budget: Arc<GlobalDataBudget>,
    queue: Mutex<VecDeque<DataItem<D>>>,
    closed: AtomicBool,
}

impl<D> DataMailbox<D> {
    fn new(capacity: usize, budget: Arc<GlobalDataBudget>) -> Self {
        Self {
            capacity,
            budget,
            queue: Mutex::new(VecDeque::new()),
            closed: AtomicBool::new(false),
        }
    }

    fn try_push(&self, data: D) -> Result<(), DataError<D>> {
        self.try_push_with_cost(data, 0)
    }

    fn try_push_with_cost(&self, data: D, bytes: usize) -> Result<(), DataError<D>> {
        let mut queue = match self.queue.lock() {
            Ok(queue) => queue,
            Err(_) => return Err(DataError::Closed(data)),
        };
        if self.closed.load(Ordering::Acquire) {
            return Err(DataError::Closed(data));
        }
        if queue.len() >= self.capacity {
            return Err(DataError::TaskFull(data));
        }
        match self.budget.try_acquire(bytes) {
            Ok(()) => {}
            Err(BudgetError::Events) => return Err(DataError::GlobalFull(data)),
            Err(BudgetError::Bytes) => return Err(DataError::GlobalBytesFull(data)),
        }
        queue.push_back(DataItem { data, bytes });
        Ok(())
    }

    fn try_pop(&self) -> Option<D> {
        let mut queue = self.queue.lock().ok()?;
        let item = queue.pop_front();
        if let Some(item) = item.as_ref() {
            self.budget.release(item.bytes);
        }
        item.map(|item| item.data)
    }

    fn len(&self) -> usize {
        self.queue.lock().map(|queue| queue.len()).unwrap_or(0)
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Ok(mut queue) = self.queue.lock() {
            while let Some(item) = queue.pop_front() {
                self.budget.release(item.bytes);
            }
        }
    }
}

impl<D> Drop for DataMailbox<D> {
    fn drop(&mut self) {
        self.close();
    }
}

/// Context available to a running task.
#[derive(Clone)]
pub struct TaskContext<D> {
    id: TaskId,
    cancel: CancelToken,
    progress: Arc<Mutex<BTreeMap<TaskId, Progress>>>,
    mailbox: Arc<DataMailbox<D>>,
    data_byte_capacity: usize,
}

impl<D> TaskContext<D> {
    /// Identity of this task.
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Cancellation token owned by this task.
    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Publish the latest progress; older values are replaced.
    pub fn report_progress(&self, progress: Progress) {
        if let Ok(mut progress_map) = self.progress.lock() {
            progress_map.insert(self.id, progress);
        }
    }

    /// Queue one bounded data event.
    pub fn send_data(&self, data: D) -> Result<(), DataError<D>> {
        self.mailbox.try_push(data)
    }

    /// Maximum bytes a single data event may charge.
    pub fn data_byte_capacity(&self) -> usize {
        self.data_byte_capacity
    }
    /// Queue one bounded data event charged against the byte budget.
    pub fn send_data_with_cost(&self, data: D, bytes: usize) -> Result<(), DataError<D>> {
        self.mailbox.try_push_with_cost(data, bytes)
    }
}

struct TaskOutcome {
    result: Result<(), String>,
    panic: Option<String>,
}

struct TaskEntry<D> {
    id: TaskId,
    cancel: CancelToken,
    mailbox: Arc<DataMailbox<D>>,
    handle: Option<JoinHandle<TaskOutcome>>,
    state: TaskState,
    state_reported: bool,
    cancelled_while_running: bool,
}

impl<D> TaskEntry<D> {
    fn new(id: TaskId, cancel: CancelToken, mailbox: Arc<DataMailbox<D>>) -> Self {
        Self {
            id,
            cancel,
            mailbox,
            handle: None,
            state: TaskState::Running,
            state_reported: false,
            cancelled_while_running: false,
        }
    }
}

/// A group of bounded, owned background tasks.
pub struct TaskGroup<D> {
    next_id: u64,
    entries: Vec<TaskEntry<D>>,
    progress: Arc<Mutex<BTreeMap<TaskId, Progress>>>,
    budget: Arc<GlobalDataBudget>,
    config: TaskGroupConfig,
    next_task: usize,
}

impl<D: Send + 'static> TaskGroup<D> {
    /// Create a group with default limits.
    pub fn new() -> Self {
        Self::with_config(TaskGroupConfig::default())
    }

    /// Create a group with explicit limits.
    pub fn with_config(config: TaskGroupConfig) -> Self {
        let config = config.normalized();
        Self {
            next_id: 1,
            entries: Vec::new(),
            progress: Arc::new(Mutex::new(BTreeMap::new())),
            budget: Arc::new(GlobalDataBudget::new(
                config.global_data_event_capacity,
                config.global_data_byte_capacity,
            )),
            config,
            next_task: 0,
        }
    }

    /// Spawn one thread-owned task.
    pub fn spawn<F>(&mut self, name: impl Into<String>, task: F) -> Result<TaskId, SpawnError>
    where
        F: FnOnce(TaskContext<D>) -> Result<(), String> + Send + 'static,
    {
        let live = self
            .entries
            .iter()
            .filter(|entry| {
                entry
                    .handle
                    .as_ref()
                    .is_some_and(|handle| !handle.is_finished())
            })
            .count();
        if live >= self.config.max_tasks {
            return Err(SpawnError::AtCapacity);
        }
        let id = TaskId(self.next_id);
        self.next_id += 1;
        let name = name.into();
        let cancel = CancelToken::new();
        let mailbox = Arc::new(DataMailbox::new(
            self.config.per_task_capacity,
            Arc::clone(&self.budget),
        ));
        let context = TaskContext {
            id,
            cancel: cancel.clone(),
            progress: Arc::clone(&self.progress),
            mailbox: Arc::clone(&mailbox),
            data_byte_capacity: self.config.global_data_byte_capacity,
        };
        let thread_name = format!("anvil-{id}-{name}");
        let handle = thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                let outcome = catch_unwind(AssertUnwindSafe(|| task(context)));
                match outcome {
                    Ok(result) => TaskOutcome {
                        result,
                        panic: None,
                    },
                    Err(payload) => {
                        let message = panic_message(payload);
                        TaskOutcome {
                            result: Err(message.clone()),
                            panic: Some(message),
                        }
                    }
                }
            })
            .map_err(SpawnError::Thread)?;
        let mut entry = TaskEntry::new(id, cancel, mailbox);
        entry.handle = Some(handle);
        self.entries.push(entry);
        Ok(id)
    }

    /// Current state, or `None` for an unknown task.
    pub fn state(&self, id: TaskId) -> Option<&TaskState> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| &entry.state)
    }

    /// Request cancellation for one task.
    pub fn cancel(&mut self, id: TaskId) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            if entry
                .handle
                .as_ref()
                .is_some_and(|handle| !handle.is_finished())
            {
                entry.cancelled_while_running = true;
            }
            entry.cancel.cancel();
        }
    }

    /// Request cancellation for every task.
    pub fn cancel_all(&mut self) {
        for entry in &mut self.entries {
            if entry
                .handle
                .as_ref()
                .is_some_and(|handle| !handle.is_finished())
            {
                entry.cancelled_while_running = true;
            }
            entry.cancel.cancel();
        }
    }

    /// Number of tasks in the group.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the group has no tasks.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drain states, coalesced progress, and bounded data within one budget.
    pub fn poll(&mut self) -> PollBatch<D> {
        self.reap();
        let deadline = Instant::now() + self.config.poll_time_budget;
        let mut events = Vec::with_capacity(self.config.max_events_per_poll);

        for entry in &mut self.entries {
            if entry.state_reported || !entry.state.is_terminal() {
                continue;
            }
            entry.state_reported = true;
            events.push(TaskEvent::State {
                id: entry.id,
                state: entry.state.clone(),
            });
            if events.len() >= self.config.max_events_per_poll {
                break;
            }
        }

        if events.len() < self.config.max_events_per_poll {
            if let Ok(mut progress) = self.progress.lock() {
                while events.len() < self.config.max_events_per_poll {
                    let Some((id, progress)) = progress.pop_first() else {
                        break;
                    };
                    events.push(TaskEvent::Progress { id, progress });
                }
            }
        }

        while events.len() < self.config.max_events_per_poll && Instant::now() < deadline {
            let mut advanced = false;
            let len = self.entries.len();
            for offset in 0..len {
                let index = (self.next_task + offset) % len;
                if let Some(data) = self.entries[index].mailbox.try_pop() {
                    events.push(TaskEvent::Data {
                        id: self.entries[index].id,
                        data,
                    });
                    advanced = true;
                    if events.len() >= self.config.max_events_per_poll {
                        break;
                    }
                }
            }
            if len > 0 {
                self.next_task = (self.next_task + 1) % len;
            }
            if !advanced {
                break;
            }
        }

        let more = self.has_pending();
        PollBatch { events, more }
    }

    /// Cancel all tasks and wait up to `timeout` for them to finish.
    pub fn shutdown(&mut self, timeout: Duration) -> ShutdownReport {
        self.reap();
        self.cancel_all();
        let deadline = Instant::now() + timeout;
        loop {
            self.reap();
            if self.entries.iter().all(|entry| entry.handle.is_none()) {
                break;
            }
            if Instant::now() >= deadline {
                for entry in &mut self.entries {
                    if entry.handle.is_some() {
                        entry.state = TaskState::Leaked;
                        entry.state_reported = false;
                        entry.handle = None;
                    }
                }
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        let leaked: Vec<TaskId> = self
            .entries
            .iter()
            .filter(|entry| matches!(entry.state, TaskState::Leaked))
            .map(|entry| entry.id)
            .collect();
        ShutdownReport {
            joined: self.entries.len().saturating_sub(leaked.len()),
            leaked,
        }
    }

    fn reap(&mut self) {
        for entry in &mut self.entries {
            let Some(handle) = entry.handle.as_ref() else {
                continue;
            };
            if !handle.is_finished() {
                continue;
            }
            let handle = entry.handle.take().expect("finished handle");
            let outcome = handle.join().unwrap_or(TaskOutcome {
                result: Err("task thread panicked".to_owned()),
                panic: Some("task thread panicked".to_owned()),
            });
            entry.state = if entry.cancelled_while_running {
                TaskState::Cancelled
            } else if let Some(panic) = outcome.panic {
                TaskState::Panicked(panic)
            } else {
                match outcome.result {
                    Ok(()) => TaskState::Finished,
                    Err(error) => TaskState::Failed(error),
                }
            };
            entry.state_reported = false;
        }
    }

    fn has_pending(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| !entry.state_reported && entry.state.is_terminal())
            || self.entries.iter().any(|entry| entry.mailbox.len() > 0)
            || self.progress.lock().map(|p| !p.is_empty()).unwrap_or(false)
    }
}

impl<D: Send + 'static> Default for TaskGroup<D> {
    fn default() -> Self {
        Self::new()
    }
}

/// Dropping a group cancels and closes its tasks. Threads that do not observe
/// cancellation are detached; call [`TaskGroup::shutdown`] to join them.
impl<D> Drop for TaskGroup<D> {
    fn drop(&mut self) {
        for entry in &mut self.entries {
            entry.cancel.cancel();
            entry.mailbox.close();
            entry.handle = None;
            entry.state = TaskState::Leaked;
        }
    }
}

/// Result of a group shutdown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShutdownReport {
    /// Tasks joined before the timeout.
    pub joined: usize,
    /// Tasks left running because they did not observe cancellation.
    pub leaked: Vec<TaskId>,
}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "task panicked".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Barrier,
        atomic::{AtomicUsize, Ordering},
    };

    fn wait_terminals<D: Send + 'static>(
        group: &mut TaskGroup<D>,
        ids: &[TaskId],
    ) -> Vec<TaskEvent<D>> {
        let mut events = Vec::new();
        for _ in 0..2_000 {
            let batch = group.poll();
            events.extend(batch.events);
            if ids
                .iter()
                .all(|id| group.state(*id).is_some_and(TaskState::is_terminal))
            {
                return events;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("tasks did not reach a terminal state");
    }

    #[test]
    fn finished_and_panicked_tasks_report_states() {
        let mut group = TaskGroup::<()>::new();
        let finished = group.spawn("ok", |_| Ok(())).expect("spawn");
        let panicked = group.spawn("panic", |_| panic!("boom")).expect("spawn");
        let events = wait_terminals(&mut group, &[finished, panicked]);
        assert_eq!(group.state(finished), Some(&TaskState::Finished));
        assert!(matches!(
            group.state(panicked),
            Some(TaskState::Panicked(_))
        ));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, TaskEvent::State { id, .. } if *id == finished))
        );
    }

    #[test]
    fn cancellation_is_cooperative_and_terminal() {
        let mut group = TaskGroup::<()>::new();
        let id = group
            .spawn("cancel", |context| {
                while !context.is_cancelled() {
                    thread::sleep(Duration::from_millis(1));
                }
                Ok(())
            })
            .expect("spawn");
        group.cancel(id);
        wait_terminals(&mut group, &[id]);
        assert_eq!(group.state(id), Some(&TaskState::Cancelled));
    }

    #[test]
    fn shutdown_keeps_completed_state() {
        let mut group = TaskGroup::<()>::new();
        let id = group.spawn("done", |_| Ok(())).expect("spawn");
        for _ in 0..100 {
            if group
                .entries
                .iter()
                .find(|entry| entry.id == id)
                .and_then(|entry| entry.handle.as_ref())
                .is_some_and(|handle| handle.is_finished())
            {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        let report = group.shutdown(Duration::from_millis(100));
        assert_eq!(report.joined, 1);
        assert_eq!(group.state(id), Some(&TaskState::Finished));
    }

    #[test]
    fn progress_is_coalesced_per_task() {
        let mut group = TaskGroup::<()>::new();
        let ready = Arc::new(Barrier::new(2));
        let task_ready = Arc::clone(&ready);
        let id = group
            .spawn("progress", move |context| {
                for done in 0..10_000 {
                    context.report_progress(Progress {
                        done,
                        total: Some(10_000),
                        message: None,
                    });
                }
                task_ready.wait();
                while !context.is_cancelled() {
                    thread::sleep(Duration::from_millis(1));
                }
                Ok(())
            })
            .expect("spawn");
        ready.wait();
        let batch = group.poll();
        let progress_events = batch
            .events
            .iter()
            .filter(|event| matches!(event, TaskEvent::Progress { id: event_id, .. } if *event_id == id))
            .count();
        assert_eq!(progress_events, 1);
        group.cancel(id);
        wait_terminals(&mut group, &[id]);
    }

    #[test]
    fn per_task_and_global_data_capacities_are_enforced() {
        let config = TaskGroupConfig {
            global_data_event_capacity: 2,
            per_task_capacity: 1,
            max_events_per_poll: 16,
            ..TaskGroupConfig::default()
        };
        let mut group = TaskGroup::<&'static str>::with_config(config);
        let barrier = Arc::new(Barrier::new(2));
        let accepted = Arc::new(AtomicUsize::new(0));
        let mut ids = Vec::new();
        for name in ["one", "two"] {
            let barrier = Arc::clone(&barrier);
            let accepted = Arc::clone(&accepted);
            ids.push(
                group
                    .spawn(name, move |context| {
                        barrier.wait();
                        for item in ["a", "b"] {
                            if context.send_data(item).is_ok() {
                                accepted.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        Ok(())
                    })
                    .expect("spawn"),
            );
        }
        let events = wait_terminals(&mut group, &ids);
        let data_events = events
            .iter()
            .filter(|event| matches!(event, TaskEvent::Data { .. }))
            .count();
        assert_eq!(accepted.load(Ordering::Relaxed), 2);
        assert_eq!(data_events, 2);
    }

    #[test]
    fn poll_budget_stops_and_resumes() {
        let config = TaskGroupConfig {
            global_data_event_capacity: 8,
            per_task_capacity: 8,
            max_events_per_poll: 1,
            ..TaskGroupConfig::default()
        };
        let mut group = TaskGroup::<&'static str>::with_config(config);
        let id = group
            .spawn("data", |context| {
                for item in ["a", "b", "c"] {
                    context.send_data(item).expect("bounded data");
                }
                Ok(())
            })
            .expect("spawn");
        let mut data_events = 0;
        for _ in 0..20 {
            let batch = group.poll();
            assert!(batch.events.len() <= 1);
            data_events += batch
                .events
                .iter()
                .filter(|event| matches!(event, TaskEvent::Data { .. }))
                .count();
            if group.state(id).is_some_and(TaskState::is_terminal) && data_events == 3 {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(data_events, 3);
    }

    #[test]
    fn spawn_enforces_max_tasks() {
        let config = TaskGroupConfig {
            max_tasks: 1,
            ..TaskGroupConfig::default()
        };
        let mut group = TaskGroup::<()>::with_config(config);
        let first = group
            .spawn("hold", |context| {
                while !context.is_cancelled() {
                    thread::sleep(Duration::from_millis(1));
                }
                Ok(())
            })
            .expect("first spawn");
        assert!(matches!(
            group.spawn("second", |_| Ok(())),
            Err(SpawnError::AtCapacity)
        ));
        group.shutdown(Duration::from_millis(100));
        assert_eq!(group.state(first), Some(&TaskState::Cancelled));
    }

    #[test]
    fn byte_budget_is_enforced() {
        let config = TaskGroupConfig {
            global_data_event_capacity: 10,
            global_data_byte_capacity: 8,
            per_task_capacity: 10,
            ..TaskGroupConfig::default()
        };
        let mut group = TaskGroup::<&'static str>::with_config(config);
        let id = group
            .spawn("bytes", |context| {
                if !matches!(
                    context.send_data_with_cost("too-big", 9),
                    Err(DataError::GlobalBytesFull("too-big"))
                ) {
                    return Err("byte budget accepted an oversized event".to_owned());
                }
                context
                    .send_data_with_cost("ok", 4)
                    .map_err(|error| format!("byte budget rejected valid event: {error:?}"))
            })
            .expect("spawn");
        let events = wait_terminals(&mut group, &[id]);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, TaskEvent::Data { data: "ok", .. }))
        );
    }

    #[test]
    fn shutdown_timeout_marks_leaked() {
        let mut group = TaskGroup::<()>::new();
        let id = group
            .spawn("slow", |_| {
                thread::sleep(Duration::from_millis(30));
                Ok(())
            })
            .expect("spawn");
        let report = group.shutdown(Duration::from_millis(1));
        assert_eq!(report.leaked, vec![id]);
        thread::sleep(Duration::from_millis(40));
    }

    #[test]
    fn shutdown_with_full_mailbox_still_joins() {
        let config = TaskGroupConfig {
            global_data_event_capacity: 1,
            per_task_capacity: 1,
            max_events_per_poll: 1,
            ..TaskGroupConfig::default()
        };
        let mut group = TaskGroup::<&'static str>::with_config(config);
        let ready = Arc::new(Barrier::new(2));
        let task_ready = Arc::clone(&ready);
        let id = group
            .spawn("full", move |context| {
                context.send_data("queued").expect("first data");
                task_ready.wait();
                while !context.is_cancelled() {
                    thread::sleep(Duration::from_millis(1));
                }
                Ok(())
            })
            .expect("spawn");
        ready.wait();
        let report = group.shutdown(Duration::from_secs(1));
        assert_eq!(report.joined, 1);
        assert!(report.leaked.is_empty());
        assert_eq!(group.state(id), Some(&TaskState::Cancelled));
    }

    #[test]
    fn noisy_task_does_not_starve_another_task() {
        let config = TaskGroupConfig {
            global_data_event_capacity: 16,
            per_task_capacity: 16,
            max_events_per_poll: 8,
            poll_time_budget: Duration::from_millis(100),
            ..TaskGroupConfig::default()
        };
        let mut group = TaskGroup::<&'static str>::with_config(config);
        let ready = Arc::new(Barrier::new(3));
        let mut ids = Vec::new();
        for name in ["one", "two"] {
            let ready = Arc::clone(&ready);
            ids.push(
                group
                    .spawn(name, move |context| {
                        ready.wait();
                        for _ in 0..4 {
                            context.send_data("x").expect("data");
                        }
                        while !context.is_cancelled() {
                            thread::sleep(Duration::from_millis(1));
                        }
                        Ok(())
                    })
                    .expect("spawn"),
            );
        }
        ready.wait();
        let mut seen = Vec::new();
        for _ in 0..20 {
            for event in group.poll().events {
                if let TaskEvent::Data { id, .. } = event {
                    seen.push(id);
                }
            }
            if seen.len() == 8 {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(seen.len(), 8);
        assert_eq!(seen.iter().filter(|id| **id == ids[0]).count(), 4);
        assert_eq!(seen.iter().filter(|id| **id == ids[1]).count(), 4);
        group.shutdown(Duration::from_millis(100));
    }
}

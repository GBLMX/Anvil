# Ratatui TUI Elm Loop Plan

Status: planned; no runnable editor crate yet.

## 1. Goal

Build the first `anvil-tui` runtime around a strict Elm-style loop:

```text
terminal event ─┐
background result ├─> Msg ─> update(Model, Msg) ─> (Model, Cmd[])
plugin event ────┘                              │
                                                └─> Cmd runtime
                                                        │
                                                        └─> background Msg
```

`update` is synchronous, deterministic, and performs no I/O. The renderer is a
pure projection of `Model`. Slow work is represented by `Cmd` values and runs in
background threads.

The loop uses `ratatui` for rendering and `crossterm` for terminal input. It does
not add Tokio, an actor framework, or a second event loop.

## 2. Boundaries

### In scope

- `anvil-tui` crate and terminal lifecycle.
- `Model`, `Msg`, `Cmd`, `Subscription`, `update`, `view`, and runtime.
- Selection-first input dispatch through `anvil-editor`/`anvil-core`.
- Viewport rendering and status line.
- Lazy gateway startup that never blocks the first frame.
- Background file open/save, gateway calls, and cancellation by generation.
- Testable terminal, input, and render layers without a real TTY.

### Out of scope

- Plugin-host runtime and Lua/QuickJS loading.
- Syntax highlighting engine.
- Full plugin keymap loading.
- TUI mouse support beyond a disabled-by-default stub.
- Network fetch/exec policy; those remain gateway responsibilities.

## 3. Dependencies

- `ratatui` `0.30` and `crossterm` `0.29`, matching ADR-0006.
- `anvil-core` and `anvil-editor` for text, selection, snapshots, and commands.
- Standard-library threads, channels, and `Duration` for the runtime.
- No Tokio, no actor runtime, no async executor.

## 4. Elm types

Proposed module layout:

```text
crates/anvil-tui/src/
  lib.rs
  model.rs
  msg.rs
  update.rs
  command.rs
  runtime.rs
  subscriptions.rs
  view.rs
  input.rs
  terminal.rs
```

### Model

`Model` owns only UI state and shared editor references:

```text
Model
  editor: EditorModel/Session
  view: EditorSnapshot/EditorView
  focus: Focus (Editor | Overlay | Status)
  overlay: Option<Overlay>
  gateway: GatewayState (NotStarted | Starting | Ready | Failed)
  plugins: PluginState
  effects: EffectGenerations
  dirty: bool
  should_quit: bool
```

The text document and line index stay in `anvil-editor`; `Model` holds shared
snapshots, not copies of document text.

### Msg

```text
Msg
  Terminal(TerminalEvent)
  Editor(EditorMsg)
  Command(EffectId, EffectResult)
  Gateway(GatewayMsg)
  Plugin(PluginMsg)
  Resize(TerminalSize)
  Quit
  Cancel(EffectId)
```

`TerminalEvent` covers key, paste, focus, and resize. Key release/repeat events
must not produce text input; only press events are actions.

### Cmd

```text
Cmd
  OpenFile { generation, path }
  SaveFile { generation, path, bytes }
  StartGateway
  GatewayRequest { generation, request }
  LoadPlugin { generation, id }
  ReloadPlugin { generation, id }
  CancelEffect(EffectId)
  RestoreTerminal
```

Every asynchronous command has an effect generation. Results carry the same
generation; stale results are ignored by `update`.

### Subscription

```text
Subscription
  Input
  Resize
  Paste
  Gateway
  Plugin
  Tick (off by default)
```

No periodic tick is enabled when the UI is idle. Redraw happens when `Model`
changes, not on a fixed frame loop.

## 5. Update rules

- `update` must never call `terminal.draw`, read files, spawn processes, or block.
- All state transitions happen inside `update`.
- Unhandled `Msg` returns the same model with no commands.
- Commands are described, not executed, by `update`.
- A command result with a stale generation is dropped.
- Quit closes subscriptions, cancels effects, restores the terminal, and then
  asks the session owner to stop the gateway.
- Gateway failure sets `Failed`; the TUI continues rendering and refuses `sys.*`
  actions with a visible status.

Pseudo-loop:

```text
render_if_dirty();
while let Some(message) = mailbox.recv() {
    let (next, commands) = update(model, message);
    model = next;
    render_if_dirty();
    for command in commands { runtime.dispatch(command); }
    if model.should_quit { break; }
}
```

## 6. Runtime and threads

- A dedicated terminal reader thread owns `crossterm::event::read`.
- A command runtime thread owns command dispatch and result collection.
- A gateway thread owns lazy start, readiness, and request/reply I/O.
- All threads send `Msg` into one bounded channel.
- Backpressure is explicit: if the mailbox is full, terminal events may be
  coalesced or rejected with a status message; no unbounded allocation.
- No thread calls `update` or `view` concurrently.

The main loop only processes messages and renders. A slow file save or gateway
call never stalls input.

## 7. Terminal lifecycle

- `TerminalGuard` enters raw mode, alternate screen, and keyboard enhancement
  where available.
- The guard restores the terminal on normal exit, error, and panic.
- A panic hook restores the guard state before the default panic output.
- Terminal capability detection is explicit; missing keyboard enhancement
  falls back to basic key events.
- The first frame is rendered before any gateway request.
- Resize invalidates layout and schedules one redraw.

## 8. View

Initial layout:

```text
┌──────────────────────────────────────────────┐
│ editor viewport                              │
│                                              │
│                                              │
├──────────────────────────────────────────────┤
│ status: mode · path · gateway · plugin       │
└──────────────────────────────────────────────┘
```

- Render only visible lines from `EditorView`.
- No full-document layout pass per frame.
- Overlays render after the editor and consume focus.
- The status line shows gateway state and degraded `sys.*` availability.
- The welcome screen is an overlay, not a second editor state.
- Color and style policies are centralized; views do not hardcode terminal
  colors.

## 9. Input and selection-first

- Key events become core `EditorCommand`/motion requests.
- The editor owns text, selection, viewport, and revision.
- Keymaps are future plugin data; phase 1 has a small built-in keymap only.
- A caret is an empty selection.
- A selection is visible only when anchor != head or an operator is active.
- Input events are replayed deterministically in tests.
- Paste is treated as an edit command, never as a sequence of key events.

## 10. Gateway startup

- `Model.gateway = NotStarted` initially.
- The first `sys.*` action or plugin handshake enqueues `Cmd::StartGateway`.
- `StartGateway` runs on the gateway thread and reports `Starting/Ready/Failed`.
- TUI startup never waits for the gateway; first frame has no gateway
  dependency.
- If the gateway is unavailable, `sys.*` actions are disabled but editing,
  selection, and rendering continue.

## 11. Errors and cancellation

- Terminal errors return an error after restoring the terminal.
- Command errors become `Msg::Command` with a typed error and status text.
- Cancellation is generation-based; no `Drop`-only cleanup.
- Quit cancels in-flight effects and waits for their shutdown where practical.
- No panic may leave raw mode, alternate screen, or a hidden cursor behind.
- No stale command result may overwrite newer editor state.

## 12. Testing

Pure tests:

- `update` table tests for every `Msg` family.
- Stale generation results are ignored.
- Quit stays quit.
- Gateway failure does not block editing.
- Resize updates layout state once.

Render tests with `TestBackend`:

- empty buffer;
- welcome overlay;
- viewport with selection;
- status line states;
- narrow terminal;
- long line/no-newline fixture;
- no full-document render in viewport mode.

Runtime tests:

- fake input reader and fake command runtime;
- input ordering;
- cancellation and shutdown;
- terminal guard restoration on error/panic;
- no idle redraw.

Manual smoke:

- real TTY start, resize, quit, Ctrl-C, terminal restoration;
- startup with gateway unavailable;
- first-frame timing.

## 13. Performance gates

Reference machine, release build:

- first frame without gateway: <= 10 ms;
- event-to-render p99: <= 16 ms for ordinary input;
- scroll p99: <= 33 ms for 100 MB document;
- no periodic redraw when idle;
- no full-document layout in `view`;
- no synchronous file/gateway I/O in `update`.

Record p50/p99 and allocator numbers; do not claim end-to-end latency from a
single frame.

## 14. Delivery sequence

### TUI-0: crate and terminal skeleton

- Add `crates/anvil-tui` with `ratatui`/`crossterm`.
- Terminal guard, raw/alternate lifecycle, panic restore.
- Initial `Model = welcome`, `Msg::Quit`, single redraw.
- Tests with `TestBackend`.

### TUI-1: pure Elm core

- Add `Msg`, `Cmd`, `update`, `Subscriptions`.
- Add deterministic update tests.
- Idle loop does not tick or redraw.

### TUI-2: viewport and status

- Render `EditorView` viewport.
- Add status line and welcome overlay.
- Width/height/resize handling.
- Golden frame tests.

### TUI-3: input and editing

- Key reader thread, press-only dispatch.
- Built-in selection-first motions and insertion.
- Paste path.
- No text mutation outside editor commands.

### TUI-4: background effects

- Command runtime thread.
- Open/save effects with generation cancellation.
- Stale result tests.
- Error status.

### TUI-5: gateway lifecycle

- Lazy `StartGateway`.
- Ready/Failed states.
- `sys.*` actions gated by gateway state.
- TUI remains usable when gateway fails.

### TUI-6: plugin events and hot reload

- Depends on plugin-host runtime/control registration.
- Plugin state and event subscription.
- Reload status; no TUI block on host restart.

## 15. Acceptance criteria

- TUI renders before gateway startup and remains usable with no gateway.
- `update` is pure and has no I/O or terminal calls.
- Every effect is cancellable or generation-guarded.
- Terminal restoration is guaranteed on normal exit, error, and panic.
- No idle redraw loop.
- Viewport render does not scan the full document.
- Input, resize, and background results preserve ordering and never block the
  main loop.
- `cargo test -p anvil-tui`, `cargo clippy`, and `cargo fmt --check` pass.
- Performance smoke meets the reference first-frame and p99 gates, or the
  report states the measured failure.

## 16. Open questions

- Whether `anvil-editor` exposes a session type suitable for `Model`, or
  `anvil-tui` owns a thin session wrapper.
- Whether the built-in phase-1 keymap lives in Rust or a small data table until
  plugin keymaps load.
- Whether file open runs in one worker or a bounded pool; initial plan uses the
  command runtime with one active open and generation cancellation.
- How gateway readiness is surfaced to the status line before control-socket
  registration exists; phase 1 uses process start/exit and socket existence.

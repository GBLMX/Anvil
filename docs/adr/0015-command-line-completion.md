# ADR-0015: Command-Line Completion and Popup

- Status: Proposed; planning only, no implementation in this change
- Date: 2026-11-01
- Related: [ADR-0003](0003-idl.md), [ADR-0008](0008-selection-first-interaction.md),
  [ADR-0014](0014-plugin-wit-seams.md),
  [High-frequency command plan](../command-architecture-plan.md)

## Context

Anvil's command mode is currently a plain input buffer:

- `InputMode::CommandLine { buffer }` stores the text.
- Enter trims and dispatches it through the string matcher in
  `crates/anvil-tui/src/commands.rs`.
- Tab, Shift-Tab, arrow selection, and a completion popup are not handled.
- There is no command registry, no completion contract in `anvil-api`, and no
  completion state in the editor.

The editor should eventually support `:` command completion for self-hosting:
command names, paths, and buffers, with a keyboard-driven popup. This needs to
be added without blocking the input loop or creating a second command layer.

## Decision

### 1. Ship a TUI MVP before an abstract plugin API

Phase 1 is deliberately TUI-local:

- completion state lives beside `InputMode::CommandLine` in `anvil-tui`;
- the first data sources are command names, workspace paths, and open buffers;
- rendering uses `ratatui` above the command/status line;
- no Lua, JSON-RPC, or plugin host is involved.

A command registry is not required for the first visible result. The MVP may
start from a small sourced list, but the list must not become a second
permanent command table; Phase 2 must derive it from the command registry plan
in `docs/command-architecture-plan.md`.

### 2. Interaction contract

When command-line completion is open:

- `Tab` moves to the next candidate and wraps at the end.
- `Shift-Tab` moves to the previous candidate and wraps at the start.
- `Enter` accepts the selected candidate and executes the resulting command.
- `Esc` closes the completion popup without leaving command mode.
- A second `Esc`, with no popup open, cancels command mode.
- `Backspace` edits the active token and recomputes candidates.
- Candidate selection is reset or clamped after every edit.
- Tab never executes a command by itself.
- Completion never mutates buffer text; it only edits the command-line buffer.

Popup behavior:

- Render above the command/status line, not over it.
- Bound the height; scroll the candidate list if necessary.
- Truncate or horizontally scroll long candidates without wrapping into the
  command line.
- Preserve terminal display width for Unicode and wide characters.
- Strip control characters from candidate labels.

### 3. Candidate sources

Phase 1 sources:

1. **Command names**
   - Complete the first whitespace-delimited token.
   - Support aliases where the command registry defines them.
   - `:write` must be an accepted alias before the `:wr -> :write` example is
     advertised; today the implemented spellings are `:w`, `:wa`, and `:wq`.
2. **Workspace paths**
   - Complete arguments of commands that accept a file path (`:e`, `:w`,
     `:wq`, and future equivalents).
   - Complete one path component at a time.
   - Directory candidates append `/`.
   - Resolve through the same workspace boundary used by file reading.
   - Never expose paths outside the workspace, including via `..` or symlinks.
3. **Open buffers**
   - Complete buffer arguments for `:b`/`buffer` once that command exists.
   - Use the displayed buffer name; resolve to the canonical buffer path.
   - Do not duplicate the active buffer in the candidate list.

Path completion must use the gateway/workspace reader, not an ambient
`std::fs` call in the TUI. It must be bounded and must not block the key path.

### 4. State ownership

The MVP completion state belongs in `anvil-tui`, not `anvil-editor`.

`anvil-editor` is the selection-first editing core. It has no
`Mode::Command`, no command-line buffer, and no UI focus state. Adding a command
mode there would mix UI input state into the core and conflict with ADR-0008.

If a future headless session or plugin host needs to drive command completion,
move the state to a session/command state machine, not into `anvil-editor`.

### 5. API contract, without blocking the MVP

Phase 2 adds serializable contracts, but does **not** put an editor-dependent
trait in `anvil-api`:

- `CompletionItem`, `CompletionRequest`, and `CompletionResponse` DTOs belong
  in `anvil-api` because the plugin boundary needs cross-language data.
- The `Completer` execution trait belongs in the kernel/session completion
  module. A plugin completer is asynchronous IPC, not a synchronous trait call.
- Built-in Command/Path/Buffer completers implement the same kernel interface.
- `anvil-tui` renders results; it does not own completion policy.

This deliberately differs from the source checklist's "define a `Completer`
trait in `anvil-api`". `anvil-api` is documented as not depending on the
editor, TUI, or platform; putting the host trait there would add that coupling
and would not model asynchronous plugin completion.

Suggested logical shape:

```text
CompletionRequest
  query: string
  cursor: byte offset in command buffer
  context: command | path | buffer
  workspace_revision: u64

CompletionItem
  label: string
  replacement: string
  detail: optional string
  kind: command | file | directory | buffer | plugin
  source: builtin | plugin id

CompletionResponse
  request_id: u64
  revision: u64
  items: list<completion item>
  truncated: bool
```

Plugin completion is cancellable, deadline-bound, and bounded. A stale
response must never replace candidates for a newer command-line query.

### 6. Matching and ranking

- Phase 1: case-insensitive prefix matching on the active token.
- Existing path fuzzy matching may be reused if it is already present; do not
  add `nucleo`/`fuzzy-matcher` just for the MVP.
- Phase 3 may introduce fuzzy matching only after a latency measurement.
- Ranking is stable and deterministic: exact prefix, command/source priority,
  then lexical order.
- Candidate count is capped; the UI displays a truncation indicator.

### 7. Plugin completion

Plugin completers are an additive Phase 3 feature:

- plugins declare or register a completion provider through the plugin
  contract;
- the kernel forwards a bounded `CompletionRequest` over IPC;
- the plugin returns `CompletionItem`s through the plugin host;
- capability scope controls access to workspace metadata;
- results are asynchronous, cancellable, and revision-checked;
- core completion remains usable when a plugin is slow, crashed, or disabled.

No plugin can intercept the key path or execute arbitrary code on Tab.

## Implementation Plan

### Phase 1: TUI MVP

- [ ] Add `CompletionState` beside `InputMode::CommandLine`.
- [ ] Add command-name completion for the currently implemented command list.
- [ ] Add workspace path completion through the existing workspace reader.
- [ ] Add open-buffer completion only when the `:b` command exists.
- [ ] Render a bounded `ratatui` list above the command line.
- [ ] Wire Tab, Shift-Tab, Enter, Esc, and Backspace.
- [ ] Add tests for cycling, wrapping, editing, cancellation, and execution.
- [ ] Verify no candidate triggers a command side effect.

Exit criteria: `:` completion works for command names and paths, is usable in
an 80×24 terminal, and never blocks the input loop.

### Phase 2: Kernel contract

- [ ] Add `CompletionItem`, request/response DTOs to `anvil-api`.
- [ ] Add the kernel `Completer` interface outside `anvil-api`.
- [ ] Implement Command/Path/Buffer completers against one interface.
- [ ] Make TUI command completion consume the kernel result.
- [ ] Add stale-request and revision tests.
- [ ] Derive command names from the command registry plan.

Exit criteria: TUI is a renderer; completion policy is testable without the
TUI.

### Phase 3: Polish and plugins

- [ ] Evaluate fuzzy matching with measurements; add a dependency only if
  needed.
- [ ] Add descriptions, file/directory kind, and plugin detail columns.
- [ ] Add bounded asynchronous plugin completion.
- [ ] Add cancellation, timeout, and plugin-disabled fallback tests.

Exit criteria: plugin completion cannot make command mode hang or execute
commands; core completion remains available without plugins.

## Verification

- Command prefix changes candidates without changing buffer text.
- Tab/Shift-Tab wrap in both directions.
- Enter executes exactly the selected replacement once.
- Esc closes only the popup first; a second Esc cancels command mode.
- Backspace updates the query and clamps selection.
- Path completion cannot escape the workspace, including symlinks and `..`.
- Path candidates do not block the input loop.
- Popup content is bounded and does not overwrite the command line.
- Unicode/wide labels do not corrupt terminal columns.
- Stale plugin/LSP completions are discarded.
- Existing command behavior remains unchanged when completion is closed.

## Alternatives considered

### Put completion in `anvil-editor` immediately

Rejected. Command-line input and popup focus are UI/session state; the editor
core remains selection-first and has no `Mode::Command`.

### Define `Completer` directly in `anvil-api`

Rejected for Phase 2. `anvil-api` is a serializable, editor-independent
contract; the execution trait belongs in the kernel/session. DTOs remain in
`anvil-api`.

### Auto-open on every keystroke

Deferred. Explicit Tab is predictable and avoids querying paths or plugins on
every character. Auto-completion can be measured later.

### Add fuzzy matching and plugin completion in Phase 1

Rejected. It adds dependencies and IPC failure modes before the basic popup and
key behavior are proven.

## Consequences

Positive:

- A visible self-hosting improvement with low architectural risk.
- Completion can be tested without Lua or a plugin host.
- The DTO/interface split preserves `anvil-api`'s dependency boundary.
- Path completion reuses the existing workspace security boundary.

Negative:

- Phase 1 duplicates a small command list until the registry exists.
- Completion state starts in the TUI and may need a later session-owned move.
- Plugin completion remains deferred.
- Fuzzy ranking and richer detail are later work.

## Open questions

- Should command aliases be canonicalized before completion or after Enter?
- Should path completion quote or escape spaces, or reject whitespace paths in
  the MVP?
- Is `:b` part of the first command set, or should buffer completion wait?
- Should the popup move to a dedicated overlay layer if help and completion can
  coexist?
- Which capability name governs plugin completion metadata access?

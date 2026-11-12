# ADR-0024: TUI State Composition and Domain Boundaries

- Status: Proposed
- Date: 2026-11-12
- Related: [ADR-0013](0013-kernel-distribution-boundary.md), [ADR-0016](0016-render-style-layers.md), [ADR-0022](0022-core-editing-completion.md), [ADR-0023](0023-internal-capability-closure.md)

## Context

The process model and crate split are already useful: `anvil-core`,
`anvil-editor`, `anvil-session`, `anvil-tui`, `anvil-ipc`, `anvil-control`, and
the gateway/plugin host have real boundaries.

The problem is inside the editor frontend. `anvil-tui::Model` currently has:

- 198 fields across a 337-line struct;
- document buffers, cursor/viewport, split/tree state, LSP workers and
  diagnostics, Git state, tasks, PTY state, debug/DAP state, plugin state,
  settings, themes, overlays, recents, and runtime channels;
- dozens of `impl Model` modules that all receive `&mut Model`.

That works for a single TUI, but it makes the next features expensive:

- each new page reads and mutates unrelated state;
- a page cannot be tested from a small state object;
- a second frontend cannot reuse the session/page logic without dragging the TUI
  model with it;
- ownership of background work is spread across fields;
- bounds, cancellation, and failure paths are easy to miss.

The goal is not a larger number of crates. The goal is fewer reasons for every
module to depend on every field.

## Decision

### 1. Keep the process and crate model

Do not rewrite the gateway, control plane, plugin host, or text engine for this
change. Do not split `anvil-tui` into many crates now.

The first refactor is internal state composition. Crate extraction follows only
when a second consumer exists.

### 2. Compose `Model` from owned state groups

Target shape:

```rust
pub struct Model {
    pub documents: DocumentState,
    pub workspace: WorkspaceState,
    pub language: LanguageState,
    pub runtime: RuntimeState,
    pub overlays: OverlayState,
}
```

Ownership rules:

| State | Owns | Must not own |
|---|---|---|
| `DocumentState` | buffers, active index, split, cursor, selections, viewport, undo/redo, dirty/missing flags | LSP workers, gateway, tree watchers |
| `WorkspaceState` | roots, path policy, tree, file workers, git status/gutter, recent files/workspaces | editor text, rendering |
| `LanguageState` | LSP workers/requests, diagnostics, completion, symbols, code lens, inlay hints, DAP request state | crossterm input, ratatui widgets |
| `RuntimeState` | gateway session, task worker, PTY, plugin supervisor/manager, control queue, busy/progress | document projection, page widgets |
| `OverlayState` | settings, theme browser, command palette, fuzzy, problems, tests, debug panel, notification | document mutation policy |

The groups are structs with methods and tests, not a second layer of public
fields. Existing modules keep behavior while their `impl Model` moves to the
relevant group where practical.

### 3. Separate page state from renderers

Every real page gets a small state object and a renderer that reads it:

```text
StartPageState
SettingsPageState
ThemePageState
CommandPaletteState
ProblemsPageState
DebugPageState
TestPageState
```

Rules:

- A page state does not hold `&mut Model`.
- Page transitions are commands/effects, not direct mutation from render code.
- Renderers are pure reads of the page state plus a small environment (theme,
  terminal size).
- Page tests construct the page state without a gateway, PTY, or filesystem.

This is the prerequisite for a future GUI/web frontend. It is also useful for
the TUI alone: settings, themes, and the command palette can be tested without
starting the full editor.

### 4. Keep one event -> command -> effect path

Use the existing `anvil-session` command and effect vocabulary. Extend it rather
than adding feature-specific request channels.

```text
input / worker event / control request
        -> command
        -> effect
        -> runtime worker or domain operation
        -> event
        -> state update
```

Requirements:

- Every background operation has an owner, a request id, cancellation, and a
  bounded queue.
- No page renderer starts I/O.
- No `Drop` implementation is the only cleanup path for a process, socket, or
  watcher.
- A response for a closed page or stale revision is dropped explicitly.

Do not introduce an async runtime merely for modernization. The current worker
+ channel model is sufficient until a measured case requires otherwise.

### 5. Extract crates only on evidence

A module is eligible for a crate when at least one is true:

1. a non-TUI consumer exists (CLI, MCP, GUI, headless app);
2. the module has no UI types and can be tested independently;
3. extraction removes a dependency direction that is currently wrong;
4. a measured build/ownership problem exists.

**UI-neutral token rule**

`theme.rs` and `keymap.rs` are UI adapters today:

- `theme.rs` exposes `ratatui::{Color, Style, Modifier}`;
- `keymap.rs` uses crossterm key types for dispatch.

They are therefore **not** eligible to move into a UI-neutral config crate
as-is. A future `anvil-config` may own only:

- settings schema, defaults, validation, path resolution, file/env precedence,
  and atomic save;
- theme choice (`fixed`/`auto`) and serialized palette tokens as plain RGB /
  modifier enums;
- keymap file syntax and binding tokens as plain key-code / modifier enums;
- no `ratatui`, `crossterm`, terminal rendering, or event-loop behavior.

The TUI keeps frontend adapters that convert neutral tokens into
`ratatui::Style` and crossterm `KeyEvent`. Until that layer exists, theme
rendering and key dispatch stay in `anvil-tui`.

Rejected for this ADR: extracting `theme.rs` and `keymap.rs` unchanged into
`anvil-config`. That would move UI dependencies rather than remove them.

Candidate order:

| Candidate | Extract when |
|---|---|
| `anvil-config` | settings schema/serialization and neutral palette/key tokens need a non-TUI consumer; no ratatui/crossterm types |
| `anvil-git` | diff/blame/conflict/hunk logic is reused outside the TUI |
| `anvil-lsp-client` | transport/request routing is needed by another frontend |
| `anvil-task` | task runner/problem matcher is reused by MCP or headless mode |
| `anvil-terminal` | only if a non-TUI consumer needs the PTY session |
| `anvil-ui` | only when a second frontend needs shared view models |
| `anvil-app` | only when composition duplication is measured across frontends |

Do not create `anvil-app` now. The TUI remains the composition root for the
current product.

### 6. Enforce dependency direction

Target dependency direction:

```text
anvil-api / anvil-core
        |
    anvil-editor
        |
   anvil-session
        |
     anvil-tui

anvil-config (no ratatui/crossterm)  anvil-git  anvil-lsp-client
                  \                     |        /
                              anvil-tui
```

Rules:

- `anvil-core` has no UI, process, network, or config dependency.
- `anvil-editor` has no rendering, crossterm, gateway, or settings dependency.
- `anvil-session` has no ratatui/crossterm dependency and no renderer widgets.
- `anvil-tui` is the only crate that knows terminal input/rendering.
- `anvil-config` has no `ratatui` or `crossterm` dependency; the TUI owns the
  adapters from neutral palette/key tokens to those types.
- Domain crates receive data and return data; they do not read the TUI model.
- The gateway remains the file/network/process policy point. A refactor must not
  add direct ambient I/O to a page.

Add an automated dependency check for these rules. File size alone is not a
failure; a wrong dependency edge is.

### 7. Migration order

1. Inventory fields and move overlay/page state first.
2. Split `LanguageState` and its request/cancellation handling.
3. Split `WorkspaceState`, including tree/file-worker ownership.
4. Split `RuntimeState` and make worker ownership explicit.
5. Reduce `Model` to composition and cross-state transitions.
6. Define neutral palette/key tokens; keep theme rendering and key dispatch in
   the TUI.
7. Extract `anvil-config` for settings schema/serialization and neutral tokens
   only after page settings are frontend-neutral.
8. Extract `anvil-git` when a second consumer appears.
9. Extract `anvil-lsp-client` when transport is reused.
10. Add dependency checks and update architecture docs.

Each step must be behavior-preserving with existing tests; do not combine a
state split with a feature change.

## Non-goals

- A second async runtime.
- A plugin UI WebView.
- Rewriting the text engine.
- Splitting every large module into a crate.
- A public plugin/page API before the internal page state is stable.
- Preserving a field layout that makes tests depend on `Model` internals.

## Consequences

Positive:

- New pages touch one page state instead of the whole editor model.
- LSP/DAP/task/PTY ownership becomes explicit.
- A future frontend can reuse session/domain crates without ratatui.
- Tests become smaller and faster.
- Crate extraction becomes a measured decision, not a layout aesthetic.

Negative:

- There is an upfront refactor with borrow-checker churn.
- Some existing tests must be moved from `Model` setup to state setup.
- The first split does not add a user-visible feature.

The refactor is justified because the current `Model` already owns the next
three feature areas (pages, language services, runtime workers). Waiting until
a second frontend exists would make the first extraction much more expensive.

## Implementation status

Phase 1 started on 2026-11-12:

- `Model.overlays` owns `command_palette`, `settings_overlay`,
  `theme_browser`, `fuzzy`, `preview`, `debug_panel`, `test_explorer`,
  `command_completion`, command-palette recents, the pipe-location panel, and
  LSP presentation results (`lsp_results`), including sticky symbols, folds,
  inlay hints, references, and code actions.
- `Model.git` owns gutter marks, status, diff, apply, blame, conflict, stage,
  resolve, request generations, and branch state.
- `Model.tree` owns entries, selection, visibility, hidden-file mode, watch
  limits, and refresh generations.
- `Model.recent` owns recent workspaces/files, quick-open frecency, and the
  persistence switch.
- `Model.workspace` owns the root path, workspace roots, primary and per-root
  gateways, gateway requirement, file workers, conflict swaps, and stale paths.
- `Model.runtime` owns task workers/results and terminal/PTY sessions and
  output.
- `Model.project` owns project-search generations, worker accounting,
  accumulated results, and reviewed replace plans.
- `Model.buffers` owns open buffers, the active buffer, split pane, and
  read-only help-page buffer/title.
- `Model.appearance` owns icon overrides, ASCII fallback mode, icon mtime
  checks, and theme persistence.
- `Model.view` owns scroll position, viewport dimensions, split ratio, pointer
  state, and soft-wrap projection state.
- `Model.plugins` owns discovered plugin entries, the host supervisor, manager
  selection, plugin-store size, installer transport, and package install state.
- `Model.search` owns the current substring query and its match, option, and
  origin-restore state.
- `Model.input` owns pending operators, leader deadlines, counts, register and
  macro prompts, recording buffers, replay state, and inner-object state.
- `Model.edit` owns visual-selection anchors, the modal layer, the last
  semantic change, and the current insert session.
- `Model.session` owns the editor document, optional source path, cursor, and
  jump/change history.
- `Model.status` owns status text, the busy label, progress, and spinner.
- `Model.config` owns settings, in-memory keymap overrides, and typed command
  effects.
- `Model.language` owns `DapState`, `DiagnosticsState`, `LspTransportState`,
  and `LspRequestsState`. Transport keeps clients, workers, config, retries,
  and retry deadlines; requests keep pending maps, document versions, request
  ids, and format-on-save coordination.
- Existing behavior is unchanged; tests access the composed state explicitly.
- `Model` now has 31 top-level fields. Remaining file flags and mode fields
  move in later behavior-preserving slices.


## Acceptance

- `Model` contains state groups, not feature fields.
- A page can be rendered and tested without a gateway, PTY, or LSP worker.
- Every background owner has bounded queue, cancellation, and stale-response
  behavior.
- Dependency direction is checked automatically.
- `anvil-config`, if extracted, has no `ratatui` or `crossterm` dependency.
- Full workspace tests, clippy, formatting, and architecture checks pass.
- No user-visible behavior change is bundled into the state split.

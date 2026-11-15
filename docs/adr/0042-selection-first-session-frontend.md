# ADR-0042: Selection-First Core, Headless Session, Thin Frontend

- Status: Accepted
- Implementation: direction accepted; EditorState/session extraction continues
  under ADR-0041
- Date: 2026-11-15
- Related: [ADR-0008](0008-selection-first-interaction.md),
  [ADR-0009](0009-text-model-snapshot-gate.md),
  [ADR-0013](0013-kernel-distribution-boundary.md),
  [ADR-0024](0024-tui-state-and-domain-boundaries.md),
  [ADR-0037](0037-kernel-api-baseline.md),
  [ADR-0040](0040-helix-keymap-compatibility.md),
  [ADR-0041](0041-tui-primitive-extraction.md)

## Context

Anvil already has the right kernel direction but the boundary is still uneven:

- `anvil-core` owns text and coordinate primitives;
- `anvil-editor` owns `EditorState`, snapshots, selections, undo, and
  transactions;
- `anvil-session` exposes a frontend-neutral `DocumentSession` and effect
  vocabulary;
- `anvil-tui` remains both the frontend adapter and the application shell, at
  roughly 47k source lines;
- LSP, DAP, tasks, Git, workspace routing, and plugins are external services
  reached through the gateway/adapters.

Helix is a useful comparison because it also uses a selection-first core, but
its `helix-view::Document` combines text, syntax, history, LSP, and per-view
selections in one imperative object. Copying that shape would violate Anvil's
kernel boundary: `anvil-editor` must remain free of LSP, configuration, and
platform dependencies.

The question is therefore not "Helix or not". It is which parts of Helix's
architecture should Anvil adopt, and where Anvil must deliberately differ.

This ADR defines the target boundary. It does not claim that all orchestration
has already moved: some LSP, DAP, task, Git, and workspace behavior still lives
in `anvil-tui`. Those domains move only when they satisfy the extraction gate in
ADR-0041; until then the TUI remains the application shell.

## Decision

### 1. Runtime layers and service adapters

Anvil has a functional editor core, a headless session boundary, and a
frontend adapter. `anvil-api` and `anvil-ipc` are contracts across those
boundaries:

```text
anvil-tui (input adapter and renderer)
    -> anvil-session (document state, effects, policy)
        -> anvil-editor (EditorState, selection, transactions, snapshots)
            -> anvil-core (TextDocument, LineIndex, History)

anvil-tui or a future anvil-services layer
    -> anvil-lsp-client / gateway / task / DAP adapters
        -> external processes
```

`anvil-session` produces typed effects and consumes typed results. It owns
orchestration state, generation checks, and policy, but it does not own
transport clients. Concrete LSP/DAP/task/gateway adapters stay outside the
kernel baseline; they live in the frontend today and may move to a dedicated
service crate when a second consumer justifies it.

### 2. `anvil-core` stays a text model

`anvil-core` keeps `TextBuffer`, `TextDocument`, `LineIndex`, coordinate
conversion, edit commands, and history primitives.

It must not depend on LSP, configuration, UI, platform services, or plugin
types.

The production text backend remains the ADR-0009 PieceTable choice. Anvil does
not switch to Rope merely because Helix uses `ropey`; a backend change requires
new benchmark evidence.

### 3. `anvil-editor` owns selection and transactions

`anvil-editor::EditorState` is the mutable editor state; snapshots are cheap
projections over shared text/index data.

The selection model is native and selection-first:

- `Selection { anchor, head }` is the public representation;
- a caret is an empty selection where `anchor == head`;
- motions change selections;
- actions consume selections;
- multi-selection is the same primitive, not a separate cursor mode;
- edits are expressed as transactions and selections are mapped through the
  change;
- snapshots carry a revision so stale edits cannot silently apply.

`anvil-editor` must not own LSP sessions, gateway clients, file dialogs, or
terminal input state.

### 4. `anvil-session` owns orchestration state, not clients

`anvil-session` is the headless application boundary. It owns:

- document sessions and their lifecycle;
- command and effect vocabulary;
- workspace/root policy;
- LSP/DAP/task state, worker-result contracts, and retry policy;
- plugin event and provider seams;
- stale-generation and cancellation rules.

It must not depend on `anvil-tui`, ratatui, crossterm, or rendering state.
It also must not depend on concrete `anvil-lsp-client`, task-runner, DAP, or
`anvil_ipc::GatewayClient` implementations; those are adapter services behind
the session's effect/result contracts. The `anvil-ipc` wire codec and DTOs are
allowed because they are protocol contracts, not transport ownership. This
keeps the ADR-0037 kernel baseline from growing new transport dependencies.

Migration debt: `plugin_session.rs` and `plugin_supervisor.rs` currently use
`anvil_ipc::GatewayClient` directly, and `host_session.rs` uses
`anvil_plugin_host::{spawn, HostProcess}`. Both dependencies are temporary;
they must be replaced by session-owned ports or moved behind the service
layer before the session API is frozen.

`DocumentSession` is a frontend-neutral wrapper around `EditorState`; it is not
a second editor kernel.

### 5. `anvil-tui` is an adapter

`anvil-tui` owns:

- key translation and pending-key UI state;
- layout, rendering, overlays, focus, mouse, and terminal I/O;
- TUI-only presentation state.

It must not become a second source of truth for:

- text edits or undo;
- selection semantics;
- document save/open policy;
- LSP lifecycle or diagnostics generation rules.

TUI `Model` domains are allowed to exist, but domain logic that has no render or
input dependency should move to `anvil-session` or a support crate when it has a
clean boundary and a testable contract.

### 6. Helix patterns to adopt

Adopt:

- selection-first command semantics;
- anchor/head selections and empty-caret equivalence;
- transactional edits with selection mapping and invertible undo;
- document/view separation, so one document can be shown in multiple views;
- cancellable/debounced background work for LSP and other services.

### 7. Helix patterns to reject

Do not copy:

- `helix-view::Document` as a single object containing Rope, Syntax, History,
  and LSP;
- Helix's `Compositor`/`Component` tree as Anvil's UI architecture;
- a Rope backend without benchmark evidence;
- a global event bus or a second async runtime;
- a full TUI rewrite in the name of architectural purity.

Anvil keeps ratatui and the existing typed-effect model.

## Boundaries and gates

### Dependency direction

Allowed:

```text
distribution -> anvil-tui
distribution -> modules -> kernel crates
anvil-tui -> anvil-session
anvil-tui -> anvil-lsp-client / anvil-task / gateway transport / DAP adapter
anvil-session -> anvil-editor / anvil-core
anvil-editor -> anvil-core
anvil-session -> anvil-api / anvil-ipc wire contracts / module service contracts
future anvil-services -> concrete LSP / task / DAP / gateway adapters
```

Forbidden:

```text
anvil-core -> anvil-editor
anvil-editor -> anvil-session
anvil-editor -> anvil-tui
anvil-session -> anvil-tui
anvil-session -> anvil-lsp-client / anvil-task / DAP transport / anvil_ipc::GatewayClient
anvil-session -> anvil-plugin-host implementation types
kernel crate -> anvil-tui
module -> anvil-tui
```

The concrete adapters listed under `anvil-tui` are its current implementation
of the session's effect/result contracts. They are not part of the kernel
baseline and may move to `anvil-services` without changing `anvil-session`.

### State ownership

- `anvil-core`: text and coordinates.
- `anvil-editor`: selection, edits, undo, snapshots.
- `anvil-session`: document/workspace/policy/effect/result state; no transports.
- `anvil-tui`: input, presentation, and current adapter execution state.

A state field has exactly one owner. Re-exports and snapshots are contracts, not
a second writable source of truth.

### Staleness and cancellation

- Every asynchronous result that can change editor state carries a revision,
  path, request id, or generation.
- Stale results are dropped, not applied and then repaired.
- LSP/worker shutdown has a bounded budget.
- A frontend must not block the event loop waiting for a service.

### Extraction gate

A module may leave `anvil-tui` only when:

1. it has no dependency on `Model`, ratatui, or terminal application state;
2. its ownership boundary can be stated in one sentence;
3. it has tests for failure, cancellation, and stale results where applicable;
4. it does not create a second editing or document model;
5. its public surface is justified by a real consumer or a frozen contract.

This is the ADR-0041 extraction rule, retained here as an architectural gate.

## Consequences

Positive:

- The kernel remains testable without a terminal, LSP server, or platform
  service.
- LSP, DAP, task, and plugin failures cannot corrupt editor text state.
- Multiple frontends can reuse the session contract.
- Helix-compatible interaction can evolve without copying Helix internals.
- The existing PieceTable and ratatui investments remain valid.

Negative:

- `anvil-tui` will remain large until non-rendering domains are extracted
  incrementally.
- Maintaining explicit snapshot/effect boundaries costs more code than direct
  calls.
- Some Helix UI conveniences will not be available without reinventing them on
  ratatui.
- The architecture is intentionally conservative about new crates and async
  runtimes.

## Non-goals

- A full Helix clone.
- Replacing PieceTable with Rope.
- Moving LSP into `anvil-editor`.
- Adopting GPUI, WebView, or an embedded browser.
- Persistent undo; ADR-0026 still governs that decision.
- Workspace-wide pull diagnostics; the current target servers do not advertise
  `workspaceDiagnostics`.

## Verification

- `cargo test --workspace --no-fail-fast`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
- `scripts/architecture-check.sh`
- `git diff --check`
- Real-TUI `scripts/ctl-smoke.sh`, `scripts/provider-smoke.sh`, and
  `scripts/mcp-smoke.sh`
- Real LSP switching smoke via `scripts/lsp-real-smoke.sh`

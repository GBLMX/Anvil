# High-Frequency Command Architecture Plan

Status: planning only. No command registry, keymap trie, effect queue, Lua
keymap host, or command capability matrix is implemented by this document.

## Goal

Make Anvil's high-frequency editor actions fast, asynchronous where needed,
and uniformly authorized without putting Lua on the keystroke path.

The first slice is **not** `make`. Build commands are lower-frequency and
higher-risk. The architecture should be proven on commands users invoke many
times per minute:

- `editor.undo`, `editor.redo`
- `search.next`
- `view.split`, `buffer.next`
- `lsp.completion`, `lsp.hover`, `lsp.definition`
- `editor.save`
- `lsp.rename`, `lsp.code_action`, `lsp.format`

## Non-goals

- No Lua callback per key event.
- No Lua direct mutation of the editor buffer.
- No Go gateway round-trip for pure editor commands.
- No `proc.spawn` blanket capability.
- No `fs.write` implied by applying a workspace edit.
- No make/build implementation in the first slice.
- No replacement of the existing text model or selection model.

## Current State

- `crates/anvil-tui/src/commands.rs` dispatches Ex commands with string
  matching; it is not a command registry.
- `crates/anvil-tui/src/keys.rs` resolves keys through hardcoded `pending`
  state and `match` arms; there is no trie.
- `crates/anvil-tui/src/lsp_ui.rs` calls LSP operations directly; they are not
  command IDs or effects.
- `crates/anvil-tui/src/workspace_edit.rs` applies edits to buffers and may read
  unopened files, but never writes files.
- `crates/anvil-plugin-host` has no Lua/Luau VM yet.
- `anvil-api` has `editor.register_command` as a method name but no command
  descriptor or effect DTO.
- The Go gateway owns `sys.*`, `fs.*`, and `lsp.*`; the session currently
  starts with `-allow-read -allow-write` and a trusted, unsandboxed LSP path.

This is a new kernel slice, not a small TUI refactor.

## Architecture

```text
key event
  -> Rust keymap trie
  -> CommandId
  -> CommandDispatcher
  -> local capability check
  -> native execution OR bounded effect queue
  -> editor effects or gateway effects
  -> result/event with request id and revision
```

Lua appears only in two places:

1. startup declarations: keymap, aliases, command descriptors;
2. custom command implementations that submit effects.

Lua never appears in the per-key resolution loop.

## 1. CommandRegistry

Place the registry in a future `anvil-session`/`anvil-command` boundary, not
`anvil-core`. `anvil-core::EditCommand` is a text/undo primitive, not a UI
command.

Each command descriptor should include:

```text
CommandId          stable namespaced ID
Aliases            :w, :wa, etc.
ArgsSchema         command-line arguments
ExecutorKind       Native | Plugin
Capabilities       editor.write, fs.read, fs.write, sys.exec, ...
Contexts           select, insert, cmdline, overlay
Priority           interactive | background
Concurrency        immediate | single-flight | cancellable
Result             status, events, error
```

Rules:

- `:w` and `Space` menu entries reference the same canonical command ID.
- Plugin commands are namespaced; they cannot override core command IDs.
- Built-in and plugin commands share the same descriptor shape.
- The registry exposes a bounded metadata query for command palette/menu; the
  key path stores an index, not a string lookup.

## 2. Keymap trie

- One trie per input context: select, insert, cmdline, overlay.
- `Space` is a prefix node, not a special branch in `keys.rs`.
- Trie leaves store a command index or `CommandId`.
- Key resolution is allocation-free on the hit path, lock-free, and never
  calls Lua or JSON-RPC.
- Trie snapshots are immutable and swapped atomically after config reload.
- Prefix timeouts, counts, and operator-pending state stay in the TUI adapter
  or keymap state; they are not command implementations.
- The `Space` menu is generated from the registry/trie, not a second hardcoded
  list.

## 3. Lua/Luau configuration

Lua runs outside the keystroke path and may only declare:

```lua
anvil.keymap.set("select", "<Space>f", "picker.open")
anvil.keymap.set("select", "<Space>r", "lsp.rename")
anvil.keymap.set("select", "K", "lsp.hover")
anvil.keymap.set("insert", "<C-Space>", "lsp.completion")
```

A Lua custom command submits effects:

```text
Lua handler
  -> ctx:submit_effect(effect)
  -> request id / status
```

It cannot set `EditorState` directly. All editor changes go through the
dispatcher and its revision checks.

Startup configuration is atomic: parse, validate, resolve collisions, build
trie, then swap. A bad mapping must not partially replace the previous map.

## 4. Effect queue

Effects are typed values, not arbitrary callbacks:

- `editor.apply_edits`
- `editor.set_selection`
- `ui.notify`
- `lsp.request`
- `fs.read`
- `fs.write`
- `sys.exec`
- `sys.fetch`

Every invocation carries a request id, command id, source/plugin, deadline,
cancellation token, and expected revision where applicable.

Queue rules:

- Pure editor commands execute immediately and do not queue.
- LSP completion/hover are single-flight; a newer request cancels or replaces
  the older one.
- Save is single-flight per buffer; a later save request coalesces with the
  pending one.
- Bounded queues; explicit overflow behavior; interactive work is not starved
  by background work.
- Stale results are dropped by path, request id, and revision.
- Lua command results contain effects; they are authorized before being
  applied.

## 5. Authorization

Authorization has two layers:

1. **CommandDispatcher (editor/session)**: checks the command's required
   capabilities and context before execution.
2. **Gateway**: checks external effects (`sys.*`, `fs.*`, `lsp.start`) and
   applies sandbox/resource policy.

Pure editor commands do not go through the gateway. The gateway is not the
editor command registry.

### Capability matrix

| Command | Required capabilities | Disk write? |
|---|---|---|
| `editor.undo`, `editor.redo` | none | no |
| `search.next`, `selection.*` | none | no |
| `view.split`, `buffer.next` | none | no |
| `lsp.completion`, `lsp.hover` | LSP request | no |
| `lsp.definition` | LSP request, `fs.read` for target open | no |
| `picker.open`, `tree.toggle` | `fs.read` | no |
| `lsp.rename` | LSP request, `editor.write`, `fs.read` for unopened files | **no** |
| `lsp.code_action` | LSP request, `editor.write`, `fs.read` for unopened files | **no** |
| `lsp.format` | LSP request, `editor.write`, `fs.read` for unopened files | **no** |
| `editor.save`, `:wa` | `fs.write` | yes |

`WorkspaceEdit` application updates buffers and creates ghost buffers; it does
not write files. `fs.write` is only for an explicit save command.

## 6. High-frequency migration order

### Phase A: pure synchronous commands

Migrate `editor.undo`, `editor.redo`, `search.next`, `view.split`, and
`buffer.next`.

Entry gate: the ADR-0008 keymap-layer decision is amended or superseded and
accepted. Do not start Phase A while the accepted ADR still names Lua/TS as the
target keymap layer.

Acceptance:

- Same editing behavior and undo boundaries.
- No async delay or gateway call.
- Key dispatch benchmark remains in the microsecond range.
- Registry and trie tests cover conflicts and count/prefix handling.

### Phase B: cancellable LSP queries

Migrate `lsp.hover`, `lsp.completion`, and `lsp.definition`.

Acceptance:

- Completion cancels/replaces stale requests.
- Hover has one in-flight request.
- Results carry path and revision; stale results do not mutate state.
- Key latency is independent of language-server latency.

### Phase C: save

Migrate `editor.save` and `:wa`.

Acceptance:

- Single-flight per buffer.
- Explicit `fs.write`.
- Failed save leaves the buffer dirty and reports the error.
- No save writes from workspace-edit application.

### Phase D: mutating LSP commands

Migrate `lsp.rename`, `lsp.code_action`, and `lsp.format`.

Acceptance:

- Apply WorkspaceEdit with `editor.write` and optional `fs.read`.
- No `fs.write` during apply.
- Optional `:wa`/save is a separate authorized effect.
- Overlapping/invalid edits do not partially mutate buffers.

### Phase E: high-frequency pickers

Migrate `picker.open` and `tree.toggle`, then address the known
selection-scroll bug separately.

### Phase F: Lua declarations and plugin commands

Only after the plugin host owns a VM:

- configuration parsing and atomic keymap swap;
- plugin command descriptors and effect submission;
- no per-key Lua callbacks.

### Phase G: build commands

Defer `build.run`/make until a build sandbox and explicit build policy exist.
A generic `proc.spawn` capability is not acceptable.

## 7. Contract requirements

### Decision prerequisite: ADR-0008 keymap layer

[ADR-0008](adr/0008-selection-first-interaction.md) decision 3 currently
states that the target key mapping layer is Lua/TS plugins. That conflicts with
this plan.

Before Phase A starts, amend ADR-0008 or add a superseding ADR that changes
**only the keymap-layer decision**:

- Lua/TS remains the declaration/config layer.
- Rust owns the hot-path trie, mode resolution, and CommandId mapping.
- Lua callbacks are not invoked per key event.

The rest of ADR-0008 remains authoritative, including the selection-first core,
native motion/action semantics, and the requirement that keymaps cannot
redefine editing semantics. The old keymap-layer sentence must be explicitly
marked superseded; do not silently contradict an accepted ADR.

Before implementing, freeze:

- Command ID names and namespaces.
- Effect payload schema and error kinds.
- Capability names, using the existing `sys.exec`, `sys.fetch`, `fs.read`,
  `fs.write` vocabulary unless a versioned migration is approved.
- Context names for select/insert/cmdline.
- Revision and cancellation semantics.
- Conflict behavior for duplicate key mappings and duplicate command IDs.

## 8. Verification plan

- Unit tests for registry lookup, duplicate IDs, alias resolution, and
  capability metadata.
- Trie tests for prefix collisions, timeouts, counts, and mode switching.
- Dispatcher tests proving pure commands never call Lua or the gateway.
- Effect queue tests for cancellation, single-flight, coalescing, stale results,
  and bounded overflow.
- WorkspaceEdit tests proving no disk writes during apply.
- Save tests proving `fs.write` is required and failure preserves dirty state.
- A hot-path benchmark for key resolution.
- Existing TUI behavior tests remain green.

## Open questions

- Command registry ownership: `anvil-session` versus a dedicated
  `anvil-command` crate.
- Whether `lsp.*` methods become first-class commands or remain effect
  providers behind command IDs.
- Whether context predicates are limited to declarative fields or permit
  plugin-defined predicates (the latter risks the hot path).
- Whether the gateway should expose a lightweight local policy-decision API, or
  the dispatcher and gateway share a capability table without RPC.
- Exact priority policy for interactive LSP requests versus background index
  work.

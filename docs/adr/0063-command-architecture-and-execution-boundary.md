# ADR-0063: Command Architecture and Execution Boundary

- Status: Accepted; command IDs, capability checks, and effect queue implemented
- Version: 1.0.0
- Date: 2026-11-20
- Related: [ADR-0015](0015-command-line-completion.md),
  [ADR-0040](0040-helix-keymap-compatibility.md),
  [ADR-0042](0042-selection-first-session-frontend.md),
  [ADR-0049](0049-editor-control-plane.md)
- Supersedes:
- Superseded by:

## Context

Anvil has several command surfaces: normal-mode keys, pending leader keys,
Ex commands, the command palette, the control socket, MCP, and runtime plugin
commands. They must share authorization and dispatch semantics without putting
Lua, JSON-RPC, or a second editor loop on the keystroke path.

The command-architecture plan contains two measured decisions that were not
captured by an ADR:

- Ex command dispatch remains string-keyed in v1;
- a keymap trie is deferred until a measured prefix-enumeration or registry
  migration need exists.

## Decision

### 1. Stable command identities

Native keymap commands and plugin commands have stable namespaced
`CommandId`s in `anvil-session`. The registry is read-only metadata: id,
aliases, contexts, capability requirements, and availability. It is not a
second execution engine.

Ex command names remain the string-keyed surface in `anvil-control`. Do not
introduce a second Ex-command registry until a measured consumer requires it.

### 2. One capability policy

`anvil-control` owns the command manifest and capability classes used by the
control socket, MCP, and Ex dispatch. Native keymap commands do not bypass that
policy when invoked remotely. Plugin commands remain namespaced and cannot
shadow core command ids.

### 3. Bounded effects for asynchronous work

Pure synchronous editor commands execute immediately. Work that needs LSP,
filesystem, process, or network interaction is represented as a typed effect
and submitted through the bounded `EffectQueue`.

No arbitrary Lua callback runs per keystroke. Lua/Luau may declare keymaps and
submit effects outside the key path, but it cannot directly mutate the editor
buffer.

### 4. Key resolution stays direct

The active normal/context keymaps are hash lookups. A trie is not built
speculatively; add one only when a measured configuration or the registry
migration justifies a shared immutable prefix map.

### 5. Failure and staleness

Effect requests carry request identity, cancellation where applicable, and
revision/path expectations. Stale results are dropped. Queue overflow and
handler failure are explicit errors, not silent fallback to a different
command or editor state.

## Consequences

Positive:

- Keyboard, Ex, control, MCP, and plugin commands share capability semantics.
- The hot path stays free of Lua and gateway round-trips.
- Asynchronous work has a bounded, cancellable ownership boundary.

Negative:

- Ex dispatch still uses string matching in v1.
- Capability metadata and execution remain split between `anvil-session`,
  `anvil-control`, and the TUI adapter.
- A future registry-driven execution migration remains work, not an assumed
  refactor.

## Verification

- Stable command ids and registry metadata are covered by `anvil-session`
  tests.
- Remote control commands are validated against the shared manifest before
  execution.
- Non-substitute Ex commands flow through the bounded effect queue.
- Keymap overrides remain atomic: a bad mapping does not partially replace the
  active map.
- Unknown commands and capability failures produce explicit errors.

# ADR-0014: Plugin WIT Contract from Exercised Editor Seams

- Status: Proposed
- Date: 2026-11-01
- Related: [ADR-0001](0001-plugin-isolation.md), [ADR-0003](0003-idl.md),
  [ADR-0007](0007-jsonrpc-envelope-and-capability-token.md),
  [ADR-0008](0008-selection-first-interaction.md),
  [ADR-0012](0012-plugin-host-isolation.md),
  [ADR-0013](0013-kernel-distribution-boundary.md)

## Context

The plugin API must be defined without inventing an abstract plugin sketch
first. The contract should mirror seams that the editor already exercises.

The current repository does not have a pair of "text change trait" and "view
query trait" ready to expose:

- `anvil_core::TextBuffer` is the only actual trait in the editor path. It is a
  storage contract: construction, byte length, line count, boundary checks,
  raw insert/delete, and slicing.
- `anvil_core::History` is generic over `TextBuffer`, but `anvil-editor` does
  not use it. `EditorState` has its own selection-aware undo/redo.
- `LineIndex` and `EditorState` are concrete types. TUI code calls them
  directly.
- `EditorSnapshot` is an immutable value, not a trait. `EditorSnapshot::view`
  produces the `EditorView` DTO in `anvil-api`.
- `anvil-api` currently has DTOs for `editor.get_view` and `editor.set_mode`,
  plus method-name constants. `editor.apply_edit`, `editor.register_command`,
  `ui.*`, `event.*`, and `sys.*` do not yet have complete DTOs.
- `anvil-plugin-host` is an isolation prototype. It currently handles
  `sys.ping` and a mock `sys.exec`; no Lua/QuickJS VM or runtime is initialized.
  Editor dispatch and plugin VM initialization are not implemented.
- There is no WIT file and no Wasm component host. ADR-0001 defers Wasm and
  keeps the planned Lua/QuickJS host in a separate process; the current host is
  only the empty sandbox and framed dispatch prototype.
- ADR-0003 makes Rust types the source of truth for the JSON-RPC API.

Mirroring `TextBuffer` directly into WIT would expose the wrong boundary. A
plugin could create a raw buffer, bypass selection and undo grouping, and
mutate text without an `EditorState` revision or permission check. That is not
the editor's plugin capability.

The first-party editor and a sandboxed third-party plugin should share one
contract shape, but the policy gateway should choose capability defaults from
the plugin's authenticated origin. The gateway must not choose a different
interface per source.

## Decision

### 1. Keep the storage trait internal

`TextBuffer`, `PieceTable`, `RopeBuffer`, `GapBuffer`, and `LineIndex` remain
Rust-internal seams. They are implementation and storage contracts, not plugin
capabilities.

The plugin contract uses `EditorState` session operations and `EditorView`
values:

- query: `EditorState::snapshot().view()`
- mutation: `EditorState::apply_transaction` and, where required,
  `EditorState::insert_at_selections`
- history: `EditorState::undo` and `EditorState::redo`
- session mode: `EditorState::set_interaction_mode`

These are the seams already used by the TUI and covered by editor tests.

### 2. WIT mirrors session operations, not storage resources

The logical plugin interface is:

```wit
interface editor {
    get-view: func() -> result<editor-view, api-error>;

    apply-edits: func(
        expected-revision: u64,
        edits: list<edit>,
    ) -> result<u64, api-error>;

    undo: func(expected-revision: u64) -> result<u64, api-error>;
    redo: func(expected-revision: u64) -> result<u64, api-error>;

    set-mode: func(mode: interaction-mode) -> result<interaction-mode, api-error>;
}
```

with records shaped from the existing DTOs:

```wit
record range {
    start: u64,
    end: u64,
}

record edit {
    range: range,
    replacement: string,
}

record text-position {
    byte-offset: u64,
    line: u32,
    grapheme-column: u32,
    display-column: u32,
}

record selection {
    anchor: text-position,
    head: text-position,
}

record viewport {
    first-line: u32,
    first-column: u32,
    line-count: u32,
    width: u32,
    height: u32,
}

record editor-view {
    revision: u64,
    interaction-mode: interaction-mode,
    viewport: viewport,
    selections: list<selection>,
}
```

`insert-at-selections` is an optional additive method only if its existing
multi-cursor semantics are part of the plugin contract. It must not replace
`apply-edits`; both must preserve the same undo and selection behavior.

Do **not** expose `from_text`, raw `insert`, raw `delete`, `slice`, or a
`text-buffer` resource in WIT.

### 3. One contract shape for first-party and sandboxed code

When the plugin host actually needs to call the editor, add a narrow Rust trait:

```rust
trait EditorSession {
    fn view(&self) -> EditorView;
    fn apply_edits(&mut self, expected: u64, edits: Vec<Edit>) -> Result<u64, ApiError>;
    fn undo(&mut self, expected: u64) -> Result<u64, ApiError>;
    fn redo(&mut self, expected: u64) -> Result<u64, ApiError>;
}
```

- `EditorState` implements it in-process.
- The privileged editor dispatch and the sandboxed host proxy implement the
  same shape.
- The TUI stays on `EditorState` until a second consumer exists; do not genericize
  it speculatively.

The WIT interface is the cross-language projection of this trait and the
`anvil-api` records. It is not a second, unrelated API.

### 4. Keep one source of truth

Until a Wasm/component host is selected, `anvil-api` Rust types remain
normative, per ADR-0003. WIT is a checked projection, not a handwritten parallel
schema.

If WIT becomes the normative IDL for a component host:

- generate Rust and TypeScript bindings from WIT, or generate WIT from the
  Rust DTOs and add a CI parity check;
- keep field names and error codes aligned;
- do not maintain both by hand.

Adding a WIT file without a component host is a design artifact, not an
implemented plugin path.

### 5. API preconditions must be closed first

The existing editor methods are not yet sufficient for the WIT shape above:

- `EditorState::apply_transaction` returns `bool` and has no
  `expected_revision` check.
- `EditorState` does not expose structured action errors; `CommandError` lives
  in `anvil-core` and is not mapped into `ApiError`.
- `anvil-api` has only `get_view` and `set_mode` DTOs.

Before freezing the WIT contract:

1. Define `apply-edits` conflict handling and add the corresponding
   revision-aware editor path.
2. Map edit failures to stable `ApiError` kinds.
3. Freeze DTOs for every method that the contract exposes.
4. Add parity tests for the first-party implementation and the host proxy.

### 6. Source identity selects capability templates

The gateway gets an authenticated source class at registration:

```rust
enum PluginSource {
    Official,
    TrustedDistribution,
    LocalUser,
}
```

- The editor resolves the source from the signed manifest, package signature,
  or installation origin. A plugin must not self-declare its source.
- The editor registers `plugin_id`, token hash, workspace root, source, and
  effective grants with the gateway.
- The gateway selects a versioned default capability template for that source.
- An explicit user grant may narrow the template, or widen it only through a
  separate user-visible grant flow.
- `session.hello` returns the effective grants.
- The method schema and WIT interface are identical for every source.

Source classes do not bypass the plugin-host sandbox. `Official` means broader
default grants, not unsandboxed execution. Only editor-internal first-party
logic may run in-process, and it is not a third-party plugin.

### 7. Capability scope is enforced at the right layer

- `editor.*`, `ui.*`, and `event.*`: the editor dispatcher checks the effective
  grant before calling `EditorState`.
- `sys.*`: the Go gateway checks the grant and performs the I/O.
- Filesystem, network, and exec restrictions are also enforced by Landlock,
  seccomp, and the exec-runner sandbox.
- The plugin host is untrusted after compromise; it cannot be the only
  enforcement point.
- A capability template cannot widen a kernel sandbox rule.

## Consequences

Positive:

- The plugin contract follows behavior the editor already tests.
- Storage backends remain replaceable without changing plugin contracts.
- First-party and sandboxed callers can share one interface and one set of
  DTOs.
- Source-based defaults replace a speculative universal least-privilege table
  without creating source-specific APIs.

Negative:

- The WIT shape cannot be finalized until revision and error semantics are
  added to the editor API.
- A Rust `EditorSession` trait is still a new seam; it is introduced only when
  the plugin host becomes a real consumer.
- WIT and JSON-RPC require a single-source generation/parity strategy.
- Official plugins remain sandboxed, so official status is not a performance or
  in-process shortcut.

## Alternatives considered

### Mirror `TextBuffer` into WIT (rejected)

It exposes storage operations, bypasses selection/undo/line-index invariants,
and lets plugins create documents that the editor does not own.

### Invent a separate view-query trait now (rejected)

There is no second implementation. `EditorSnapshot` is already the immutable
query value, and `EditorView` is the serialized shape.

### Design a universal least-privilege template (rejected)

A single minimum template ignores trust and distribution reality. Source-based
templates plus authenticated origin are more practical, provided the interface
does not change by source.

### Hand-write WIT and JSON Schema independently (rejected)

Two unverified IDLs drift. Use one source and a CI parity check, or do not add
WIT until the component host is selected.

## Verification

- Inventory check: only `TextBuffer` is a Rust trait; no view-query trait is
  claimed.
- Contract check: WIT fields match `anvil-api` DTO fields and error codes.
- Behavior check: first-party `EditorSession` and host proxy produce identical
  text, selections, revisions, undo, and redo for the same edits.
- Permission check: an editor method called by a plugin cannot bypass the
  editor dispatcher.
- Source check: a plugin cannot claim `Official` in its own manifest; unsigned
  or unknown packages default to `LocalUser`.
- Sandbox check: an `Official` plugin still cannot read outside its workspace
  or execute an unallowlisted tool.
- Template check: user grants intersect with the selected template; denied
  requests return `policy_denied` without leaking policy internals.

## Open questions

- Whether the first component host is Wasm/WASI or the planned Lua/QuickJS host
  remains deferred by ADR-0001. The current host initializes neither runtime.
- Whether `insert_at_selections` belongs in v1 WIT or remains an editor-internal
  fast path behind `apply-edits`.
- Whether `register_command`, `ui.*`, and `event.*` are part of the first WIT
  world or a later additive interface.

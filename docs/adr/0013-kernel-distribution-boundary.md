# ADR-0013: Rust Kernel and Distribution Boundary

- Status: Proposed
- Date: 2026-11-01
- Related: [ADR-0002](0002-go-daemon.md), [ADR-0006](0006-upstream-dependencies.md), [ADR-0008](0008-selection-first-interaction.md)

## Context

Anvil is intended to have a reusable Rust kernel and an official standard
edition. Third parties should be able to compose the kernel with their own UI,
configuration, and trusted Rust code, then ship a distribution. This is similar
in spirit to a Linux kernel plus distributions, but it is not a binary-ABI
model: Rust has no stable ABI.

The current repository is a Phase 0 monorepo:

- `anvil-core`, `anvil-editor`, `anvil-api`, and `anvil-ipc` are candidate
  kernel crates.
- `anvil-tui` currently contains both frontend code and standard-edition
  composition: key handling, LSP orchestration, file workers, and UI.
- The Go gateway is a per-session trusted policy service.
- The plugin host exists as an isolation prototype, not a public plugin
  runtime.

Without an explicit boundary, "kernel" will become whichever crate happens to
be reused first, and a third-party distribution will have to fork the TUI.

## Decision

### Source-level kernel, not a Rust plugin ABI

The public extension model for Rust code is **source-level composition**.
Distributions depend on published kernel crates, add trusted Rust crates, and
compile their own binary. Anvil does not promise a stable Rust ABI and does not
load dynamic Rust libraries as plugins.

Runtime plugins remain Lua/TS plugins running in the isolated plugin host.

### Candidate kernel crates

The following crates may expose a public, semver-versioned API:

- `anvil-api`: public capability and editor-view types.
- `anvil-core`: text model and text-buffer contracts.
- `anvil-editor`: selection-first editor state, commands, undo, and snapshots.
- `anvil-ipc`: protocol codec when an external client needs the wire contract.
- A future `anvil-session`: headless document, workspace, LSP, gateway, and
  plugin lifecycle orchestration.

`anvil-tui` is the current frontend and standard-edition implementation, not a
kernel API. UI-specific commands, rendering, key translation, and default
configuration must not leak into kernel crates.

`anvil-plugin-host` is an implementation of the runtime-isolation boundary. Its
internal Rust API is not a public plugin ABI.

### Standard edition

The official standard edition is a composition crate/binary that selects:

- the TUI and default keymap;
- default language-server settings;
- default plugins and themes;
- the gateway and exec-runner packaging.

The standard edition may be replaced by a third-party distribution without
changing kernel crates.

### Gateway contract

The Go gateway remains a per-session trusted policy service behind a versioned
protocol. A distribution that replaces the gateway must preserve:

- the capability/token handshake;
- the `sys.*` policy and cancellation semantics;
- the exec-runner sandbox boundary;
- the per-session lifecycle.

The Go gateway is not part of the Rust ABI and is not a plugin runtime.

### Trust model

- Rust code compiled into a distribution is **trusted distribution code** and
  runs in the editor process. It has the same trust as kernel code built into a
  kernel image.
- Lua/TS plugins are **untrusted runtime code** and run in the plugin host.
- The gateway is trusted to enforce policy but is unprivileged and isolated by
  process/socket boundaries.
- Compiling Rust into a distribution does not make it sandboxed, and must not be
  described as such.

### Compatibility and interface freeze

- Current `0.x` APIs are experimental. Semver applies to public Rust APIs, but
  breaking changes are allowed before a documented freeze.
- No interface is frozen until it has at least two consumers: the official
  standard edition and an independent distribution/test consumer.
- Wire protocols require explicit version negotiation before a stable claim.
- A frozen interface needs tests for compatibility, cancellation, errors, and
  security invariants.
- `anvil-api` and future `anvil-session` are the first candidates for a stable
  contract; TUI/UI types are not.

## Consequences

Positive:

- Third parties can compose a distribution without forking the TUI.
- The Linux-like kernel/distribution story does not depend on unstable Rust ABI.
- Trusted compiled code and untrusted runtime plugins remain distinguishable.
- Security boundaries stay process/protocol-based.

Negative:

- Extracting `anvil-session` from the current TUI is necessary before the
  boundary is complete.
- Publishing crate APIs requires compatibility tests and documentation.
- Third-party Rust code is trusted code; the ecosystem must not imply sandboxing.
- The current `anvil-tui` remains temporarily oversized during the transition.

## Alternatives considered

### Stable dynamic Rust plugin ABI (rejected)

Rust has no stable ABI and dynamic loading would create version and unwinding
hazards. Runtime plugins use Lua/TS instead.

### Keep the monolithic application as the only distribution (rejected)

This prevents third-party UI and composition while the project claims a kernel
model.

### Make the Go gateway a global daemon (rejected)

This would turn one workspace compromise into a cross-workspace risk and
conflicts with ADR-0002.

## Verification expectations

- Kernel crates have no dependency on `anvil-tui` or terminal UI types.
- `anvil-tui` can be replaced by a test frontend using only kernel/session APIs.
- The official standard edition and at least one independent consumer build
  against the same candidate API.
- Gateway protocol compatibility is tested with version negotiation.
- Trust documentation distinguishes compiled Rust code from sandboxed plugins.

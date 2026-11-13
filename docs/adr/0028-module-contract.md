# ADR-0028: Trusted Source-Level Module Contract

- Status: Accepted
- Date: 2026-11-13
- Related: [ADR-0013](0013-kernel-distribution-boundary.md), [ADR-0027](0027-distribution-composition-crate.md)

## Context

ADR-0027 landed the standard composition crate and a second consumer. The
`0.7.x` plan can now define how official optional Rust modules join a
distribution.

Modules are trusted, source-level Rust crates compiled into the binary. They are
not runtime Lua/TS plugins, do not use a stable Rust ABI, and are not loaded
from a workspace, marketplace, or download.

The contract must prevent the composition crate from becoming a dumping ground
and prevent modules from bypassing kernel, session, gateway, or focus/overlay
semantics.

## Decision

### 1. Contract crate

Add `anvil-module` as the typed contract crate. It may depend on
`anvil-session` and `anvil-editor` public APIs, but never on `anvil-tui`
internals or `anvil-tui` private state.

Modules depend on `anvil-module`, not on the standard distribution crate.
Composition crates depend on both.

### 2. Static registration only

Each module exposes one explicit registration function:

```rust
pub fn register(registry: &mut ModuleRegistry) -> Result<(), ModuleError>;
```

The composition crate calls these functions explicitly. There is no scanning,
dynamic discovery, `dlopen`, environment-selected module loading, or runtime
installation.

Registration produces only typed contributions:

- commands;
- background services;
- explicit extension-point providers declared by the contract;
- module metadata and lifecycle hooks.

Unknown or duplicate contributions fail registration.

### 3. Identity and namespacing

- Module id: `<vendor>.<name>`.
- Command ids contributed by a module: `<module-id>.<command>`.
- Module ids and command ids are unique within a composition.
- A module may not replace, shadow, or mutate a kernel command id.
- Collisions fail at startup before the TUI event loop starts.
- Module ids and command ids are static strings; no user-controlled names.

The standard edition reserves the `official.*` namespace for its modules. A
third-party module must use its own vendor prefix.

### 4. Compatibility

- `anvil-module` exports an integer module-contract version.
- A module declares the contract range it supports.
- The composition crate validates every module before initialization.
- A mismatch is a startup error; it is never silently ignored.
- Module compatibility is source-level: kernel/module crates use semver and the
  distribution rebuilds modules.
- No stable Rust ABI promise is made.

### 5. Lifecycle and failure

Module initialization runs before the TUI loop and before any user input is
processed.

- Registration or initialization failure fails the binary closed and reports
  the module id.
- A module panic during initialization fails the binary closed.
- Runtime panic behavior must be documented per extension point; the default is
  fail closed, not partial editor state.
- A module may not add a second event loop, start a UI-owned thread that renders,
  or bypass the focus/overlay stack.
- Shutdown hooks run once, in reverse registration order.

No module-disable-at-runtime capability is defined in this contract. A
composition may add isolation later only with an ADR proving state integrity;
until then, fail closed.

### 6. Capabilities and I/O

Modules are trusted, so there is no sandbox. They still receive no ambient
privilege:

- Filesystem, Git, network, and process access go through the same
  `anvil-session`/gateway APIs and policy checks as other callers.
- A module cannot request a plugin capability token.
- A module cannot change `sys.*` grants, Landlock/seccomp policy, the exec
  runner, or gateway policy.
- Modules must not invent a second I/O path for convenience.
- UI contributions must use contract extension points; they may not reach into
  `anvil-tui` private state.

### 7. Composition and features

The composition crate is the only place that selects modules.

- Each module is a Cargo feature plus an explicit registration call.
- The standard edition declares its curated set and includes `official.git` by
  default in the first slice.
- The minimal distribution fixture remains feature-free unless its own
  composition explicitly selects modules.
- A feature-free build must compile and run without module crates.
- Modules must not be enabled by environment variables at runtime.

### 8. First module: `official.git`

`anvil-mod-git` is the first contract exercise:

- It exposes the existing trusted Git integration through module extension
  points.
- It reuses the established gateway/exec boundary; it adds no new I/O path.
- It can be excluded without breaking editing, LSP, tree, search, or the TUI.
- Standard edition includes it by default; the minimal fixture does not.
- Behavior parity with the pre-module implementation is required before
  additional modules are accepted.

The exact Git extension points are defined with the 0.7.1 implementation, not
in this ADR. They must be UI-neutral and must not expose `anvil-tui` private
state.

### 9. Validation and tests

The 0.7.1 implementation must cover:

- feature-free build;
- standard build with `official.git`;
- module-only build;
- duplicate module id;
- duplicate command id;
- kernel command shadowing;
- contract version mismatch;
- module registration failure;
- Git behavior parity;
- no dependency on `anvil-tui` internals from `anvil-mod-git`;
- installed standard binary starts with and without the module feature.

## Consequences

- Module wiring is explicit and reviewable.
- The standard edition remains a composition, not a second kernel.
- Adding a module requires a Cargo feature, a registration call, collision
  validation, and compatibility metadata.
- There is no runtime module ecosystem and no dynamic loading.

## Non-goals

- Stable Rust ABI.
- Dynamic Rust plugins or hot loading.
- Loading modules from a workspace or marketplace.
- Sandboxing trusted modules.
- Moving core editing semantics out of kernel crates.
- A public third-party module ecosystem before the compatibility gate.

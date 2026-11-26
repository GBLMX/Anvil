# ADR-0027: Distribution Composition Crate and Two Consumers

- Status: Accepted
- Date: 2026-11-13
- Related: [ADR-0013](0013-kernel-distribution-boundary.md), [ADR-0024](0024-tui-state-and-domain-boundaries.md)

## Context

ADR-0013 separates the Rust kernel from a distribution, but the command-line
entrypoint and process composition currently live in the `anvil-tui` binary.
That makes `anvil-tui` both the frontend library and the de facto standard
distribution. A second consumer cannot reuse the entrypoint without copying CLI
parsing, control-socket forwarding, and process error handling.

The `0.7.x` module plan cannot start until a standard-edition composition crate
exists and the ADR-0013 two-consumer gate is real.

## Decision

### 1. Add a thin standard composition crate

Create `anvil-distro-standard` with:

- a reusable `Distribution` value (`name`, `usage`);
- the shared command-line parser;
- control-socket forwarding for an already running instance;
- process error/exit-code handling;
- the call into `anvil_tui::run_with_input`.

The crate does not own editing, LSP, Git, filesystem, sandbox, or plugin
semantics. It only composes the existing frontend and kernel APIs.

### 2. The standard binary moves

The `anvil` binary target moves from `anvil-tui` to
`anvil-distro-standard`. `anvil-tui` remains a library and test/benchmark
frontend. Release layouts and PGO builds select `anvil-distro-standard`.

This is a cutover, not a compatibility shim: no second `anvil` binary remains in
`anvil-tui`.

### 3. Add a second consumer

Add `examples/anvil-distro-minimal`, a workspace member with its own
`anvil-minimal` binary. It calls the same `anvil_distro_standard::main` entrypoint
with a different `Distribution` value.

The fixture intentionally does not fork the CLI or startup logic. Its purpose is
to prove the composition boundary before module APIs exist. It is not a
marketplace product.

### 4. Module contract remains separate

This ADR does not define `anvil-mod-*` registration, capabilities, or feature
selection. A module-contract ADR is still required before `0.7.x` code. The
composition crate is the place where future curated module sets will be
selected.

## Consequences

- Two consumers share one entrypoint and one CLI contract.
- Release and PGO scripts must build the composition crate.
- `anvil-tui` can be tested and benchmarked without owning a process entrypoint.
- The minimal fixture adds a second binary to workspace builds; that cost is
  accepted as the two-consumer gate.
- No dynamic Rust ABI, runtime module loading, or permission widening is
  introduced.

## Exit criteria

- `anvil` is produced only by `anvil-distro-standard`.
- `anvil-minimal` builds and reports its own distribution name.
- Both binaries share parsing, forwarding, and TUI startup.
- Release layout, CI, and PGO commands select `anvil-distro-standard`.

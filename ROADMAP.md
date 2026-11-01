# Roadmap

This is a directional roadmap, not a release commitment. The detailed gates stay
in `PROJECT_CHARTER.md`, `docs/phase-0-7-plan.md`, and the ADRs.

## Current

- `0.4.x` feature line after the `0.3.0 → 0.4.0` maintenance release.
- Stabilize the selection-first kernel and its existing tests.
- Complete the kernel/distribution boundary decision
  ([ADR-0013](docs/adr/0013-kernel-distribution-boundary.md)).
- Keep the interim TUI keymap clearly separated from kernel semantics.

## Next

1. Extract `anvil-session` from the TUI so a frontend can use the kernel without
   forking the terminal UI.
2. Version the gateway protocol and add compatibility tests.
3. Establish a reproducible kernel performance baseline.
4. Add user keymap/config boundaries without freezing the plugin keymap format.
5. [0.4.4–0.4.8] Split the plugin runtime into gateway capability control,
   manifest/host loading, activation supervisor, resource isolation, and
   plugin I/O/document transactions; design is in
   [ADR-0017](docs/adr/0017-plugin-activation-and-lifecycle.md).
6. [0.4.1] Close the remaining workspace tree watcher validation in
   [#9](https://github.com/GBLMX/Anvil/issues/9); implementation is described
   in [ADR-0018](docs/adr/0018-workspace-tree-watch.md).

## Later

- A standard-edition composition crate and a sample third-party distribution.
- Broader language/tooling support after the kernel boundary is stable.
- Release signing, SBOM, and provenance after release automation exists.
- Optional PGO/BOLT for the official binary only after representative workload
  measurements.
- Windows/macOS remain out of scope for v1.

## Explicit non-goals

- A stable Rust ABI or dynamic Rust plugin loading.
- Vimscript compatibility.
- A plugin runtime inside the editor process.
- A global gateway shared by multiple workspaces.
- Claiming production support before a stable release exists.

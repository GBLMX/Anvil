# 0.3.0 → 0.4.0: Polish & Specification

Status: active release train. `0.3.x` is the polish phase; `0.4.0` closes it.

## Intent

No new capability families during this train. Spend the version range on
correctness, security, UX consistency, documentation, and executable
specifications. A feature may land only when it closes an existing advertised
contract or release gate.

## Plugin-host scope

`0.4.0` closes the empty plugin-host boundary only. A plugin VM/runtime, plugin
loading, the capability table, gateway control-socket token registration, and
end-to-end plugin `sys.*` dispatch are explicitly `0.5.0` work.


## Allowed work

- Bug, crash, data-loss, and security fixes.
- Conformance fixes in the editor, LSP, gateway, IPC, and plugin-host paths.
- Tests, documentation, ADRs, schemas, generated-binding checks, and
  performance regression measurements.
- Refactors justified by correctness or testability, with no undocumented
  behavior drift.
- Closing incomplete advertised features, such as command-style Code Actions
  or missing LSP formatting options.
- Dependency removals and dead-code removal.

## Not allowed

- New plugin runtimes or public plugin API families.
- New dependency categories without an ADR.
- Broad redesigns, speculative abstractions, or unrelated refactors.
- Platform expansion beyond the documented Linux-first scope.
- Feature flags that are not required to close an existing contract.

## Specification deliverables

- Every declared key, command, LSP operation, error code, and trust boundary
  has a document or ADR and a test or an explicit unsupported-status note.
- LSP surface is closed and consistent: diagnostics, completion, definition,
  hover, rename, Code Actions, formatting, and document symbols.
- Command-style Code Actions are either implemented or explicitly excluded from
  the advertised contract; they must not fail silently.
- Public/API version semantics and schema generation remain reproducible.
- Security invariants have executable tests; a failing invariant blocks release.
- Known limitations remain listed in the README or the relevant document.

## 0.4.0 release gates

- No known P0/P1 defects, data-loss paths, or panics on normative runs.
- `cargo test --workspace --no-fail-fast` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo fmt --all -- --check` passes and `git diff --check` is clean.
- Docs, CHANGELOG, project charter, architecture maps, and crate versions agree.
- No unreferenced placeholder markers or advertised-but-unimplemented paths.
- The working tree contains only intended release changes.

## Versioning during the train

- Patch releases (`0.3.x`) carry fixes, tests, and documentation only.
- `0.4.0` is tagged after the gates above pass.
- Breaking changes are still possible before `1.0.0`, but require documentation
  and must not be used to sneak in new feature families.

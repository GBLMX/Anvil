# ADR-0077: Benchmark-Only Text Backends

- Status: Proposed
- Version: 1.0.0
- Date: 2026-11-24
- Related: [ADR-0004](0004-text-model.md),
  [ADR-0009](0009-text-model-snapshot-gate.md),
  [ADR-0037](0037-kernel-api-baseline.md)
- Supersedes:
- Superseded by:

## Context

ADR-0009 selects `PieceTable` as the production text backend. Rope and Gap
Buffer remain comparison implementations. They are useful evidence, but they
are still part of the production crate:

- `anvil-core/src/bin/text-bench.rs`, `text-p99.rs`, and `text-rss.rs` import
  `RopeBuffer` and `GapBuffer`;
- `anvil-core/benches/text_model.rs` compares both;
- `RopeBuffer` and `GapBuffer` are exported from `anvil-core`;
- `ropey` is a normal dependency of `anvil-core`.

Deleting the three binaries alone would only move the entry points. The
`ropey` dependency and public benchmark-only backends would remain. The
benchmark report and gate remain valuable; the production kernel does not need
to expose comparison implementations to keep them.

## Decision

### 1. Keep PieceTable as the production backend

`TextDocument = PieceTable` remains the only production `TextDocument`. The
`TextBuffer` contract, `LineIndex`, command history, and production APIs remain
in `anvil-core`.

### 2. Move comparison backends to a benchmark-only crate

Move these into an unpublished benchmark crate, for example
`anvil-text-bench`:

- `RopeBuffer`;
- `GapBuffer`;
- the three text benchmark binaries;
- the Criterion comparison benchmark.

The benchmark crate is a development artifact. It is not a runtime dependency
of `anvil-core`, `anvil-editor`, `anvil-session`, or the TUI.

### 3. Remove the production ropey dependency

After the move, `anvil-core` must not depend on `ropey` and must not export
`RopeBuffer` or `GapBuffer`. The benchmark crate owns `ropey` and the
comparison tests.

### 4. Preserve benchmark evidence

The measured results and selection rationale remain in
`docs/benchmarks/text-model.md`. Moving the harness does not weaken the
ADR-0009 gate; it only removes benchmark-only code and dependencies from the
production kernel.

### 5. Treat the API removal as intentional

Removing benchmark-only public types from `anvil-core` is an API break. It is
accepted as part of this 0.x decision, but it must be documented in the
changelog and migration guidance.

## Consequences

Positive:

- A smaller production kernel API.
- One fewer normal dependency in `anvil-core`.
- Benchmark code and comparison-only tests have one owner.
- The published text-model evidence remains intact.

Negative:

- `RopeBuffer` and `GapBuffer` no longer exist as `anvil-core` exports.
- Existing benchmark commands move to the benchmark crate.
- Criterion and ropey move from kernel dev dependencies into the benchmark
  crate.
- The change is not a runtime feature removal, but it is a public API break.

## Verification

- Focused tests:
  - `cargo tree -p anvil-core` contains no `ropey`;
  - `cargo test -p anvil-core` passes with `TextDocument = PieceTable`;
  - the benchmark crate builds and runs the comparison harness;
  - `rg` finds no `RopeBuffer` or `GapBuffer` exports in `anvil-core`.
- Failure/cancellation tests:
  - removing a benchmark-only module does not change editor/session tests;
  - a stale benchmark command fails with a clear "benchmark crate" message.
- Real surface smoke:
  - `cargo test --workspace` and the documented benchmark command pass;
  - `docs/benchmarks/text-model.md` still points to reproducible commands.

## References

- [ADR-0004](0004-text-model.md)
- [ADR-0009](0009-text-model-snapshot-gate.md)
- [Text model benchmark](../benchmarks/text-model.md)

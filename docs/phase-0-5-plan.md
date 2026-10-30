# Phase 0.5 Plan: Core Evidence and Editing Primitives

Status: approved for execution; 6-week timebox. M1 is complete.

## 1. Entry state

Phase 0 is not complete. The current baseline is:

- `anvil-api`: interaction/view/error contracts, generated TS, JSON Schema,
  LuaLS stubs, method constants, and binding freshness checks.
- `anvil-ipc`: framing, JSON-RPC envelope, Unix socket transport, and
  `SO_PEERCRED`.
- `go/internal/fsroot`: `openat2` workspace reader and tests.
- `anvil-core`: `TextBuffer` with Rope, Gap Buffer, and piece-table candidates.
- `text-bench`: single-run 1/10/100 MB smoke benchmark.
- CI: Rust, Go, docs, TypeScript binding checks, and generated-artifact
  freshness.

The current `text-bench` is a smoke test: it proves the trait boundary works but
is not decision-grade. It does not prove which backend should enter production.

### Phase 0 map

1. **0.1 Documents and contracts**: complete.
2. **0.5 Core evidence and editing primitives**: this plan.
3. **0.6 Security work**: exec/fetch/plugin-host isolation, SSRF, and gateway.
4. **0.7 TUI minimum editor**: first-frame measurement and the cold-start gate.

The subphases are sequential for exit criteria, but API-contract alignment may
run in parallel when it does not change core semantics.

## 2. Goal

Turn the core from "three implementations behind one trait" into an
evidence-backed, correctness-tested editing core with one selected production
text backend.

Phase 0.5 exits with:

1. a trustworthy benchmark and a signed text-model decision;
2. one production backend, with the others benchmark-only;
3. a tested cursor/command/revision foundation that uses the selected backend;
4. no regression in the existing API and transport checks.

## 3. Non-goals

- No TUI or rendering work.
- No plugin host, Lua/QuickJS runtime, or Go gateway implementation.
- No complete ten-method API surface.
- No speculative parser, syntax-highlighting, or LSP stack.
- No second production text backend.

`sys.exec` and `sys.fetch` security types are a separate Phase 0 security
workstream; this plan only records their boundary.

## 4. Workstream A: TextBuffer correctness

### Required changes

- Add `is_char_boundary` assertions to `insert` and `delete` for every backend.
  A non-boundary offset must panic at the operation that received it, not later
  in `slice`.
- Define and test boundary semantics for `insert`, `delete`, and `slice` on
  empty buffers, start/end positions, multi-byte characters, and full deletion.
- Add property tests that compare random operation sequences against a plain
  `String` model. At minimum:
  - insert at every valid boundary;
  - delete ranges of varied sizes;
  - unicode text with combining and wide characters;
  - empty and one-line buffers.
- Make `len_lines` semantics identical across all candidates, including empty
  text, trailing newline, and no trailing newline.
- Remove hidden O(n) line counting from any path intended to run per frame.
  Either cache the count, maintain it incrementally, or explicitly make the
  backend fail the line-lookup gate.

### Acceptance

```text
cargo test -p anvil-core
proptest suites pass with documented seeds
invalid-boundary tests panic at insert/delete, not slice
all three candidates agree on line-count cases
```

## 5. Workstream B: Decision-grade benchmark

### Harness

- Add `criterion` as a dev dependency and a `[[bench]]` target.
- Keep `text-bench` as a quick smoke tool; it is not the decision harness.
- Use deterministic seeds and fixed edit traces.
- Warm up before sampling; report p50/p95/p99 and max, not one `Instant`.
- Run at least 30 samples per case.

### Commands

```text
TEXT_BENCH_MB=1 cargo bench --bench text_model
cargo bench --bench text_model -- --save-baseline main
cargo run --release -p anvil-core --bin text-rss -- rope 100
cargo run --release -p anvil-core --bin text-rss -- gap 100
cargo run --release -p anvil-core --bin text-rss -- piece 100
TEXT_P99_MB=100 cargo run --release -p anvil-core --bin text-p99
```

RSS must run one backend per process; it reports `VmRSS` and `VmHWM`.

### Workloads

For 1 MB, 10 MB, and 100 MB:

- open, line index construction, first viewport;
- random inserts and deletes at 1/50/99% offsets, with varied sizes;
- large deletes spanning multiple lines;
- `len_lines` and line lookup;
- viewport extraction without full-document copies;
- one-line 100 MB;
- Unicode and CRLF/BOM cases;
- peak RSS, using a Linux `/proc/self/status` probe or `getrusage`.

Undo/redo and snapshot benchmarks run after M4 creates the command/actor layers.
They supplement M3; they do not block it. Snapshot correctness before M4 is a
structural check only (no full-document copy), not a performance claim.

### Decision framework

Write `docs/adr/0009-text-model-decision.md` before accepting benchmark
results.

Required content:

- hard gates: correctness, `len_lines` and line lookup O(1) or O(log n) on the
  selected backend, 100 MB open/viewport budgets, and no full-document copy per
  snapshot;
- weighted criteria:
  - random-edit p99: 40%
  - line lookup and viewport: 25%
  - memory/RSS: 20%
  - snapshot/undo: 15%;
- weight provenance: the split is a v1 engineering prior based on interactive
  editing frequency, not measured user telemetry. ADR-0009 records that
  assumption and includes a ±10% sensitivity check; if the winner changes, add
  workloads rather than accept the prior;
- tie-break: Rope or piece table wins only when the weighted score is within 5%;
  Gap wins when it exceeds the score by more than 5% and passes every hard gate;
- invalid-run rule: repeat any case whose p99 coefficient of variation exceeds
  20%; three consecutive invalid runs mark the machine unsuitable;
- sign-off: one owner signature and one reviewer signature (owner-only
  self-approval during Phase 0.5, with a second reviewer from Phase 1 onward).

The percentages are decision inputs, not performance claims. They must be
fixed before looking at results.

### Acceptance

- `cargo bench --bench text_model` produces a report with all candidates.
- Results are recorded in `docs/performance.md` with commit, CPU, kernel, and
  command.
- The decision ADR is accepted before production code is switched.
- Only the selected backend is enabled by default; other backends are
  feature-gated or benchmark-only.
- The decision baseline is saved and a `bench-regression` CI job compares
  against it; p99 regression above 10% fails the job.

## 6. Workstream C: Core editing primitives

### Cursor and coordinates

- Decide whether cursor/coordinate conversion belongs in `anvil-core` or
  `anvil-tui`; record the decision in ADR-0010.
- Add byte/char/grapheme/display-cell conversion tests using
  `unicode-segmentation` and `unicode-width` if core owns it.
- Define `offset_to_line` and `line_to_offset` ownership; do not leave line
  indexing implicit.
- ADR-0010 must answer: whether `EditorView` is built by core or TUI; whether
  `unicode-*` lives in core or a separate crate if core builds it; and whether
  TUI snapshots carry byte offsets only or the full three-column position.

### Commands, undo, and revision

- Add an `EditCommand`/transaction type with a stable revision.
- Define undo grouping at the command boundary, not per byte.
- Add tests for:
  - insert/delete undo/redo;
  - merged typing groups;
  - stale revision rejection;
  - selection preservation across edits.
- Add an `EditorSnapshot` path with an immutable revision and viewport state;
  snapshots must not copy the full document per frame.

### Acceptance

- Core tests cover cursor conversion boundaries, undo grouping, stale
  revisions, and snapshot immutability.
- `anvil-api::EditorView` can be constructed from the core snapshot without
  duplicating coordinate logic.
- No Tokio, `ratatui`, plugin runtime, or gateway dependency enters
  `anvil-core`.

## 7. Workstream D: Contract alignment

Only changes required by the core slice:

- Keep `editor.get_view` and `editor.set_mode` types stable.
- Add method constants only; do not add payloads for the remaining eight
  methods in this phase.
- Keep `ApiError` mapping aligned with ADR-0007.
- Regenerate TS/Schema/Lua artifacts and pass the freshness gate.
- Method constants, error alignment, and regeneration may run in parallel with
  M1–M3 as long as they do not change core semantics.
- `set_selection` and cursor-related API additions wait until M4 accepts
  ADR-0010.

## 8. Milestones

### M0: Baseline (done)

- three candidates;
- smoke benchmark;
- CI and generated bindings.

### M1: Correctness gate (done)

- boundary assertions;
- property tests;
- line-count semantics;
- cached line counts; no per-call full-document allocation.

### M2: Benchmark gate (in progress: criterion + RSS harness landed)

- criterion harness;
- workload and RSS coverage;
- deterministic edit traces;
- benchmark report;
- baseline report: [text-model.md](benchmarks/text-model.md)
- baseline saved for PR regression checks.

### M3: Decision gate

- ADR-0009 accepted and signed off by owner + reviewer;
- one production backend selected;
- other backends benchmark-only.

### M4: Editing primitives gate

- cursor/coordinate ownership ADR;
- command/undo/revision;
- snapshot contract;
- core tests and API view integration.

### M5: Phase 0.5 exit

- all gates above pass;
- no `todo!()`, `unimplemented!()`, empty command bodies, or placeholder
  modules;
- worktree clean;
- CI green.

## 9. Risks

| Risk | Mitigation |
|---|---|
| Benchmark favors local edits and hides random jumps | Use deterministic random traces, varied edit sizes, and large deletes |
| `len_lines` becomes a per-frame O(n) trap | Measure it; cache or reject the backend |
| Gap Buffer memory doubles | Record peak RSS; do not hide the capacity cost |
| Piece-table fragmentation is masked by insert/delete pairs | Use long-lived traces and measure piece count |
| Cursor/revision ownership remains ambiguous | Resolve with ADR-0010 before implementation |
| Decision criteria are invented after results | Freeze ADR-0009 first |
| Benchmark machine noise | Warmup, 30+ samples, record environment, allow invalid-run rule |
| Scope grows into all ten API methods | Keep contract changes to the two existing methods |
| Proptest fixes invalidate the benchmark | Run M2 only after M1 is stable |
| Six-week timebox expires | Drop ADR research: default cursor ownership to core and snapshot to structural validation |

## 10. Out of scope for this plan

- Plugin isolation PoC.
- Exec sandbox.
- SSRF suite.
- Gateway implementation.
- TUI first-frame measurement.
- Full ten-method API payloads.
- Lua/QuickJS host integration.

These remain Phase 0 security/contract work after the core evidence gate.

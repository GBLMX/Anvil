# ADR-0009: Text Model Snapshot Gate and Production Backend

- Status: Accepted (owner; Phase 0.5 owner-only self-approval)
- Date: 2026-10-30
- Inputs: [text-model benchmark](../benchmarks/text-model.md), ADR-0004

## Context

ADR-0004 required a benchmark before choosing a production text backend. The
benchmark now covers open, line count, viewport, random edits, RSS, and the
planned `Arc<Document>` snapshot contract.

The first benchmark pass used full `Clone` as a snapshot proxy. That was the
wrong contract: the planned snapshot is an `Arc`, which is O(1) for every
backend. The corrected measurements separately measure:

1. `Arc::clone` snapshot creation;
2. the first edit after a retained snapshot, which forces copy-on-write.

## COW design

- Snapshots are `Arc<Document>`; creation is reference-counted, not textual.
- Mutation goes through `Arc::make_mut`, which clones `PieceTable` metadata.
- `PieceTable` stores `original: Arc<str>` and inserted text as
  `Vec<Arc<str>>` chunks.
- Each `Piece` references `Source::Original` or `Source::Added(index)` plus a
  byte range.
- Insert creates a new immutable chunk; no buffer shared with a snapshot is
  mutated in place.
- Delete and split rewrite only the piece index; text chunks stay shared.
- Complexity: snapshot O(1); first edit O(piece count) plus inserted bytes;
  the base document is never copied.

## Measurements

100 MB, p99, 30 samples. The snapshot workload applies the 256-edit trace
before taking the retained snapshot:

| Backend | Open | Random edits | Arc snapshot | First edit after snapshot |
|---|---:|---:|---:|---:|
| Rope | 78.392 ms | 405.469 µs | 43 ns | 4.280 µs |
| Gap | 23.387 ms | 693.389 ms | 48 ns | 68.707 ms |
| Piece | 20.529 ms | 141.271 µs | 49 ns | 5.288 µs |

100 MB RSS after the 256-edit trace: Rope 223,512 KB; Gap 310,012 KB; Piece
207,516 KB.

## Decision

**PieceTable is the production backend**, exposed as [`TextDocument`] in
`anvil-core`. Rope remains a benchmark fallback; GapBuffer is rejected.

`anvil-core::TextDocument` aliases `PieceTable`; the application boundary uses
that name rather than a benchmark candidate.

The deciding gate is not raw edit throughput. It is:

> A retained snapshot must not force a full-document copy on the next edit.

`Arc` snapshot creation is O(1) for all three, but the first edit after a
snapshot on an already-edited document:

- Rope: 4.280 µs (clone shares storage);
- Piece: 5.288 µs (clones the piece index, shares text chunks);
- Gap: 68.707 ms (copies the backing storage).

PieceTable now passes the COW gate and also wins random-edit throughput, open
latency, and memory. Rope remains a viable fallback; GapBuffer is rejected on
both random edits and snapshot first-write.

## Consequences

Positive:

- Snapshot retention does not copy 100 MB at the next edit.
- Rope remains within the Phase 0 open/viewport/edits budgets.
- The `TextBuffer` boundary stays intact; the other backends remain available
  for benchmark comparison.

Negative:

- PieceTable retains piece and chunk indexes; compaction remains future work.
- The COW first-write cost is bounded by piece count, not document size.
- This is a Phase 0.5 backend decision, not a release API freeze.

## Sensitivity

The measured COW gate is categorical: 5.3 µs versus 4.3–68.7 ms. Weight changes
inside the 40/25/20/15 envelope do not change the gate result. A weighted edit
score alone would favor PieceTable, which is why the hard gate is recorded
separately rather than hidden in the weights.

## Sign-off

- Owner: accepted 2026-10-30
- Reviewer: not required during Phase 0.5 owner-only self-approval

M3 is complete. Production code uses `TextDocument = PieceTable`; Rope and
GapBuffer remain benchmark-only.

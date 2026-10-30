# ADR-0009: Text Model Snapshot Gate and Production Candidate

- Status: Proposed for owner + reviewer sign-off
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

## Measurements

100 MB, p99, 30 samples:

| Backend | Open | Random edits | Arc snapshot | First edit after snapshot |
|---|---:|---:|---:|---:|
| Rope | 83.680 ms | 403.709 µs | 45 ns | 724 ns |
| Gap | 27.915 ms | 705.919 ms | 45 ns | 30.956 ms |
| Piece | 19.585 ms | 146.138 µs | 105 ns | 1.769 µs |

100 MB RSS: Rope 223,092 KB; Gap/Piece 207,520 KB.

## Decision

**PieceTable is the production candidate.** Its COW `Arc<str>` sources make the
first post-snapshot edit bounded; GapBuffer remains benchmark-only.

The deciding gate is not raw edit throughput. It is:

> A retained snapshot must not force a full-document copy on the next edit.

`Arc` snapshot creation is O(1) for all three, but the first edit after a
snapshot:

- Rope: 724 ns (clone shares storage);
- Piece: 1.769 µs (clones the piece index, shares text chunks);
- Gap: 30.956 ms (copies the backing storage).

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
- This decision is a Phase 0.5 candidate selection, not a release API freeze.

## Sensitivity

The measured COW gate is categorical: 1.8 µs versus 0.7–31 ms. Weight changes
inside the 40/25/20/15 envelope do not change the gate result. A weighted edit
score alone would favor PieceTable, which is why the hard gate is recorded
separately rather than hidden in the weights.

## Sign-off

- Owner: pending
- Reviewer: pending

Until both signatures are recorded, M3 is not complete and no production code
switch is made.

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
| Rope | 75.038 ms | 448.459 µs | 41 ns | 527 ns |
| Gap | 23.451 ms | 726.075 ms | 45 ns | 72.874 ms |
| Piece | 22.054 ms | 158.107 µs | 43 ns | 15.776 ms |

100 MB RSS: Rope 223,092 KB; Gap/Piece 207,520 KB.

## Decision

**Rope is the production candidate.** PieceTable and GapBuffer remain
benchmark-only until a copy-on-write redesign proves a bounded first-write cost.

The deciding gate is not raw edit throughput. It is:

> A retained snapshot must not force a full-document copy on the next edit.

`Arc` snapshot creation is O(1) for all three, but the first edit after a
snapshot:

- Rope: 527 ns (clone shares storage);
- Piece: 15.776 ms (copies document data);
- Gap: 72.874 ms (copies the backing storage).

PieceTable wins random-edit throughput and open latency, but those wins do not
offset a 15.8 ms copy on the first post-snapshot edit. GapBuffer is rejected on
both random edits and snapshot first-write.

## Consequences

Positive:

- Snapshot retention does not copy 100 MB at the next edit.
- Rope remains within the Phase 0 open/viewport/edits budgets.
- The `TextBuffer` boundary stays intact; the other backends remain available
  for benchmark comparison.

Negative:

- Rope uses about 7% more RSS than Piece/Gap in the 100 MB probe.
- Rope open is slower than Piece (75 ms vs 22 ms), still below the 2 s budget.
- This decision is a Phase 0.5 candidate selection, not a release API freeze.

## Sensitivity

The measured COW gate is categorical: 527 ns versus 15.8–72.9 ms. Weight changes
inside the 40/25/20/15 envelope do not change the gate result. A weighted edit
score alone would favor PieceTable, which is why the hard gate is recorded
separately rather than hidden in the weights.

## Sign-off

- Owner: pending
- Reviewer: pending

Until both signatures are recorded, M3 is not complete and no production code
switch is made.

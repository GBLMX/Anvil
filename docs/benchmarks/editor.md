# Editor Benchmarks

- Date: 2026-10-31
- CPU: 12th Gen Intel(R) Core(TM) i7-12700H
- Kernel: 7.3.0-4-cachyos
- Text: 100 MB synthetic, 80-byte lines, 1,310,720 lines
- Chunks: 1,281

## Commands

```text
cargo run --release -p anvil-editor --bin editor-p99
cargo test -p anvil-editor --test editor_correctness
cargo test -p anvil-editor --test line_index_stress -- --nocapture
cargo test --release -p anvil-editor --test line_index_stress -- --ignored --nocapture
```

`A` cases use a fresh `EditorState` per run and 100 samples; `C` cases use
10,000 samples. `p99` is the 99th-percentile sample.

## A: Normal edits

| Case | p50 | p95 | p99 | Target | Result |
|---|---:|---:|---:|---:|---|
| A1 middle insert | 33.532 µs | 43.489 µs | 64.214 µs | < 50 µs | FAIL |
| A2 split insert | 49.090 µs | 59.362 µs | 78.620 µs | < 100 µs | PASS |
| A3 merge delete | 46.534 µs | 67.328 µs | 213.018 µs | < 100 µs | FAIL |
| A4 paste 10k lines | 139.692 µs | 240.575 µs | 343.861 µs | < 2 ms | PASS |
| A5 append | 0.374 µs | 0.837 µs | 2.607 µs | < 10 µs | PASS |

A1 and A3 remain above their targets. A5 now passes: append without a newline
uses the last-chunk fast path.

### A5 breakdown

Criterion, 100 MB, shared snapshot setup:

| Component | Time |
|---|---:|
| `A5_piece_append` | 63.8 ns |
| `A5_line_index_append` | 5.147 µs |
| `A5_append` (retained snapshot) | 11.407 µs |

The p99 ordinary-edit run is 0.374 µs p50 / 2.607 µs p99 because the state's
line index is not shared with a retained snapshot. The shared-snapshot case
still pays the `LineChunk` vector COW copy; this is not the ordinary append
path.

## B: Correctness

| Case | Evidence | Result |
|---|---|---|
| B1 cross-chunk delete + undo | `tests/editor_correctness.rs` text/selection equality | PASS |
| B2 10k edits + 10k undo | restored original text | PASS |
| B3 undo memory | coalesced contiguous typing, bounded record size | PASS |
| B4 cross-chunk selection mapping | offsets after insert/delete | PASS |

B1/B2/B3/B4 are `cargo test` cases, not Criterion performance cases.

## C: EditorView projection

| Case | p50 | p95 | p99 | Target | Result |
|---|---:|---:|---:|---:|---|
| C1 single caret | 25 ns | 27 ns | 27 ns | < 10 µs | PASS |
| C2 100 selections | 70 ns | 75 ns | 78 ns | < 50 µs | PASS |
| C3 1 MB single line | 25 ns | 26 ns | 26 ns | < 100 µs | PASS |

`EditorView` clones only revision/viewport/selection metadata and shared
`Arc` references; it does not touch document text.

## M4 status

M4 is **not accepted** under the stated performance gate: A1 and A3 fail
their p99 targets; A5 now passes. The implementation and correctness tests are complete, but
the flat chunk-header update model needs a follow-up optimization (or an
explicitly renegotiated acceptance threshold) before claiming M4.

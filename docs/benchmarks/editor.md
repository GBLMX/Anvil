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
| A1 middle insert | 30.947 µs | 33.513 µs | 52.714 µs | < 50 µs | FAIL |
| A2 split insert | 47.675 µs | 51.926 µs | 76.148 µs | < 100 µs | PASS |
| A3 merge delete | 43.882 µs | 76.423 µs | 217.540 µs | < 100 µs | FAIL |
| A4 paste 10k lines | 136.538 µs | 240.198 µs | 368.202 µs | < 2 ms | PASS |
| A5 append | 15.386 µs | 16.548 µs | 27.666 µs | < 10 µs | FAIL |

A1, A3, and A5 remain above their targets. The index scales with the flat chunk
header vector: `base`/`first_line` shifts cost O(chunks), and the 1,281-chunk
100 MB case cannot meet the 10 µs append target with the current representation.
This is a measured design limit, not a passing result.

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
| C1 single caret | 25 ns | 27 ns | 28 ns | < 10 µs | PASS |
| C2 100 selections | 69 ns | 74 ns | 76 ns | < 50 µs | PASS |
| C3 1 MB single line | 25 ns | 26 ns | 26 ns | < 100 µs | PASS |

`EditorView` clones only revision/viewport/selection metadata and shared
`Arc` references; it does not touch document text.

## M4 status

M4 is **not accepted** under the stated performance gate: A1, A3, and A5 fail
their p99 targets. The implementation and correctness tests are complete, but
the flat chunk-header update model needs a follow-up optimization (or an
explicitly renegotiated acceptance threshold) before claiming M4.

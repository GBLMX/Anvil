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

`A` cases use a fresh `EditorState` per run, 1,000 samples, and are pinned to
cores 0-3 with `taskset`. `C` cases use 10,000 samples. `p99` is the
99th-percentile sample.

## A: Normal edits

| Case | p50 | p95 | p99 | Target | Result |
|---|---:|---:|---:|---:|---|
| A1 middle insert | 33.673 µs | 36.114 µs | 38.371 µs | < 50 µs | PASS |
| A2 split insert | 32.238 µs | 40.109 µs | 59.036 µs | < 100 µs | PASS |
| A3 merge delete | 40.359 µs | 46.740 µs | 67.522 µs | < 100 µs | PASS |
| A4 paste 10k lines | 216.591 µs | 258.524 µs | 447.068 µs | < 2 ms | PASS |
| A5 append | 1.322 µs | 2.340 µs | 2.415 µs | < 10 µs | PASS |

A1, A2, A3, A4, and A5 all pass the locked-core 1,000-sample run. The index
shifts contiguous metadata arrays (`bases`, `first_lines`, `ends`) and
`starts` instead of AoS chunk structs. The earlier 100-sample run exposed
scheduling noise; the locked 1,000-sample run is the evidence used below.

### A5 breakdown

Criterion, 100 MB, shared snapshot setup:

| Component | Time |
|---|---:|
| `A5_piece_append` | 61.6 ns |
| `A5_line_index_append` | 5.140 µs |
| `A5_append` (retained snapshot) | 11.556 µs |

The p99 ordinary-edit run is 0.334 µs p50 / 2.245 µs p99 because the state's
line index is not shared with a retained snapshot. The shared-snapshot case
still pays the metadata-array COW copy; this is not the ordinary append path.

### A3 breakdown

Criterion, 100 MB, shared snapshot setup:

| Component | Time |
|---|---:|
| `A3_piece_delete` | 6.122 µs |
| `A3_line_index_delete` | 47.577 µs |
| `A3_merge_delete` total | 61.019 µs |

Line-index work dominates but is within the p99 gate once the SoA metadata
arrays replace AoS chunk writes. No cascade threshold change is needed.

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
| C1 single caret | 25 ns | 26 ns | 26 ns | < 10 µs | PASS |
| C2 100 selections | 70 ns | 76 ns | 79 ns | < 50 µs | PASS |
| C3 1 MB single line | 25 ns | 30 ns | 31 ns | < 100 µs | PASS |

`EditorView` clones only revision/viewport/selection metadata and shared
`Arc` references; it does not touch document text.

## M4 status

M4 is **accepted** under the locked-core, 1,000-sample performance gate. All
A and C cases pass, B1-B4 pass, and the 100k stress run passes with sampled
index consistency checks.

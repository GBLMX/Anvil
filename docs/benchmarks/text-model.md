# Text Model Benchmark: Phase 0.5 Baseline

Status: measurement report, not a final backend decision.

## Environment

```text
commit:      4db2553
rustc:       1.88.0 (6b00bc388 2025-06-23)
kernel:      7.3.0-4-cachyos
cpu:         12th Gen Intel(R) Core(TM) i7-12700H
criterion:   0.5.1
```

Command:

```bash
cargo bench --bench text_model -- --save-baseline phase0
```

The baseline is stored under Criterion's local `target/criterion` directory. It
is machine-local and must be regenerated for a new benchmark host.

## Results

Criterion reports the confidence interval around its estimate; the middle value
is shown below. These are not p99 measurements yet.

### Open

| Backend | 1 MB | 10 MB | 100 MB |
|---|---:|---:|---:|
| Rope | 364.33 µs | 5.5848 ms | 62.582 ms |
| Gap | 157.22 µs | 2.0980 ms | 21.232 ms |
| Piece | 108.83 µs | 1.2907 ms | 15.296 ms |

### `len_lines` (warm buffer, cached counts)

| Backend | 1 MB | 10 MB | 100 MB |
|---|---:|---:|---:|
| Rope | 253 ps | 288 ps | 257 ps |
| Gap | 252 ps | 257 ps | 256 ps |
| Piece | 261 ps | 256 ps | 265 ps |

### Viewport extraction (80 columns × 24 lines)

| Backend | 1 MB | 10 MB | 100 MB |
|---|---:|---:|---:|
| Rope | 380.78 ns | 380.69 ns | 385.00 ns |
| Gap | 73.321 ns | 73.487 ns | 77.676 ns |
| Piece | 43.615 ns | 45.408 ns | 43.348 ns |

### Random edit trace (256 deterministic edits)

| Backend | 1 MB | 10 MB | 100 MB |
|---|---:|---:|---:|
| Rope | 144.61 µs | 711.36 µs | 6.7014 ms |
| Gap | 2.3547 ms | 39.606 ms | 566.27 ms |
| Piece | 84.100 µs | 94.547 µs | 260.60 µs |

The criterion table is a separate harness; the p99 table below is the
regression reference.

## 100 MB RSS probe

Each backend ran in its own process:

```bash
cargo run --release -p anvil-core --bin text-rss -- <rope|gap|piece> 100
```

| Backend | State | VmRSS | VmHWM |
|---|---|---:|---:|
| Rope | open | 223,300 KB | 223,364 KB |
| Rope | edited | 223,512 KB | 223,516 KB |
| Gap | open | 207,548 KB | 207,616 KB |
| Gap | edited | 310,012 KB | 310,012 KB |
| Piece | open | 207,420 KB | 207,488 KB |
| Piece | edited | 207,516 KB | 207,516 KB |

The RSS probe includes the 100 MB input string and the constructed buffer; it
does not isolate shared runtime overhead.

## 100 MB p99 probe

```bash
TEXT_P99_MB=100 cargo run --release -p anvil-core --bin text-p99
```

| Backend | Open p99 | `len_lines` p99 | Viewport p99 | Random edits p99 |
|---|---:|---:|---:|---:|
| Rope | 78.392 ms | 31 ns | 5.084 µs | 405.469 µs |
| Gap | 23.387 ms | 26 ns | 0.969 µs | 693.389 ms |
| Piece | 20.529 ms | 25 ns | 2.310 µs | 141.271 µs |

The 100 MB run uses 30 samples; the 1 MB and 10 MB runs use 100 samples.

### Snapshot contract (100 MB, `Arc<Document>`)

| Backend | Arc snapshot p99 | First edit after snapshot p99 |
|---|---:|---:|
| Rope | 43 ns | 4.280 µs |
| Gap | 48 ns | 68.707 ms |
| Piece | 49 ns | 5.288 µs |

Snapshot creation is O(1) for every backend through `Arc`. The snapshot probe
applies the 256-edit trace before taking the snapshot, so the first-edit cost
includes the real edited piece index. PieceTable stores its original and
inserted text as `Arc` chunks, so its first post-snapshot edit no longer copies
the document. Rope also remains O(1); Gap copies its storage.

100 MB RSS after the same 256-edit trace: Rope 223,512 KB; Gap 310,012 KB;
Piece 207,516 KB.

The earlier full-`Clone` measurement was a proxy and is not used for the
snapshot hard gate.

The PR gate reruns the 10 MB probe against `origin/main` in a second worktree
on the same runner and fails if any workload's p99 regresses by more than 10%;
workloads with a baseline p99 below 10 µs are skipped as timer noise.
The 100 MB run remains a manual reference measurement.

## Interpretation limits

- The p99 probe is a separate timing loop, not Criterion's confidence
  interval. Its 100 MB sample count is 30; use a larger run for a final claim.
- The random trace is deterministic but synthetic; it does not model a real
  user's edit distribution.
- Undo/redo and snapshot costs are deferred until M4 creates the command/actor
  layers.
- This data does not select a production backend.

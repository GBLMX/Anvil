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
| Rope | 144.61 µs | 711.36 µs | 8.9701 ms |
| Gap | 2.3547 ms | 39.606 ms | 567.19 ms |
| Piece | 84.100 µs | 94.547 µs | 341.50 µs |

## 100 MB RSS probe

Each backend ran in its own process:

```bash
cargo run --release -p anvil-core --bin text-rss -- <rope|gap|piece> 100
```

| Backend | VmRSS | VmHWM |
|---|---:|---:|
| Rope | 223,092 KB | 223,156 KB |
| Gap | 207,528 KB | 207,580 KB |
| Piece | 207,520 KB | 207,580 KB |

The RSS probe includes the 100 MB input string and the constructed buffer; it
does not isolate shared runtime overhead.

## 100 MB p99 probe

```bash
TEXT_P99_MB=100 cargo run --release -p anvil-core --bin text-p99
```

| Backend | Open p99 | `len_lines` p99 | Viewport p99 | Random edits p99 |
|---|---:|---:|---:|---:|
| Rope | 126.058 ms | 48 ns | 8.721 µs | 696.619 µs |
| Gap | 33.341 ms | 51 ns | 1.669 µs | 959.948 ms |
| Piece | 27.616 ms | 58 ns | 2.335 µs | 219.029 µs |

The 100 MB run uses 30 samples; the 1 MB and 10 MB runs use 100 samples.

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

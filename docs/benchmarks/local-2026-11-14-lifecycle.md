# Local Lifecycle Re-run — 2026-11-14

Status: local workstation run. This is **not** the Ryzen 7 5800X reference
machine and does not satisfy the reference-machine exit gate. It records the
post-lifecycle measurement so the local baseline can detect regressions.

## Machine

- CPU: 12th Gen Intel(R) Core(TM) i7-12700H, 20 logical CPUs
- OS: Linux 7.3.4-1-cachyos
- Build: release benchmark profile
- Filesystem: local NVMe

## TUI edit loop

Command:

```bash
taskset -c 0-3 cargo bench -p anvil-tui --bench e2e_edit --features e2e-bench
```

The benchmark now uses 30 samples for every latency percentile.

### Native key to screen

| Run | p50 | p95 | p99 | n |
|---|---:|---:|---:|---:|
| 2026-11-13 baseline | 98.9 µs | 101.6 µs | 101.6 µs | 20 |
| 2026-11-14 pinned #1 | 98.5 µs | 106.8 µs | 109.9 µs | 30 |
| 2026-11-14 pinned #2 | 98.1 µs | 104.6 µs | 105.4 µs | 30 |

Pinned p99 is 3.7–8.2% above baseline, below the 10% regression rule. Unpinned
runs reached 118.9–128.0 µs p99, but the tail was not reproducible with CPU
affinity fixed.

### PTY key to screen

| Run | p50 | p95 | p99 | n |
|---|---:|---:|---:|---:|
| 2026-11-13 baseline | 1,214.8 µs | 1,563.5 µs | 1,563.5 µs | 20 |
| 2026-11-14 pinned #1 | 203.8 µs | 327.1 µs | 382.3 µs | 30 |
| 2026-11-14 pinned #2 | 306.2 µs | 888.4 µs | 962.2 µs | 30 |

The PTY path remains faster than baseline. Run-to-run variance is high because
the benchmark measures full terminal output readiness.

### Other scenarios

Pinned run #2:

| Scenario | p50 | p95 | p99 | n |
|---|---:|---:|---:|---:|
| native open 100 MB, first frame | 112.769 ms | 112.769 ms | 112.769 ms | 1 |
| native scroll 100 MB | 107.3 µs | 124.6 µs | 126.4 µs | 30 |
| native search 100 MB, first | 21.188 ms | 21.188 ms | 21.188 ms | 1 |
| native search 100 MB | 21.951 ms | 22.617 ms | 22.617 ms | 10 |
| PTY open 100 MB, first frame | 211.026 ms | 211.026 ms | 211.026 ms | 1 |
| PTY scroll 100 MB | 290.9 µs | 633.4 µs | 670.1 µs | 30 |
| PTY search 100 MB, first | 19.628 ms | 19.628 ms | 19.628 ms | 1 |
| PTY search 100 MB | 19.854 ms | 22.145 ms | 22.145 ms | 10 |

All non-key scenarios are at or below the 2026-11-13 local baseline. Native
key p50 is unchanged, while p95 and p99 are 3–8% above baseline. This file is
not reference-machine gate evidence.

# Reference Performance Baseline — 2026-11-20

Status: authoritative local reference baseline for the i7-12700H workstation.
It covers the core text model, editor, line index, TUI, and Go gateway. Plugin
first-frame and hot-reload measurements remain manual.

Absolute thresholds apply to this machine. Do not compare absolute numbers
across different host hardware; use relative CI thresholds elsewhere.

## Machine

- CPU: Intel Core i7-12700H, 6 P-cores + 8 E-cores, 20 threads
- RAM: 15 GiB
- OS: Arch Linux, kernel 7.3.4-1-cachyos
- CPU affinity: P-cores `0-11`; E-cores `12-19` excluded
- Governor: `powersave` (not switched; documented limitation)
- Terminal: Kitty with keyboard protocol enabled
- Build: release, LTO, stripped; no debug assertions
- Filesystem: local NVMe
- Commit: `97dfa6d`; rustc 1.88.0; Go 1.26.11

Commands:

```bash
taskset -c 0-11 cargo bench -p anvil-text-bench --bench text_model
taskset -c 0-11 cargo bench -p anvil-core --bench line_index
TEXT_EDITOR_EDIT_SAMPLES=1000 taskset -c 0-3 \
  cargo run --release -p anvil-editor --bin editor-p99
cargo test -p anvil-editor --test line_index_stress -- --nocapture
cargo test --release -p anvil-editor --test line_index_stress -- --ignored --nocapture
taskset -c 0-3 cargo bench -p anvil-tui --bench e2e_edit --features e2e-bench
go test -run '^$' -bench . -benchmem \
  ./go/internal/fetch ./go/internal/gateway ./go/internal/execrunner
```

## Core text model

Criterion mean estimates. The 100 MB cases use Criterion's limited sample
count; these are means, not p99 values.

| Backend | Size | Open | `len_lines` | Viewport | Arc snapshot | First edit | Random edits |
|---|---:|---:|---:|---:|---:|---:|---:|
| rope | 1MB | 360.78 µs | 216.23 ps | 338.88 ns | 9.2519 ns | 32.518 µs | 140.85 µs |
| rope | 10MB | 4.0544 ms | 216.03 ps | 353.56 ns | 9.2612 ns | 318.30 µs | 473.13 µs |
| rope | 100MB | 59.764 ms | 216.00 ps | 349.04 ns | 9.2531 ns | 3.7462 ms | 4.0238 ms |
| gap | 1MB | 157.85 µs | 220.17 ps | 69.247 ns | 9.2421 ns | 549.45 µs | 2.3020 ms |
| gap | 10MB | 2.0172 ms | 216.26 ps | 75.265 ns | 9.2462 ns | 3.3645 ms | 37.976 ms |
| gap | 100MB | 21.023 ms | 216.06 ps | 68.403 ns | 9.2683 ns | 26.393 ms | 560.41 ms |
| piece | 1MB | 107.14 µs | 215.48 ps | 39.798 ns | 9.2612 ns | 4.8087 µs | 50.693 µs |
| piece | 10MB | 1.1886 ms | 216.00 ps | 39.554 ns | 9.2861 ns | 4.8314 µs | 56.732 µs |
| piece | 100MB | 14.387 ms | 216.56 ps | 40.050 ns | 9.3619 ns | 203.39 µs | 277.97 µs |

## Line index

| Scenario | Mean |
|---|---:|
| 100 MB build | 19.346 ms |
| 100 MB `offset_to_line` | 28.739 ns |
| 100 MB `line_to_offset` | 12.463 ns |
| 100 MB small insert | 61.620 µs |

Stress:

- 10k random edits passed; `offset-p99-ns=1169`.
- 100k random edits passed in 21.60 s; `offset-p99-ns=645`.

## Editor p99

`editor-p99`, 1000 samples:

| Case | p50 | p95 | p99 |
|---|---:|---:|---:|
| `A1_middle_insert` | 1,386 ns | 1,829 ns | 1,879 ns |
| `A2_split_insert` | 26,640 ns | 31,108 ns | 35,593 ns |
| `A3_merge_delete` | 65,179 ns | 79,550 ns | 93,208 ns |
| `A4_paste_10k_lines` | 135,778 ns | 168,591 ns | 253,542 ns |
| `A5_append` | 105 ns | 144 ns | 466 ns |
| `C1_single_caret_view` | 15 ns | 16 ns | 17 ns |
| `C2_100_selections_view` | 60 ns | 66 ns | 77 ns |
| `C3_long_line_view` | 15 ns | 16 ns | 16 ns |

## TUI

| Path | Scenario | p50 | p95 | p99 | n |
|---|---|---:|---:|---:|---:|
| rust | key-to-screen | 107.6 µs | 118.5 µs | 119.6 µs | 30 |
| rust | 100 MB first frame | 114.1 ms | — | — | 1 |
| rust | 100 MB scroll | 118.6 µs | 125.3 µs | 138.9 µs | 30 |
| rust | 100 MB first search | 22.0 ms | — | — | 1 |
| rust | 100 MB search | 20.9 ms | 22.7 ms | 22.7 ms | 10 |
| pty | 100 MB first frame | 189.9 ms | — | — | 1 |
| pty | key-to-screen | 146.0 µs | 244.6 µs | 293.6 µs | 30 |
| pty | 100 MB scroll | 139.8 µs | 441.6 µs | 488.1 µs | 30 |
| pty | 100 MB first search | 21.9 ms | — | — | 1 |
| pty | 100 MB search | 23.0 ms | 26.5 ms | 26.5 ms | 10 |

## Go gateway

| Benchmark | ns/op | B/op | allocs/op |
|---|---:|---:|---:|
| `BenchmarkFetchHTTP` | 298,606 | 34,973 | 161 |
| `BenchmarkPermissionCheck` | 12.90 | 0 | 0 |
| `BenchmarkFrameRoundTrip` | 151.1 | 184 | 5 |
| `BenchmarkHello` | 1,566 | 952 | 17 |
| `BenchmarkReadFileChunk` | 1,242,696 | 5,767,299 | 4 |
| `BenchmarkReadResponseEncoding` | 12,178,320 | 35,370,635 | 14 |
| `BenchmarkBase64Encode` | 5,208,485 | 15,384,581 | 2 |
| `BenchmarkResponseJSON` | 5,272,982 | 7,766,570 | 4 |
| `BenchmarkRunTrue` | 341,448 | 68,501 | 62 |

`BenchmarkReadFileChunk` measured 4640.85 MB/s; `BenchmarkReadResponseEncoding`
measured 473.56 MB/s.

## Gate status

- Reference-machine baseline: established above.
- Rerun at the dependency and release boundaries documented in
  [performance.md](../performance.md).

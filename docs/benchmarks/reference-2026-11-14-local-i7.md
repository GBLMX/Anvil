# Temporary Reference Baseline — 2026-11-14

Status: first automated local baseline after designating the i7-12700H
workstation as the temporary reference machine. It covers the core text model,
editor, line index, TUI, and Go gateway. Plugin first-frame and hot-reload
measurements remain manual. The file must be replaced, not compared across
machines, when the planned 64 GiB host arrives.

## Machine

- CPU: Intel Core i7-12700H, 6 P-cores + 8 E-cores, 20 threads
- RAM: 15 GiB
- OS: Arch Linux, kernel 7.3.4-1-cachyos
- CPU affinity: P-cores `0-11`; E-cores `12-19` excluded
- Governor: `powersave` (documented limitation; not switched)
- Terminal: Kitty with keyboard protocol enabled
- Build: release, LTO, stripped; no debug assertions
- Filesystem: local NVMe
- Commit: 4f339f7; rustc 1.88.0; Go 1.26.11

Commands:

```bash
taskset -c 0-11 cargo bench -p anvil-core --bench text_model
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
| rope | 1MB | 348.182 µs | 223.6 ps | 359.00 ns | 9.25 ns | 32.721 µs | 140.012 µs |
| rope | 10MB | 4.072 ms | 223.0 ps | 351.83 ns | 9.39 ns | 321.957 µs | 492.249 µs |
| rope | 100MB | 61.251 ms | 222.5 ps | 372.50 ns | 9.35 ns | 4.071 ms | 4.439 ms |
| gap | 1MB | 155.502 µs | 218.0 ps | 68.54 ns | 9.31 ns | 560.667 µs | 2.295 ms |
| gap | 10MB | 2.050 ms | 223.7 ps | 73.45 ns | 9.31 ns | 3.385 ms | 39.846 ms |
| gap | 100MB | 21.321 ms | 217.1 ps | 69.64 ns | 9.26 ns | 26.649 ms | 570.693 ms |
| piece | 1MB | 105.587 µs | 225.4 ps | 39.39 ns | 9.30 ns | 5.014 µs | 87.571 µs |
| piece | 10MB | 1.301 ms | 225.3 ps | 40.76 ns | 9.35 ns | 5.639 µs | 99.996 µs |
| piece | 100MB | 15.695 ms | 223.3 ps | 45.40 ns | 9.23 ns | 203.282 µs | 312.079 µs |

## Line index

| Scenario | Mean |
|---|---:|
| 100 MB build | 19.905 ms |
| 100 MB `offset_to_line` | 28.509 ns |
| 100 MB `line_to_offset` | 12.368 ns |
| 100 MB small insert | 69.527 µs |

Stress:

- 10k random edits passed; `offset-p99-ns=3089`.
- 100k random edits passed in 38.20 s.

## Editor p99

`editor-p99`, 1000 samples:

| Case | p50 | p95 | p99 |
|---|---:|---:|---:|
| `A1_middle_insert` | 1,156 ns | 1,620 ns | 1,812 ns |
| `A2_split_insert` | 26,953 ns | 32,448 ns | 53,099 ns |
| `A3_merge_delete` | 56,194 ns | 65,176 ns | 85,095 ns |
| `A4_paste_10k_lines` | 152,791 ns | 187,295 ns | 390,146 ns |
| `A5_append` | 765 ns | 1,312 ns | 1,376 ns |
| `C1_single_caret_view` | 16 ns | 17 ns | 17 ns |
| `C2_100_selections_view` | 77 ns | 83 ns | 89 ns |
| `C3_long_line_view` | 15 ns | 16 ns | 16 ns |

## TUI edit loop

Pinned 30-sample run:

| Layer | Scenario | p50 | p95 | p99 | n |
|---|---|---:|---:|---:|---:|
| Rust | key to screen | 98.1 µs | 104.6 µs | 105.4 µs | 30 |
| Rust | open 100 MB, first frame | 112.769 ms | 112.769 ms | 112.769 ms | 1 |
| Rust | scroll 100 MB | 107.3 µs | 124.6 µs | 126.4 µs | 30 |
| Rust | first `/` search, 100 MB | 21.188 ms | 21.188 ms | 21.188 ms | 1 |
| Rust | subsequent `/` search, 100 MB | 21.951 ms | 22.617 ms | 22.617 ms | 10 |
| PTY + gateway | open 100 MB, first frame | 211.026 ms | 211.026 ms | 211.026 ms | 1 |
| PTY + gateway | key to screen | 306.2 µs | 888.4 µs | 962.2 µs | 30 |
| PTY + gateway | scroll 100 MB | 290.9 µs | 633.4 µs | 670.1 µs | 30 |
| PTY + gateway | first `/` search, 100 MB | 19.628 ms | 19.628 ms | 19.628 ms | 1 |
| PTY + gateway | subsequent `/` search, 100 MB | 19.854 ms | 22.145 ms | 22.145 ms | 10 |

The native key p99 is 3.7–8.2% above the pre-switch 2026-11-13 local run and
remains below the 10% regression threshold.

## Go gateway

| Benchmark | ns/op | B/op | allocs/op |
|---|---:|---:|---:|
| `BenchmarkFetchHTTP` | 240,745 | 35,013 | 161 |
| `BenchmarkPermissionCheck` | 12.92 | 0 | 0 |
| `BenchmarkFrameRoundTrip` | 163.9 | 184 | 5 |
| `BenchmarkHello` | 1,477 | 936 | 17 |
| `BenchmarkReadFileChunk` | 1,106,397 | 5,767,299 | 4 |
| `BenchmarkReadResponseEncoding` | 12,377,368 | 34,486,299 | 13 |
| `BenchmarkBase64Encode` | 4,793,607 | 15,384,593 | 2 |
| `BenchmarkResponseJSON` | 5,317,353 | 7,764,201 | 4 |
| `BenchmarkRunTrue` | 333,495 | 68,505 | 62 |

## Gate status

- Temporary reference baseline: established above.
- Absolute thresholds: evaluate against this file until the 64 GiB host arrives.
- 64 GiB replacement: re-run everything and replace this file; do not compare
  absolute numbers across the two machines.

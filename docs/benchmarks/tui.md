# TUI edit-loop latency

Measured 2026-10-31 on:

- Intel Core i7-12700H, 20 logical CPUs, Linux 7.3.0-4-cachyos.
- `rustc 1.88.0`, release benchmark and release `anvil` binary.
- Terminal model 120x40; local PTY, no terminal emulator in the loop.
- 100 MB fixture: 8-digit line labels plus ten search markers at EOF.

## Method

Rust-side numbers are `Model::dispatch` + `ensure_cursor_visible` +
`Terminal<TestBackend>::draw`. They exclude terminal output, kernel PTY
buffers, and terminal parsing. The benchmark uses the release profile.

PTY numbers use `forkpty`, launch `target/release/anvil` with `--workspace`,
and stop at the first output byte after the input. The TUI starts its sibling
`target/release/anvil-gateway`; the numbers therefore include gateway startup,
chunked UDS transfer, and `anvil`'s update + draw, but not a real terminal
emulator's parsing or painting. PTY search measurements drain the typed query
before starting the timer, so they measure Enter-to-first-output, not query
echo. The first search is reported separately from the following ten searches.

Run:

cargo bench -p anvil-tui --bench e2e_edit --features e2e-bench
```

Build the gateway sibling before the PTY run:

```text
go build -o target/release/anvil-gateway ./go/cmd/anvil-gateway
```

## Results (direct `std::fs`, pre-gateway)

| Layer | Scenario | p50 | p95 | p99 | n |
|---|---|---:|---:|---:|---:|
| Rust | key to screen, 1 MB | 76.5 µs | 79.6 µs | 79.6 µs | 20 |
| Rust | open 100 MB, first frame | 95.3 ms | 95.3 ms | 95.3 ms | 1 |
| Rust | scroll 100 MB, one `j` | 74.4 µs | 104.0 µs | 104.0 µs | 20 |
| Rust | first `/` search, 100 MB | 210.7 ms | 210.7 ms | 210.7 ms | 1 |
| Rust | subsequent `/` search, 100 MB | 213.7 ms | 229.8 ms | 229.8 ms | 10 |
| PTY | open 100 MB, first frame | 209.6 ms | 209.6 ms | 209.6 ms | 1 |
| PTY | key to first output, 1 MB | 223.3 µs | 508.9 µs | 508.9 µs | 20 |
| PTY | scroll 100 MB, one `j` | 104.4 µs | 367.1 µs | 367.1 µs | 20 |
| PTY | first `/` search, 100 MB | 219.0 ms | 219.0 ms | 219.0 ms | 1 |
| PTY | subsequent `/` search, 100 MB | 221.3 ms | 239.1 ms | 239.1 ms | 10 |

The Rust open row is post-parallel-index; the PTY row is pre-parallel and is
kept as the historical direct baseline.

## Gateway path (2026-10-31)

Same 100 MB fixture, release TUI, sibling release `anvil-gateway`, binary
frames with 7 MiB chunks (15 chunks / 104,857,596 bytes):

| Layer | Scenario | p50 | p95 | p99 | n |
|---|---|---:|---:|---:|---:|
| Rust | open 100 MB, first frame | 95.3 ms | 95.3 ms | 95.3 ms | 1 |
| PTY + gateway | open 100 MB, first frame | 171.1 ms | 171.1 ms | 171.1 ms | 1 |
| PTY + gateway | key to first output, 1 MB | 160.6 µs | 277.1 µs | 277.1 µs | 20 |
| PTY + gateway | scroll 100 MB, one `j` | 255.2 µs | 542.6 µs | 542.6 µs | 20 |
| PTY + gateway | first `/` search, 100 MB | 221.2 ms | 221.2 ms | 221.2 ms | 1 |
| PTY + gateway | subsequent `/` search, 100 MB | 234.1 ms | 252.6 ms | 252.6 ms | 10 |
| Gateway | per 7 MiB binary chunk | 12.6 ms avg | — | — | 15 |

Gateway ready (spawn + handshake): 3.1 ms. Serial binary UDS loop: 188.9 ms.
For comparison, the previous JSON/base64 5.5 MiB path was 563.1 ms PTY with
19 chunks.

### Previous JSON path (superseded for reads)

Server-side stage benchmark (page-cache warm, per chunk):

| Stage | ms |
|---|---:|
| `ReadFileAt` | 0.84 |
| base64 encode | 4.98 |
| JSON encode | 5.33 |
| full response encode | 11.8 |

`ReadFileAt` moves 5.5 MiB in 0.84 ms, about **6.5 GB/s** (the Go benchmark
reports 6,847 MB/s); file I/O is not the bottleneck.

Client-side decode benchmark (Rust, per 5.5 MiB response):

| Stage | ms |
|---|---:|
| JSON decode | 1.17 |
| base64 decode | 2.09 |
| append to `String` | 0.31 |
| full client decode | 5.88 |
| UDS frame read | 1.46 |

Client decode is about **5.9 ms/chunk**, not 10 ms; 19 chunks are about
**112 ms**. The server encode is about **224 ms**. The remaining observed
time is socket/startup/handler overhead, not the line-index work alone.

The 4 MiB run was **732.0 ms PTY**, 25 chunks, 21.7 ms average, 661.3 ms
total. Raising to 5.5 MiB removed 92 ms but did not close the gap.

Pipelining measurement (real gateway, binary frames, 7 MiB chunks, UDS client
without document decode, page-cache warm):

| In-flight | Total | Per chunk |
|---:|---:|---:|
| 1 | 188.9 ms | 12.6 ms |
| 2 | 135.1 ms | 9.0 ms |
| 4 | 126.8 ms | 8.5 ms |
| 8 | **124.8 ms** | **8.3 ms** |
| 15 | 138.3 ms | 9.2 ms |

An 8-request window saves **64 ms** against window 1 in the pure UDS loop.
The TUI uses a **4-request** window; its measured end-to-end PTY open is
**171.1 ms** (including decode, index, rendering, and gateway startup), which
meets the 200 ms Phase 0.6 target. Window 8 reaches 168.4 ms but raises gateway
memory sharply; window 4 is the better tradeoff.

Worker-count check on the previous JSON path (same fixture, direct Rust /
gateway PTY):

| Workers | Rust open | Gateway PTY |
|---:|---:|---:|
| 16 | **96.1 ms** | 563.1 ms |
| 8 | 103.0 ms | **560.7 ms** |
| 4 | 110.4 ms | 597.3 ms |

Eight workers do not produce a 30–50 ms gateway win; the 16-worker gateway
result is within ~2.4 ms of eight. The earlier 74 ms/40 ms discrepancy was
run-to-run variance, not a large contention effect. Keep the 16-worker default
with `ANVIL_LINE_SCAN_WORKERS` as an override.

Worker ranges are half-open and split on bytes. A `\n` byte cannot occur inside
a UTF-8 continuation byte, and a newline at a range boundary belongs to exactly
one range (`[start, end)`), so the two-pass count cannot double-count or miss
it. The 3,000,000-line parallel/serial equivalence test covers the merge path.

## Open phase breakdown (instrumented)

One instrumented 100 MB run (`ANVIL_OPEN_TIMING=1`) after the 5-run median:

| Phase | Time |
|---|---:|
| gateway start + model setup, before read | ~21 ms |
| `sys.stat` + 4-way binary read | **79.8 ms** |
| `EditorState::from_text` (line index) | **61.6 ms** |
| terminal + first frame | 0.4 ms |
| total to first frame | **163.3 ms** |

The 171.1 ms median includes run-to-run variance. The line index alone is
61.6 ms, so the 50 ms target is unreachable for 100 MB with a full index even
if transfer becomes free. 200 ms is the Phase 0.6 target; 50 ms remains a
long-term target requiring delayed/partial indexing, not just fd passing.
## Interpretation

- Direct Rust 100 MB open is **95.3 ms** after parallel line scanning.
  The 4-way binary gateway path is **171.1 ms PTY**, meeting the 200 ms Phase
  0.6 open target. Serial binary UDS was 188.9 ms for 15 chunks.
- The previous JSON/base64 path was 563.1 ms PTY and 19 chunks. It has been
  replaced for reads; its server encode cost (11.8 ms/chunk) and client decode
  cost (5.88 ms/chunk) no longer apply to open.
- Binary pipelining is wired into the TUI at window 4. The pure UDS loop drops
  from 188.9 ms to 126.8 ms at window 4; measured TUI end-to-end is 171.1 ms.
  Window 8 is slightly faster (124.8 ms UDS / 168.4 ms TUI) but gateway VmHWM
  rises to ~199 MB, so window 4 is the selected tradeoff.
- The 50 ms open target needs both delayed/partial indexing and a zero-copy
  transfer path; the current binary frame path is not close.
- Parallel line scanning remains necessary; it cut direct Rust open from
  174 ms to 89–96 ms across runs.
- Search remains dominated by the in-process scan, so the gateway barely
  changes it. Key-to-screen and scroll are within local PTY noise.
- These numbers include the Go gateway, but not LSP, terminal emulator, or idle
  CPU.

## Memory (gateway path, 100 MB)

Measured with `/proc/<pid>/status` after the first 100 MB frame:

| Process | VmRSS | VmHWM |
|---|---:|---:|
| TUI | 198 MB | 300 MB |
| `anvil-gateway` child | 66 MB | 91 MB |

The TUI footprint is unchanged from the direct path: the peak includes the
source `String`, the `PieceTable` copy, and the line index. The binary gateway
adds about 66 MB steady and 91 MB peak for four in-flight raw chunks plus
framing. The combined steady footprint is about 264 MB.

The two-pass parallel line scan keeps memory flat: it allocates the final
line-start array once and writes disjoint per-worker slices, instead of keeping
per-worker arrays and a second merged array.

A `line_starts` preallocation experiment alone did not change VmHWM. The win
came from building `LineIndex` directly from the source `&str` instead of
calling `TextBuffer::slice` through `EditorState::new`: that removes one
temporary 100 MB copy. The peak includes the source `String`, the
`PieceTable` copy, and the absolute/relative line-start arrays.

The chunked gateway protocol bounds the server-side transient to one chunk
(7 MiB raw binary for reads; JSON/base64 writes are 5.5 MiB). The editor still
assembles the complete text and line index, so its steady-state footprint is
unchanged; memory is not reduced by chunking. `sys.write_file` streams into a
temporary file rather than accumulating the whole document.

## Boundaries

- The Go gateway is included in PTY + gateway and per-chunk rows, but not in
  Rust-side direct rows. LSP and syntax highlighting are not in this loop. The
  76.5 µs key-to-screen number is the core edit/render path before Tree-sitter
  or span styling is added.
- Tree-sitter highlighting will add highlight computation and styled span
  generation on the draw path. The core edit path should remain in the same
  microsecond range, but the full key-to-paint number will grow.
- The PTY layer stops at the first output byte and does not include a real
  terminal emulator's parsing or painting. A real terminal will add roughly
  2–5 ms on top of the local PTY measurement.
- Search is Enter-triggered linear scanning, not live incremental highlighting.
  Live search highlighting needs an incremental index and is a separate
  engineering effort.

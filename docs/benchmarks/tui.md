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
| Rust | open 100 MB, first frame | 96.1 ms | 96.1 ms | 96.1 ms | 1 |
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

Same 100 MB fixture, release TUI, sibling release `anvil-gateway`, 5.5 MiB
chunks (19 chunks / 104,857,596 bytes):

| Layer | Scenario | p50 | p95 | p99 | n |
|---|---|---:|---:|---:|---:|
| Rust | open 100 MB, first frame | 96.1 ms | 96.1 ms | 96.1 ms | 1 |
| PTY + gateway | open 100 MB, first frame | 563.1 ms | 563.1 ms | 563.1 ms | 1 |
| PTY + gateway | key to first output, 1 MB | 550.5 µs | 690.8 µs | 690.8 µs | 20 |
| PTY + gateway | scroll 100 MB, one `j` | 342.6 µs | 487.4 µs | 487.4 µs | 20 |
| PTY + gateway | first `/` search, 100 MB | 262.1 ms | 262.1 ms | 262.1 ms | 1 |
| PTY + gateway | subsequent `/` search, 100 MB | 229.9 ms | 261.6 ms | 261.6 ms | 10 |
| Gateway | per 5.5 MiB read chunk | 27.7 ms avg | 48.6 ms max | 5.3 ms min | 19 |

Gateway ready (spawn + handshake): 3.13 ms. Total UDS read loop: 637.4 ms.

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

Pipelining measurement (real gateway, 5.5 MiB chunks, UDS client without
document decode, page-cache warm):

| In-flight | Total | Per chunk |
|---:|---:|---:|
| 1 | 591.8 ms | 31.1 ms |
| 2 | 372.0 ms | 19.6 ms |
| 4 | 389.0 ms | 20.5 ms |
| 8 | **335.3 ms** | **17.6 ms** |
| 19 | 376.6 ms | 19.8 ms |

An 8-request window saves **256 ms** against window 1 in the same run. It is
not only a 28 ms socket optimization; the server encode overlaps across cores.
Projected PTY open is roughly **307 ms** before client decode is included.

Worker-count check for CPU competition (same fixture, direct Rust / gateway PTY):

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

## Interpretation

- Direct Rust 100 MB open is **96.1 ms** after parallel line scanning;
  the pre-parallel direct PTY baseline was 210 ms. The gateway path is **563 ms
  PTY**, about **+353 ms** over that older direct PTY number. It **does not
  meet the Phase 0.6 open target of 200 ms**. The 50 ms open number is a
  long-term target, not the Phase 0.6 blocker.
- The server encode path, not file I/O, is the dominant added cost. Per 5.5
  MiB chunk: `ReadFileAt` is 0.84 ms, base64 is 4.98 ms, JSON is 5.33 ms, and
  the full response encode is 11.8 ms. The file is page-cache warm.
- The 5.5 MiB chunk is real and fits the 8 MiB frame; the 4 MiB run was 732 ms
  and 5.5 MiB is 563 ms. But 7 MiB raw exceeds the frame after base64, and the
  practical ceiling is about 5.9 MiB. The remaining gap is not round-trip
  count alone.
- Pipelining is the cheapest measured improvement: window 8 takes the UDS loop
  from 591.8 ms to 335.3 ms in the same run, saving 256 ms. It still leaves a
  projected ~307 ms PTY open, above the 200 ms Phase 0.6 target.
- Binary framing removes the measured base64 and JSON work: about 10.3 ms
  server + 3.3 ms client per chunk, or ~258 ms total. That projects to roughly
  **305 ms PTY**, not 240 ms. fd passing removes the whole 19.2 ms/chunk
  transfer path and is the only option that can approach the direct baseline.
  The 200 ms target still needs binary framing and pipelining; parallel
  line-index construction is implemented.
- Search remains dominated by the in-process scan, so the gateway barely
  changes it. Key-to-screen and scroll are within local PTY noise.
- These numbers include the Go gateway, but not LSP, terminal emulator, or idle
  CPU.

## Memory (gateway path, 100 MB)

Measured with `/proc/<pid>/status` after the first 100 MB frame:

| Process | VmRSS | VmHWM |
|---|---:|---:|
| TUI | 198 MB | 301 MB |
| `anvil-gateway` child | 58 MB | 63 MB |

The TUI footprint is unchanged from the direct path: the peak includes the
source `String`, the `PieceTable` copy, and the line index. The gateway adds
about 58 MB steady and 63 MB peak for 5.5 MiB chunk, base64, and JSON buffers.
The combined steady footprint is about 255 MB.

The two-pass parallel line scan keeps memory flat: it allocates the final
line-start array once and writes disjoint per-worker slices, instead of keeping
per-worker arrays and a second merged array.

A `line_starts` preallocation experiment alone did not change VmHWM. The win
came from building `LineIndex` directly from the source `&str` instead of
calling `TextBuffer::slice` through `EditorState::new`: that removes one
temporary 100 MB copy. The peak includes the source `String`, the
`PieceTable` copy, and the absolute/relative line-start arrays.

The chunked gateway protocol bounds the server-side transient to one chunk
(5.5 MiB raw, about 7.3 MiB base64 plus JSON overhead). The editor still
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

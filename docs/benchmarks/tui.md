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

PTY numbers use `forkpty`, launch `target/release/anvil`, and stop at the
first output byte after the input. They include process scheduling, PTY
transport, and `anvil`'s update + draw, but not a real terminal emulator's
parsing or painting. PTY search measurements drain the typed query before
starting the timer, so they measure Enter-to-first-output, not query echo.
The first search is reported separately from the following ten searches.

Run:

cargo bench -p anvil-tui --bench e2e_edit --features e2e-bench
```

## Results

| Layer | Scenario | p50 | p95 | p99 | n |
|---|---|---:|---:|---:|---:|
| Rust | key to screen, 1 MB | 76.5 µs | 79.6 µs | 79.6 µs | 20 |
| Rust | open 100 MB, first frame | 174.1 ms | 174.1 ms | 174.1 ms | 1 |
| Rust | scroll 100 MB, one `j` | 74.4 µs | 104.0 µs | 104.0 µs | 20 |
| Rust | first `/` search, 100 MB | 210.7 ms | 210.7 ms | 210.7 ms | 1 |
| Rust | subsequent `/` search, 100 MB | 213.7 ms | 229.8 ms | 229.8 ms | 10 |
| PTY | open 100 MB, first frame | 209.6 ms | 209.6 ms | 209.6 ms | 1 |
| PTY | key to first output, 1 MB | 223.3 µs | 508.9 µs | 508.9 µs | 20 |
| PTY | scroll 100 MB, one `j` | 104.4 µs | 367.1 µs | 367.1 µs | 20 |
| PTY | first `/` search, 100 MB | 219.0 ms | 219.0 ms | 219.0 ms | 1 |
| PTY | subsequent `/` search, 100 MB | 221.3 ms | 239.1 ms | 239.1 ms | 10 |

## Interpretation

- The 100 MB open is dominated by building the line index and first frame:
  174 ms Rust-side, 210 ms including PTY startup.
- Scrolling is cheap: about 74 µs Rust-side and 104 µs to first PTY output.
- Search is linear in document size: about 200 ms for 100 MB. PTY overhead is
  negligible when the search scan dominates.
- PTY overhead is **not** a fixed 2–8 ms on this machine: key-to-first-output
  adds roughly 147 µs at p50 and 429 µs at p95 over the Rust-side number.
  A real terminal emulator will add its own parsing and painting cost on top.
- These numbers exclude the Go gateway, LSP, terminal emulator, and idle CPU.

## Memory (direct std::fs, not gateway)

Measured with `/proc/<pid>/status` after the first 100 MB frame:

| Metric | Value |
|---|---:|
| VmRSS after first frame | 198 MB |
| VmHWM (peak) | 300 MB |

The peak includes the source `String`, the `PieceTable` copy, and the line
index. The steady RSS is lower after the source string is dropped, but the
text and index remain resident. This was measured before gateway integration,
so it does not include UDS buffers or base64 decode buffers.

A `line_starts` preallocation experiment alone did not change VmHWM. The win
came from building `LineIndex` directly from the source `&str` instead of
calling `TextBuffer::slice` through `EditorState::new`: that removes one
temporary 100 MB copy. The peak includes the source `String`, the
`PieceTable` copy, and the absolute/relative line-start arrays.

The chunked gateway protocol bounds the server-side transient to one chunk
(4 MiB raw, about 5.3 MiB base64 plus JSON overhead). The editor still
assembles the complete text and line index, so its steady-state footprint is
unchanged; memory is not reduced by chunking. `sys.write_file` streams into a
temporary file rather than accumulating the whole document.

## Boundaries

- No Go gateway, LSP, or syntax highlighting is in this loop. The 76.5 µs
  key-to-screen number is the core edit/render path before Tree-sitter or
  span styling is added.
- Tree-sitter highlighting will add highlight computation and styled span
  generation on the draw path. The core edit path should remain in the same
  microsecond range, but the full key-to-paint number will grow.
- The PTY layer stops at the first output byte and does not include a real
  terminal emulator's parsing or painting. A real terminal will add roughly
  2–5 ms on top of the local PTY measurement.
- Search is Enter-triggered linear scanning, not live incremental highlighting.
  Live search highlighting needs an incremental index and is a separate
  engineering effort.

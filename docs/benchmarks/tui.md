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
| Rust | key to screen, 1 MB | 73.3 µs | 78.4 µs | 78.4 µs | 20 |
| Rust | open 100 MB, first frame | 185.9 ms | 185.9 ms | 185.9 ms | 1 |
| Rust | scroll 100 MB, one `j` | 74.8 µs | 101.0 µs | 101.0 µs | 20 |
| Rust | first `/` search, 100 MB | 219.9 ms | 219.9 ms | 219.9 ms | 1 |
| Rust | subsequent `/` search, 100 MB | 202.5 ms | 226.2 ms | 226.2 ms | 10 |
| PTY | open 100 MB, first frame | 193.7 ms | 193.7 ms | 193.7 ms | 1 |
| PTY | key to first output, 1 MB | 128.3 µs | 762.8 µs | 762.8 µs | 20 |
| PTY | scroll 100 MB, one `j` | 224.5 µs | 509.8 µs | 509.8 µs | 20 |
| PTY | first `/` search, 100 MB | 222.3 ms | 222.3 ms | 222.3 ms | 1 |
| PTY | subsequent `/` search, 100 MB | 220.2 ms | 227.4 ms | 227.4 ms | 10 |

## Interpretation

- The 100 MB open is dominated by building the line index and first frame:
  186 ms Rust-side, 194 ms including PTY startup.
- Scrolling is cheap: about 75 µs Rust-side and 225 µs to first PTY output.
- Search is linear in document size: about 200 ms for 100 MB. PTY overhead is
  negligible when the search scan dominates.
- PTY overhead is **not** a fixed 2–8 ms on this machine: key-to-first-output
  adds roughly 55 µs at p50 and 685 µs at p95 over the Rust-side number.
  A real terminal emulator will add its own parsing and painting cost on top.
- These numbers exclude the Go gateway, LSP, terminal emulator, and idle CPU.

## Boundaries

- No Go gateway, LSP, or syntax highlighting is in this loop. The 73 µs
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

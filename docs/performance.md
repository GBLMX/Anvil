# Anvil Performance Baseline

Status: Phase 0 measurement plan. Numbers are gates, not shipped claims.
0.1.1 syntax measurements are recorded below; the full gate suite is not yet
run on the reference machine.

## 1. Reference machine

- CPU: Ryzen 7 5800X, fixed performance governor
- RAM: 32 GB
- OS: Arch Linux, kernel 5.13+ for untrusted plugins
- Terminal: Kitty with keyboard protocol enabled
- Build: release, LTO, stripped; no debug assertions
- Filesystem: local NVMe; page cache state recorded

CI may run on a different machine. Absolute gates belong on the reference
machine; shared CI uses relative regression thresholds.

## 2. Definitions

### First frame

Time from process start to the first completed terminal frame, excluding human
input and excluding plugin execution.

Report three values:

1. `first_frame_editor`: editor only, gateway not started.
2. `first_frame_gateway`: editor plus gateway startup.
3. `first_frame_plugin`: first frame with one empty plugin host started.

A single `<10ms` number is not meaningful without these three.

### File open

Wall time from `open` request to a renderable buffer and first viewport frame.
Measure 1 MB, 10 MB, and 100 MB files.

Include:

- UTF-8 validation;
- line index construction;
- BOM/CRLF detection;
- first viewport layout;
- peak RSS.

### Edit latency

Time from key event to the next frame after an insertion/deletion at a given
offset. Report p50, p95, p99, and max.

Offsets:

- 1% of file;
- 50%;
- 99%;
- near a selection;
- after undo/redo.

Sizes: 1 MB, 10 MB, 100 MB.

Long-line and no-newline files are a separate table. A file that is one 100 MB
line must not hang the UI.

### Scroll

Time between scroll events and rendered frames. Report p99 frame time, not only
average FPS.

Targets are evaluated per file size and with syntax highlighting on/off:

- no highlight: p99 frame <= 16 ms for 60 Hz, or <= 33 ms for the 30 fps gate;
- incremental highlight: p99 frame <= 33 ms;
- no full-document layout pass per frame.

### Plugin call latency

- policy decision: gateway only, no I/O;
- UDS round trip: empty JSON-RPC method;
- `sys.read_file`: 4 KiB and 5 MiB;
- `sys.fetch`: local TLS test server, no public network;
- `sys.exec`: gateway -> exec-runner -> tool, spawn to exit; report runner
  sandbox setup separately from tool runtime.

Do not mix policy latency with network or process startup.

### Hot reload

From reload request to new host ready:

- empty plugin;
- plugin with 10k Lua allocations;
- plugin with an active child process;
- 1000 consecutive reloads.

Assertions: no child process remains, no fd growth, RSS returns to a documented
band.

### Memory

- editor RSS after first frame;
- RSS per 1/10/100 MB document;
- plugin host RSS with empty/heavy plugin;
- peak RSS during open, edit, highlight, and reload.

## 3. Phase 0 target table

| Metric | Target | Gate |
|---|---:|---|
| Editor first frame | <= 10 ms | Reference machine, release |
| First frame with gateway | measure; no promise yet | Report p50/p99 |
| 100 MB open | <= 2 s | First viewport in same run |
| 100 MB scroll | p99 <= 33 ms | Viewport rendering only |
| 100 MB edit at 50% | no O(n) frame stall; measure p99 | Text model decision |
| Gateway policy | p99 <= 1 ms | No DNS, no file I/O |
| UDS round trip | p99 <= 6 ms | 8 MiB frame limit |
| Hot reload | <= 500 ms | Empty plugin, reference machine |
| Child leak after 1000 reloads | 0 | Process table assertion |
| RSS growth after 1000 reloads | <= 5% | Same plugin, same workload |


If a target is exceeded, the result is a design finding, not a reason to change
the measurement.

## 3.1 0.1.1 syntax measurements

Measured with the installed release binary over PTY (120x40), gateway
included, on the workstation used for 0.1.1 development. Insert latency is
key-to-output median in insert mode after warmup.

| Rust file | First frame | Insert median | Insert max |
|---:|---:|---:|---:|
| 64 KiB | 23 ms | 0.18 ms | 0.88 ms |
| 250 KiB | 25 ms | 0.07 ms | 0.56 ms |
| 1 MiB | 29 ms | 0.45 ms | 0.83 ms |

Before the worker-thread change, 250 KiB insert latency was 50.9 ms because
every revision reparsed on the UI thread. Parsing now runs on a debounced
background worker with stale spans retained; files over 256 KiB intentionally
skip highlighting.

## 4. Methodology

- Use release builds with `lto = true`, `codegen-units = 1`.
- Pin CPU frequency; disable turbo variability for absolute runs if possible.
- Warm up the terminal and page cache; report cold and warm separately.
- Run at least 30 samples per metric; report p50/p99, not one run.
- Record commit, rustc/go versions, kernel, terminal, and filesystem.
- Use `criterion` for microbenchmarks and `hyperfine` for process-level runs.
- Use a local TLS test server for fetch tests; never call public endpoints in CI.
- Use synthetic files with fixed seeds; store generation scripts in the repo.
- For scroll, generate input at a fixed rate and measure frame timestamps, not
  loop iterations.

## 5. CI policy

- Shared CI runners may run correctness tests and relative regressions.
- Absolute latency gates run on the reference runner.
- A performance regression fails CI when:
  - the change is statistically significant over at least 30 samples, and
  - p99 regresses by more than 10% against the recorded baseline.
- Security tests are never waived for performance.
- A benchmark that cannot run on the current machine is reported as skipped,
  not silently passed.

## 6. Text model decision inputs

The Phase 0 benchmark must answer, for each candidate:

- p99 edit latency at 1/50/99% offsets for 1/10/100 MB;
- p99 line lookup and viewport extraction;
- memory per 1 MB of text;
- UTF-8 byte/char/grapheme conversion cost;
- undo/redo cost;
- behavior with one 100 MB line;
- snapshot cost when the document is shared via `Arc`.

Selected backend: PieceTable, exposed as `anvil-core::TextDocument`; see
[ADR-0009](adr/0009-text-model-snapshot-gate.md). Rope remains a benchmark
fallback; Gap Buffer is rejected.

The initial candidate harness lives in `anvil-core`:

```text
cargo run --release -p anvil-core --bin text-bench -- 1 10 100
```

It is a smoke/measurement starting point, not the final p99 decision run.

## 7. Reporting template

```text
commit:
rustc:             go:
kernel:            terminal:
cpu governor:      filesystem:

first_frame_editor p50/p99:
first_frame_gateway p50/p99:
open_100mb p50/p99:       peak_rss:
edit_100mb_1% p99:
edit_100mb_50% p99:
edit_100mb_99% p99:
scroll_100mb p99 frame:
gateway_policy p99:
uds_roundtrip p99:
hot_reload p99:
reload_leak:          child_processes:
```

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

## 1.1 Re-run triggers

Performance is re-run at dependency boundaries, not after every commit.

| Trigger | Required run |
|---|---|
| Documentation/plan-only change | None |
| `0.4.6b` activation work | Targeted: `first_frame_plugin`, host start, startup activation on/off |
| Before `0.4.7` | Reference-machine full baseline (currently missing) |
| `0.4.7` resource/security | Targeted: plugin start, reload, OOM, process/cgroup cleanup |
| `0.4.8` plugin I/O | Targeted: UDS round trip, `sys.read_file`, `sys.fetch`, `sys.exec` |
| `0.4.9` session/protocol | Full hot path: first frame, 1/10/100 MB open, edit, scroll, key-to-screen |
| Any `0.5.x` slice touching input/render | Targeted: first frame, key-to-screen, scroll, palette/preview/tabs |
| `0.6.0` / `0.6.1` | Full gates plus package verification, install throughput, and plugin start |
| Any release tag | Full gates in this document |

Absolute gates run only on the reference machine. Shared CI may run relative
regressions; a p99 regression over 10% with at least 30 samples fails the gate.
Security tests are never waived for performance.

## 1.2 Early-code optimization policy

Do not optimize code because it is old, large, or visually suspect. Optimize
only after a reproducible measurement shows a gate or a user-visible hot path
miss.

| Finding | Action |
|---|---|
| Misses a gate in section 3 | Open one targeted optimization slice with before/after numbers |
| Within gates, no user-visible stall | Leave it; do not churn the code |
| Large file blocks a `0.5.0` boundary change | Refactor as part of that slice, not as a performance rewrite |
| Search or open has a p99 outlier | Re-measure first; then optimize only that path |

Current candidate: 100 MB `/` search reports p99 around 230 ms in the
historical spot check. That is a re-measurement target, not a mandate to change
the search implementation yet.

Baseline commands:

```bash
cargo bench -p anvil-core --bench text_model
go build -o target/release/anvil-gateway ./go/cmd/anvil-gateway
cargo bench -p anvil-tui --bench e2e_edit --features e2e-bench
```

Record results under `docs/benchmarks/` before and after the change. A refactor
without a measured regression does not count as an optimization.

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

## 3.2 0.3.0 local spot checks (not gates)

These numbers were collected on an i7-12700H development workstation, not the
Ryzen 7 5800X reference machine. They are spot checks, not release-gate
evidence. Several scenarios intentionally have fewer than the required 30
samples and are marked as single-run or observed-max values.

| Scenario | n | Observation | Interpretation |
|---|---:|---:|---|
| Rust 100 MB first frame | 1 | 90.8 ms | single run |
| PTY 100 MB first frame | 1 | 205 ms | single run |
| Rust 100 MB scroll | 20 | p99 120.7 us | spot check only |
| PTY 100 MB scroll | 20 | p99 720 us | spot check only |
| Rust key-to-screen | 20 | p99 88.5 us | spot check only |
| PTY key-to-screen | 20 | p99 680 us | spot check only |
| 100 MB first search | 1 | 217 ms Rust / 232 ms PTY | single run |
| 100 MB subsequent search | 10 | p99 222 ms Rust / 269 ms PTY | spot check only |
| PieceTable 100 MB open | 3 | 19.2 ms | observed maximum |
| PieceTable 100 MB viewport | 3 | 2.1 us | observed maximum |
| PieceTable 100 MB random edits | 3 | 115 us | observed maximum |

`BenchmarkReadResponseEncoding` measured **12.25 ms mean** for a large
JSON/base64 read response on this workstation. It is a Go benchmark mean, not
a UDS round-trip p99, and it has not been rerun on the reference machine with
the required sample count. Treat it as an unverified performance risk, not a
P1 gate failure.

The TUI's normal file path uses binary frames; `BenchmarkReadFileChunk`
measured 0.81 ms on the same machine. Large `sys.read_file` consumers should
prefer binary frames. The JSON/base64 large-read path remains available for
compatibility but is not the recommended transport for multi-megabyte reads.

### Multi-cursor transaction spot check

`editor/multicursor/1MB` measures inserting one character at every caret in a
1 MB file. `insert_carets` is the public multi-caret path; it uses the
one-edit-at-a-time transaction for 16 or fewer carets and the batch path above
that threshold. `legacy_carets` always measures the transaction path.

| Carets | `insert_carets` path | `insert_carets` | Legacy | Ratio |
|---:|---|---:|---:|---:|
| 10 | fallback | 515 µs | 506 µs | 1.02x |
| 100 | batch | 584 µs | 5.25 ms | 9.0x |
| 1000 | batch | 1.48 ms | 57.8 ms | 39x |

These are local Criterion spot checks, not release gates. The batch path applies
text edits without per-edit selection/index work, rebuilds `LineIndex` once, and
maps caret offsets in a sorted batch. Selections with non-empty ranges and
multi-edit replacements still use the legacy transaction path.

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

### PGO and BOLT

Opt-in profile-guided optimization is documented in
[pgo-bolt.md](pgo-bolt.md). PGO uses a PTY workload in
`scripts/pgo-workload.py`; BOLT additionally requires `perf`, `perf2bolt`, and
`llvm-bolt`. Neither path changes the default release build or CI.

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

## 8. 2026-11-06 boundary run

Environment: i7-12700H, 15 GiB RAM, Linux 7.3.1, Rust 1.88.0,
Go 1.26.10, `powersave` governor. CPU sampled at 4.3 GHz on pinned P-cores.
Release build used Cargo defaults, not the reference-machine LTO profile.

### Boundary results

| Scenario | Observation | Gate |
|---|---:|---:|
| Empty document render | < 1 ms | pass |
| 1 MB single-line render | 1 ms | pass |
| 100 MB single-line render | 100 ms, 314 MB peak RSS | no hang; frame budget missed |
| Max `u16` terminal hit-test | no overflow | pass |
| Empty editor A1 p99 | 37.2 µs | < 50 µs |
| 1 MB editor A1 p99 | 59.7 µs | < 50 µs |
| 100 MB editor A1 p99 | 105.1 µs | < 50 µs |
| 100 MB editor A3 p99 | 174.6 µs | < 100 µs |
| 100 MB key-to-screen p99 | 130 µs Rust / 846 µs PTY | pass |
| 100 MB scroll p99 | 145 µs Rust / 606 µs PTY | pass |
| 100 MB search p99 | 28 ms Rust / 30 ms PTY | pass |
| 100 MB open first frame | 113 ms Rust / 163 ms model | pass |
| PTY terminal setup without reply | 2.0 s | terminal-query artifact |
| 100k line-index stress | chunk count 1,281; offset p99 2.8 µs | pass |

The 0.5.9 search path previously built a `HashSet` of every grapheme boundary
in the document. A 100 MB `/` search measured **15.0 s** before the fix and
**22–30 ms** after replacing it with local `GraphemeCursor` checks.

The 100 MB single-line frame remains above the 33 ms scroll budget because
`visible_lines` materializes the whole line before horizontal clipping. It no
longer hangs, but the line-width path needs a windowed renderer before the
frame budget can be claimed.

The PTY 2 s startup is crossterm's keyboard-enhancement query timeout. A real
terminal that answers the query avoids it; the synthetic PTY does not.

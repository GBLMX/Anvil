# ADR-0006: Upstream Dependency Baseline

- Status: Accepted for Phase 0
- Date: 2026-10-30

## Context

Anvil needs a small dependency surface that supports:

- a low-latency blocking terminal UI;
- large-text editing;
- an untrusted Lua/QuickJS plugin host;
- a Linux sandbox;
- a Go policy gateway;
- generated API artifacts;
- reproducible security and performance tests.

The dependency set should avoid:

- an async runtime in the editor/plugin host when a blocking design is enough;
- a second shell/HTTP framework inside the gateway;
- C libraries that require system Lua/QuickJS/libseccomp;
- duplicate semver-incompatible terminal backends;
- a runtime dependency on an unstable editor framework.

## Decision

### Rust

| Area | Choice | Version policy | Why |
|---|---|---|---|
| TUI | `ratatui` + `crossterm` | `0.30` / `0.29` | Rust 1.88 compatible; direct terminal control; no actor/runtime |
| Text | `ropey` | `1.6` | Default candidate for 100 MB random edits; added only after ADR-0004 gate |
| Lua | `mlua` with `luau` | `0.12` | Luau sandbox removes dangerous stdlib; vendored runtime |
| JS | `rquickjs` | `0.14` | QuickJS-NG bindings; no V8/Deno |
| Sandbox | `landlock`, `seccompiler`, `rustix` | `0.4`, `0.5`, `1` | Unprivileged Landlock/seccomp without libseccomp |
| IPC | `serde`, `serde_json` | `1` | Length-prefixed JSON-RPC; no `jsonrpsee` |
| Config/manifest | `serde`, `toml_edit` | `1`, `0.25` | Comment-preserving TOML; shared format |
| CLI | `clap` | `4` | `anvil doctor` and build commands |
| Errors | `thiserror`, `anyhow` | `2`, `1` | Typed core errors; anyhow only at binaries |
| Logs | `tracing`, `tracing-subscriber` | `0.1`, `0.3` | Structured fields without a logging framework |
| IDL | `schemars`, `ts-rs` | `1`, `11` | Schema/TS from Rust types; LuaLS template |
| Unicode | `unicode-segmentation`, `unicode-width` | `1`, `0.2` | Grapheme and cell-width correctness |
| Dev | `tempfile`, `proptest`, `criterion`, `libfuzzer-sys` | `3`, `1`, `0.5`, `0.4` | Fixtures, properties, benchmarks, fuzz targets |

### Go

| Area | Choice | Why |
|---|---|---|
| Runtime | Go standard library | `net/http`, `os/exec`, `context` cover the gateway |
| Syscalls | `golang.org/x/sys/unix` | `openat2`, `SO_PEERCRED`, process control |
| Leak tests | `go.uber.org/goleak` (test only) | Goroutine and child-lifecycle checks |
| Format/lint | `gofmt`, `go vet` | Minimal, reproducible CI baseline |

### Deliberate exclusions

- **No Tokio in v1.** Editor and plugin host use OS threads and blocking
  channels. The Go gateway owns asynchronous I/O. If a future feature needs an
  async Rust runtime, it needs an ADR and a measured reason.
- **No `jsonrpsee`.** The protocol is small and length-prefixed; use the custom
  codec in `anvil-ipc`.
- **No `libseccomp`.** `seccompiler` keeps the sandbox build free of a system C
  library and is a better fit for the exec runner.
- **No Ferriso runtime dependency in v1.** `ferriso-core`, `ferriso-tui`, and
  `ferriso-editor` are references, not pinned upstreams. Ferriso is useful for
  its snapshot model and gap-buffer/line-index design, but its 0.3.x API and
  Tokio actor assumptions are not a stable base for Anvil. Copy or adapt only
  small MIT-licensed pieces with attribution; do not expose Ferriso types in the
  public API.
- **No `syntect`/tree-sitter decision yet.** Syntax highlighting is a separate
  ADR after the text/viewport baseline; do not add a parser stack in Phase 0.
- **No V8/Deno, no system Lua, no system QuickJS.**
- **No second crossterm backend.** Ratatui's crossterm feature is the only
  terminal path; avoid semver-duplicated `crossterm` crates.

## Rules

1. Dependencies are pinned in `Cargo.lock` / `go.sum`; ranges in manifests are
   upper-bounded by the versions above.
2. `cargo-deny` allowlist: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC,
   Unicode-DFS-2016. Anything else requires an ADR.
3. Native crates must vendor their C source (`mlua` Luau, `rquickjs`
   QuickJS-NG); do not link a system Lua/QuickJS.
4. No new native dependency without a security and license review.
5. Every dependency that parses untrusted input needs a fuzz target or a
   documented reason why not.
6. A direct dependency upgrade is a normal change; a version family jump
   (0.x -> 0.y, semver-incompatible) needs a changelog entry and a test run.

## Consequences

Positive:

- No async runtime in the low-latency path.
- One terminal backend and one small IPC codec.
- Luau removes the Lua standard-library escape surface.
- Landlock/seccomp are available without root or libseccomp.
- The dependency set is compatible with Rust 1.88 and the observed 2026 crate
  versions.

Negative:

- Blocking plugin calls need one OS thread per plugin; acceptable for v1 but
  caps plugin concurrency.
- `mlua` Luau is not byte-for-byte Lua 5.4; plugins using removed libraries or
  bytecode need migration.
- `rquickjs` and `mlua` are native C/C++ dependencies and remain inside the
  sandbox risk model.
- Rope integration still needs the Phase 0 benchmark and grapheme/byte
  conversion work.
- Direct ratatui means Anvil owns terminal lifecycle, panic cleanup, and input
  translation that Ferriso would have provided.

## Revisit triggers

- 100 MB edit/scroll gate fails with Rope.
- Luau compatibility blocks a required plugin.
- Blocking plugin IPC cannot meet latency under a realistic plugin count.
- Landlock/seccomp cannot be applied portably to the exec runner.
- A dependency license or maintenance issue appears.

## References

- [Ratatui installation and Rust version](https://ratatui.rs/installation/)
- [Ropey](https://docs.rs/ropey/latest/ropey/)
- [Luau sandbox](https://luau.org/sandbox/)
- [mlua](https://crates.io/crates/mlua)
- [rquickjs](https://crates.io/crates/rquickjs)
- [landlock crate](https://crates.io/crates/landlock)
- [seccompiler crate](https://crates.io/crates/seccompiler)
- [Go x/sys/unix](https://pkg.go.dev/golang.org/x/sys/unix)

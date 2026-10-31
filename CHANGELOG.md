# Changelog

All notable changes to Anvil are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.4] - 2026-10-31

### Fixed

- LSP command multiplexing now polls socket readability before blocking on a
  complete Content-Length frame, avoiding partial-frame timeout desync during
  large completion/hover responses or busy gateway traffic.

## [0.2.3] - 2026-10-31

### Added

- LSP completion popup with fuzzy label filtering and snippet insertion,
  `gd` definition jumps, and `K` hover documentation.

### Fixed

- Tab outside Insert mode no longer modifies the buffer; Tab indentation is
  insert-mode only.
- Trusted `-no-sandbox-lsp` processes inherit the gateway PATH, fixing child
  toolchains and Python shebangs for gopls/pyright.

## [0.2.2] - 2026-10-31

### Added

- Language-server selection is standardized by file type: rust-analyzer for
  Rust, gopls for Go, and pyright/basedpyright/pylsp for Python, each with its
  own language id and project-root markers.

### Fixed

- `:reconnect` drops the old file-tree worker and LSP worker before replacing
  the gateway session, so later tree/fuzzy operations use the new socket.

## [0.2.1] - 2026-10-31

### Added

- File tree and fuzzy-file enumeration now run in a dedicated `FileWorker`
  thread over a separate gateway connection; the UI renders a loading state
  instead of blocking. File discovery uses `rg --files` through `sys.exec`
  with a bounded `sys.read_dir` fallback.

## [0.2.0] - 2026-10-31

### Added

- `:reconnect` restarts the gateway child and performs a fresh handshake,
  replacing a poisoned client without discarding buffers or unsaved text.

- LSP wire foundation: Rust `Content-Length` framed JSON-RPC transport,
  request/notification helpers, `textDocument/publishDiagnostics` parsing, and
  a Go `internal/lsp` framed manager with persistent stdin/stdout pipes and
  crash/lifecycle primitives.

- `GatewayClient` now queues interleaved server-initiated JSON-RPC
  notifications while waiting for a response; binary read/write paths skip
  notifications instead of desynchronizing.

- Gateway `lsp.start` / `lsp.send` / `lsp.stop` routes with persistent
  `internal/lsp` process pipes, sandboxed exec-runner launch, `lsp.message`
  notifications, `lsp.stderr`, and `lsp.crashed` on reader failure. LSP tool
  launches can grant workspace write plus rustup/cargo read/execute paths. `GatewayClient` exposes
  matching LSP helpers plus blocking notification receive.

- Buffer paths are canonicalized before matching LSP diagnostics, including
  symlinks and `..` segments.
- LSP document and root URIs are canonical absolute `file://` URIs with
  percent-encoding; diagnostics URIs are decoded before path matching.
- TUI LSP worker on a second authenticated gateway connection; initialize
  response ordering, initialized, didOpen, didSave, publishDiagnostics parsing,
  diagnostics map, and red underlined diagnostic ranges. Trusted local language
  servers run through `-no-sandbox-lsp`; plugin-host remains sandboxed.
  `ANVIL_GATEWAY_LOG` captures gateway/LSP traffic.

- Four-space insert-mode Tab, display-width
  tab expansion, a gateway-backed workspace tree (`Space e` / `:tree`), and a
  fuzzy file finder (`Space f`). The tree filters `.git` and `target`.
- LSP restarts up to three times after crashes; the LSP handshake token is
  reusable only within the owning gateway session so the editor can reconnect.
  Diagnostics use UTF-16 to
  byte/display-column mapping for exact underlined ranges.

### Known limitations

- LSP is experimental; trusted local language servers use `-no-sandbox-lsp`,
  while plugin-host remains sandboxed.
- Splits and advanced Ex commands are not implemented yet.

## [0.1.1] - 2026-10-31

### Added

- Vi word motions `w`/`b`/`e` and operators `dw`/`db`/`de`/`yw`/`yb`/`ye`
  with byte-borrowing word classification in `anvil-core`; multibyte word
  ends stay on UTF-8 boundaries. Numeric prefixes are supported for motions,
  `d`/`y` word operators, `dd`, `yy`, `x`, and line movement.

- `ciw`, `ci"`, and `ci'` inner text-object changes; named yank/put and
  delete/change registers via `"a`, `"_` blackhole support, system clipboard
  register `"+`, macro recording `q{reg}` / replay `@{reg}` / `@@`, and numeric
  prefixes for motions and operators.

- Literal `:s/old/new/[g]` and `:%s/old/new/g` substitution with dynamic
  offset updates after length-changing replacements.

- Tree-sitter syntax highlighting for Rust and JSON with a revision-keyed
  cache, zero-copy span slicing, keyword/string/comment/number rendering, a
  background parser thread, 50 ms debounce, and stale-span retention while a
  new parse is in flight. Files over 256 KiB remain unhighlighted to keep key
  latency bounded.

- TUI relative line-number gutter, current-line background highlight,
  scrolloff margin, dirty `[+]` title marker, colored mode badge in the status
  line, block/bar cursor shape switching, and current/other search-match
  highlighting.

- ADR-0012 plugin-host isolation contract: one sandboxed host per plugin,
  FD-delivered one-shot tokens, no inherited editor/gateway FDs, process-group
  lifecycle, and accepted escape/orphan tests.

- Empty plugin host: token-FD read, `close_range` to stdio only,
  `no_new_privs`, Landlock filesystem read rules, `session.hello`, and tests for
  `/etc/passwd` denial, exec/write denial, fd closure, native-ABI seccomp with
  AF_UNIX-only sockets (TCP/UDP `EPERM`), editor UDS EOF exit, and editor
  `SIGKILL` reaping.

- Plugin-host cgroup v2 helper: `pids.max=64`, `memory.max=256MiB`,
  post-spawn PID attach, cgroup2 `statfs` validation with existing-file writes
  only, and `HostExit::OomKilled` classification for SIGKILL when a memory
  limit was active.

- Plugin-host framed UDS dispatch: `anvil-ipc` length-prefixed frames,
  `sys.ping -> pong`, and `sys.exec -> Unsupported` until the sandboxed
  executor is wired.

- TUI debug `HostSession`: `:plugin-start` / `:plugin-exec` / `:plugin-stop` /
  `:plugin-oom`, background UDS thread with mpsc events, 2-second RPC timeout,
  OOM status mapping, host SIGKILL event-loop test, and host/cgroup cleanup on
  drop. No VM or plugin API yet.

### Planned

- 0.1.1 / Phase 0.7: plugin-host isolation, Tree-sitter syntax highlighting,
  LSP client, and optional `:reconnect` poison recovery. See
  [docs/phase-0-7-plan.md](docs/phase-0-7-plan.md).

## [0.1.0] - 2026-10-31

### Added

- Phase 0.6 exec sandbox foundation: `anvil-exec-runner`, Landlock
  read-only workspace/runtime rules with per-tool and ELF-interpreter execute
  grants, a positive seccomp allowlist with `clone`/`clone3` and
  executable-mapping filtering, OS-thread locking and identity checks for
  per-thread confinement, `rg`/`fd` argv policy parsers, optional cgroup v2
  `pids.max` with pre-exec membership, soft-RLIMIT relaxation, stale-cgroup
  cleanup, and `RLIMIT_NPROC` fallback, plus a supervised process-group runner
  for timeout and output caps.

- Phase 0.6 gateway per-session cgroup provisioning and cleanup, plus a
  deployment model document for systemd delegation and root-owned roots.

- Phase 0.6 Go gateway skeleton: Unix-socket framed JSON-RPC,
  one-shot `session.hello` token-hash authentication, `SO_PEERCRED` UID checks,
  connection-cancel propagation, and `sys.exec` routing through
  `anvil-exec-runner`; Rust handshake example included.

- Phase 0.6 SSRF-constrained `sys.fetch`: exact host allowlist, DNS pinning,
  blocked-address validation, disabled redirects, bounded bodies, allowlisted
  request headers with name-only sensitive auditing, and host-bound TLS SNI.

- Phase 0.6 TUI-0/TUI-1: minimal `anvil-tui` editor with insert variants
  (`i`/`a`/`A`/`o`/`O`), `:w`/`:wq`/`:w file`/`:q!`, an unsaved-change quit
  guard, undo/redo, `x`/`dd`/`yy`/`p` with an unnamed register, substring
  `/` search with `n`/`N`, an embedded read-only `:help` page, multi-buffer
  switching with `:e`/`:bn`/`:bp`, grapheme-aware cursor movement and deletion,
  horizontal scrolling, cursor/viewport separation, dirty-only redraws,
  terminal restore, and borrowed text chunks.

- Phase 0.6 TUI edit-loop benchmark: layered Rust-side and PTY latency for
  key-to-screen, 100 MB open, 100 MB scroll, and 100 MB search.

- TUI 100 MB open memory: build `LineIndex` directly from the source `&str`
  instead of a full `TextBuffer::slice` copy; peak VmHWM drops from 493 MB to
  300 MB, steady RSS remains 198 MB.

- Two-pass parallel line scanning for large text: disjoint worker ranges count
  and fill the final line-start array, cutting 100 MB Rust open from 174 ms to
  **89–96 ms** and the JSON gateway path from 640 ms to 563 ms without
  increasing the 198/300 MB TUI RSS/peak.

- Open phase instrumentation (`ANVIL_OPEN_TIMING`) and a decomposition showing
  ~80 ms stat/pipeline read, ~62 ms line index, and ~0.4 ms first frame.
+
+- Binary `sys.read_file` frames: `"binary": true` returns `[eof flag][raw
  bytes]`, 7 MiB chunks, no base64/JSON on the read path. 100 MB gateway PTY
  open drops from 563 ms serial to **171.1 ms** with a 4-request window; gateway
  RSS is 66/91 MB. Writes use binary data frames plus JSON control; 100 MB
  save is 22.2 ms on ext4 (page-cache warm).

- Workspace-scoped chunked `sys.read_file`/`sys.write_file`: fail-closed
  gateway I/O, base64 5.5 MiB JSON writes, binary 7 MiB reads, atomic rename, streaming writes, path
  canonicalization before permission checks, and no `std::fs` fallback.

- `anvil-ipc::GatewayClient`: synchronous handshake, chunked read/write,
  response-id checks, policy-error detection, and complete-file helpers.

- TUI gateway integration: `--workspace`, per-session `anvil-gateway` spawn,
  token handshake, gateway-backed open/save/`:e`/`:w`, 256 MiB open limit, and
  fail-closed behavior when the gateway cannot start. No `std::fs` document
  fallback remains in the runtime path.

- Phase 0.5 complete: PieceTable production selection, cursor/line-index
  primitives, editor snapshots, selection-aware undo, and the locked-core
  A/C benchmark and 100k stress gate.

- Project charter and Phase 0 security/performance baseline.
- Threat model covering path traversal, SSRF, plugin escape, IPC spoofing,
  resource exhaustion, cancellation, and manifest escalation.
- Architecture decision records for plugin isolation, the per-session Go
  gateway, lightweight IDL, text-model selection, exec policy, upstream
  dependencies, the JSON-RPC/token contract, and the selection-first
  interaction model, including the text-model snapshot decision gate and a
  accepted cursor/snapshot ownership ADR.
- Repository governance files: security policy, contribution guide, code of
  conduct, editor configuration, license files, and CI skeleton.
- `anvil-ipc`: length-prefixed JSON-RPC framing with an 8 MiB limit, clean-EOF
  handling, truncation detection, I/O error preservation, Unix socket
  connections with `0600` socket permissions, `SO_PEERCRED` lookup, and
  validated `Request`/`Response`/`Meta`/`ErrorObject` envelopes.
- `anvil-api`: interaction, view, and error contracts: `InteractionMode`,
  `EditorView`, `Viewport`, `Selection`, `TextPosition`, `SetModeRequest`,
  `SetModeResponse`, `ApiError`, and stable method-name constants.
- `anvil-api/bindings`: generated TypeScript declarations, JSON Schema, LuaLS
  annotations, and the API version constant.
- `anvil-core`: `TextBuffer` with `PieceTable` selected as the production
  `TextDocument` backend; Rope and Gap Buffer remain benchmark-only. UTF-8
  boundary checks, cached line counts, proptest model comparisons,
  `EditCommand`/`History` undo-redo with revisions and typing groups, COW
  PieceTable text chunks, chunked `LineIndex`, cursor/grapheme/display
  conversion, a criterion decision harness, a p99/snapshot timing
  probe, a Linux `text-rss` probe, and a PR-only 10 MB same-runner p99
  regression gate.
- `anvil-editor`: shared `EditorSnapshot`, `EditorState`, selection mapping,
  and `EditorView` projection.
- `go/internal/fsroot`: `openat2`-confined workspace reader, atomic writer,
  size query, and traversal/symlink/regular-file/limit tests.

### Security

- `anvil-exec-runner` applies Landlock/seccomp, the symlink-swap race test
  passes, and non-allowlisted exec (including through `ld.so`) is denied;
  the gateway routes `sys.exec` through this runner.
- Untrusted plugins run outside the editor process; the Go gateway is a policy
  gateway, not the plugin sandbox.

- Four-space insert-mode Tab, display-width
  tab expansion, a gateway-backed workspace tree (`Space e` / `:tree`), and a
  fuzzy file finder (`Space f`). The tree filters `.git` and `target`.
- LSP restarts up to three times after crashes; the LSP handshake token is
  reusable only within the owning gateway session so the editor can reconnect.
  Diagnostics use UTF-16 to
  byte/display-column mapping for exact underlined ranges.

### Known limitations

- 0.1.0 is pre-alpha; the TUI is runnable but the plugin API is not stable.
- Untrusted plugin-host isolation remains the Phase 0.6/0.7 open security item.
- Landlock-backed untrusted plugins require Linux 5.13+.
- 100 MB open/save meet 200 ms on this NVMe Linux host; 50 ms open remains a
  long-term target.

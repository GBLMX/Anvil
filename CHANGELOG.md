# Changelog

All notable changes to Anvil are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Pre-1.0 version
lines are release-series labels, not a compatibility promise; `0.4.x` is the
post-`0.4.0` feature line.

## [Unreleased]

### Added

- Workspace tree watcher: the gateway exposes `sys.read_dir_signature`, and
  the TUI polls the root plus expanded directories through `FileWorker`.
  External create, delete, and rename now refresh the tree while preserving
  selection and expansion; watchers are capped and marked stale when the
  gateway cannot observe the tree. See
  [ADR-0018](docs/adr/0018-workspace-tree-watch.md).

## [0.4.0] - 2026-11-01

### Release focus

- `0.4.0` closes the Polish & Specification train: fixes, tests,
  documentation, and contract closure only; no new capability families. See
  [the release policy](docs/polish-and-specification.md).
- The plugin host stops at the sandboxed empty-host boundary. Plugin runtime,
  loading, capability bindings, gateway control-socket registration, and
  end-to-end plugin `sys.*` dispatch move to the post-`0.4.0` feature line;
  the runtime slice is tracked as `0.4.4`.

### Added

- Buffer lifecycle commands: `:new` creates an unnamed buffer and `:bd [n]`
  closes the current or indexed buffer. Dirty close prompts for
  save/discard/cancel; `:bd!` discards; closing the last buffer leaves one
  valid empty buffer.
- Multi-cursor caret insertion now applies text edits without per-edit index
  updates, rebuilds `LineIndex` once, and maps selections in a sorted batch;
  a Criterion benchmark compares it with the legacy transaction path.
- Start dashboard for empty launches: workspace/version context and shortcuts
  for insert, fuzzy file finding, the file tree, help, and quit.
- Dark truecolor TUI theme with rounded panels, accent focus borders, mode
  badges, panel surfaces, and WCAG-AA text/syntax contrast.
- Fixed line-number gutter clipping and indentation guides overwriting the
  first code character.
- Workspace tree create (`a`/`A`), rename (`r`), and confirmed delete
  (`d`) through confined `sys.create_dir`/`sys.rename`/`sys.remove` gateway
  operations; tree and fuzzy lists now scroll to keep the selection visible.
- LSP starts automatically for supported files and keeps one worker per
  `(tool, language_id)`, so switching Rust ↔ Go no longer stops the other
  language server.
- Plugin-host now initializes a sandboxed Luau runtime for a smoke check after
  Landlock/seccomp; plugin loading and capability bindings remain pending.
- CLI `-h`/`--help`, `-V`/`--version`, and `--workspace=<DIR>` parsing, with
  parser tests and documented usage.
- Opt-in PGO and BOLT workflow: `scripts/pgo-bolt.sh` builds instrumented and
  profile-use binaries, records a PTY workload, and supports post-link BOLT
  profiles via `perf2bolt`; no default release or CI behavior changes.
- Vertical two-pane splits with `Ctrl-w v`, `Ctrl-w h/l` focus switching, and
  `Ctrl-w q` close; each pane keeps its own cursor, scroll, and buffer.
- Multi-selection core: atomic multi-range transactions, `s` word-occurrence
  selection, `Alt-s` line-cursor splitting, `C`/`Alt-C` cursor extension, and
  secondary-caret rendering.
- Space which-key overlay and indentation guides with current-block
  highlighting.
- Signature Help requests after `(`/`,` with a reusable popup, cancellation,
  and stale response checks.
- Workspace Symbols (`Space S`) with client-side filtering and cross-file
  navigation.
- Built-in LSP settings for TypeScript/JavaScript, C/C++, Java, Lua,
  JSON/JSONC, YAML, and shell files, with per-language root markers, arguments,
  and environment overrides.
- LSP initialize errors are surfaced in the status bar instead of being treated
  as a generic crash; `jdtls` shows an indexing-workspace startup hint.

### Fixed

- Search mode now preserves Tree-sitter foreground colors; matches patch a
  background layer instead of replacing syntax spans.
- Open buffers now watch external metadata changes through the gateway in the
  background `FileWorker`: clean buffers reload, dirty buffers prompt before
  reloading, and unchanged metadata is debounced.
- File tree move/copy now run through gateway `sys.rename`/`sys.copy` in the
  background worker, and `d` recursively deletes entries through
  `sys.remove_all`; the status bar shows a spinner while the operation runs.
- Upgraded the Go module and CI toolchain to Go 1.26.10 and `golang.org/x/sys`
  to v0.44.0, resolving the 45 standard-library vulnerabilities reported by
  `govulncheck` against Go 1.22.3 and the remaining module advisory; CI now
  runs a pinned `govulncheck` v1.9.0 vulnerability scan.
- Rename, Code Action, and Formatting responses are ignored when the active
  path or editor revision changed after the request, preventing stale edits
  from being applied to a different or newer buffer.
- Document-symbol traversal is bounded to 64 levels and 4096 items so malformed
  or adversarial symbol trees cannot exhaust the editor stack or memory.
- Gateway file reads, directory listings, and stats now open with
  `O_NONBLOCK`, so a workspace FIFO cannot block the gateway or editor.
- File permissions now require both the requested path and its resolved
  symlink target to match the granted subtree, preventing a link inside an
  allowed path from exposing a workspace file outside that grant.
- The `e2e_edit` performance harness now compiles again through a
  feature-gated scroll helper, so 100 MB end-to-end measurements can run.
- Added pinned `golangci-lint` v2.15.0 to CI with the standard linters and
  intentional deferred-close exclusions; fixed the remaining errcheck and
  staticcheck findings.
- Bounded the seccomp program instruction count before converting it to
  `uint16`, with a regression test for oversized programs.

## [0.3.0] - 2026-10-31

### Added

- LSP Code Actions/Quick Fixes (`Space a`) with a selectable popup and a
  shared `WorkspaceEdit` application path. Code Action requests forward the
  complete diagnostic payload (`code`, `codeDescription`, `relatedInformation`,
  `tags`, and `data`), and the client advertises Code Action literal support.
- LSP Rename (`Space r`) with an inline rename prompt; edits apply to every
  file returned by the language server.
- LSP Formatting (`Space F`, `:format`) applies `TextEdit[]` through the shared
  `WorkspaceEdit` path without writing to disk.
- Document Symbols (`Space s`, `:symbols`) with hierarchical
  `DocumentSymbol`/`SymbolInformation` parsing, selection, and cursor jump.
- `:wa` saves every modified buffer. `WorkspaceEdit` changes to unopened files
  create background buffers and never write files during an edit.

### Changed

- LSP Code Actions/Rename now share UTF-16 workspace-edit application,
  unopened-file loading, overlap rejection, and active-buffer snapshot updates.
- Stale Code Action responses are cancelled when the popup closes or another
  overlay/request replaces it.

## [0.2.19] - 2026-10-31

### Fixed

- WorkspaceEdit now updates the matching active-buffer snapshot as well as
  `model.editor`; switching away/back no longer resurrects stale text after an
  edit. Added regression coverage for editing the active buffer after opening
  another file.

## [0.2.18] - 2026-10-31

### Added

- `WorkspaceEdit` engine for LSP `changes` and `documentChanges`: URI decoding,
  UTF-16 range conversion, cross-file application, unopened-file loading,
  descending edits, overlap rejection, and atomic validation before buffers
  change. Code Actions/Rename UI wiring remains the next step.

## [0.2.17] - 2026-10-31

### Changed

- Extracted editing/motions/registers into `editing.rs` and unit/integration
  tests into `tests.rs`. `lib.rs` is now 981 lines, down from 4355 before the
  modularization pass.

### Fixed

- Corrected the no-file save status from `E32: no file name` to
  `E: no file name` and tightened the regression assertion.

## [0.2.16] - 2026-10-31

### Changed

- Further split `anvil-tui/src/lib.rs`: file I/O lives in `file_io.rs`, buffer
  lifecycle/navigation in `buffer.rs`, and search/substitution/Ex dispatch in
  `commands.rs`. `lib.rs` is now 2552 lines, down from 3113 at the start of
  this extraction pass.

## [0.2.15] - 2026-10-31

### Fixed

- Completion cancellation now removes every pending completion request, even
  when no popup is visible. Insert-mode Esc and buffer switches therefore
  cannot let a late response reopen a popup after the request was cancelled.

## [0.2.14] - 2026-10-31

### Changed

- Completion now auto-triggers after `.` and `:` in Insert mode, cancels
  pending completion requests on close/buffer switch/fuzzy open, and ignores
  stale responses. Completion, hover, diagnostics, and fuzzy overlays are
  mutually exclusive.

## [0.2.13] - 2026-10-31

### Fixed

- Diagnostic navigation prefers severity-bearing primary diagnostics over
  related/secondary entries, so `]d` / `[d` and the popup show the actionable
  error instead of a related-location note.

## [0.2.12] - 2026-10-31

### Added

- `]d` / `[d` diagnostic navigation with wraparound, a Diagnostic popup, and
  severity labels. Selecting a diagnostic moves the cursor to its UTF-16 start
  position.

## [0.2.11] - 2026-10-31

### Added

- File tree `H` toggles hidden directories; `.github` and `.cargo` remain
  visible by default. Fuzzy file discovery passes the hidden state to
  `rg --files` and excludes `.git`, `target`, `.cache`, `.cargo`, and `.rustup`.

## [0.2.10] - 2026-10-31

### Changed

- Fuzzy file results show workspace-relative paths, clip long paths from the
  left with an ellipsis, and highlight case-insensitive query matches with
  bold/underline styling.

## [0.2.9] - 2026-10-31

### Added

- Multi-document LSP sessions: buffer navigation sends `didOpen`/`didChange`
  for the active document, and `:w` sends `didChange` + `didSave`.
- Automatic language-server switching when the active buffer changes language
  (Rust -> Go -> Python), with stale worker state cleared.

## [0.2.8] - 2026-10-31

### Fixed

- Navigation through definition, file tree, and fuzzy finder no longer replaces
  an edited active buffer without snapshotting it. `open_path` now preserves
  modified buffers, reuses already-open buffers, and routes navigation through
  the multi-buffer model.

## [0.2.7] - 2026-10-31

### Changed

- Split the TUI monolith into `keys`, `render`, `lsp_ui`, and `file_tree_ui`
  modules. `lib.rs` dropped from 4355 to 2863 lines without behavior changes.
- LSP settings, completion/hover/definition requests, and file-tree/fuzzy
  behavior now live at their domain boundaries.
- Synchronized the Phase 0.7 plan with the implemented Rust/Go/Python LSP and
  module layout.

## [0.2.6] - 2026-10-31

### Changed

- LSP worker failures are classified as crashes consistently, stale
  completion/hover/pending state is cleared on stop/restart/reconnect, and an
  unsupported file no longer consumes the LSP gateway connection.
- Removed unused `ClientError::kind` and duplicated LSP timeout plumbing; poll
  readability is the single framed-connection wait path.

## [0.2.5] - 2026-10-31

### Fixed

- `wait_readable` treats `HUP`/`ERR` as readiness, so a closed gateway or LSP
  socket reaches `recv_notification` and emits `Crashed` instead of spinning
  until timeout.

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

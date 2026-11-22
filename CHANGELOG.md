# Changelog

All notable changes to Anvil are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Pre-1.0 version
lines are release-series labels, not a compatibility promise; `0.5.x` is the
post-`0.5.0` feature line.


## [Unreleased]

## [0.7.0] - 2026-11-22
### Added
- Local release layout, installer, signed-plugin, artifact, and
  signature-verification dry run passed with ephemeral production-like Ed25519
  keys; evidence is in `docs/release-dry-run-2026-11-22.md`.
- Command-line completion now has an `anvil-api` contract and a
  UI-neutral kernel policy in `anvil-session`; the TUI renders the kernel
  candidates for command, path, and buffer completion.
- `Space w` opens the start page focused on recent workspaces, including from
  an existing editor session.
- Selecting another workspace from `Space w` now performs a process handoff
  to that workspace; modified buffers block the switch.
- Opening a startup file no longer overwrites recent workspaces before they
  are loaded.
- Settings and language files now hot-reload with last-good fallback; an open
  Settings overlay defers the update until it closes.
- TUI selection markers, breadcrumb/status separators, and message dots now
  use one shared visual vocabulary; the command palette uses `›` instead of
  the previous `❯`.
- Start-page recents and the changed-files picker now use the shared
  file/folder icon registry.
- `Ctrl-w s` opens a horizontal split; mouse resize and focus routing follow
  the split orientation.
- Completion now fuzzy-filters all candidates with boundary/camel ranking and
  restores candidates after backspace instead of narrowing permanently.
- Insert-mode `Ctrl-n` triggers completion and remains IME-safe; `Ctrl-p` /
  `Ctrl-n` select the previous/next candidate while the popup is open.
- Shell startup files such as `.zshrc`, `.zshenv`, `.bashrc`, and `.profile`
  now use shell syntax highlighting, including `#` comments; extensionless
  config scripts are detected from their shell shebang. The same startup
  dotfiles now support `Ctrl-/` line comments.
- Lua `.lua` files now get Tree-sitter highlighting through
  `official-language-lua` / `language-pack-lua`, included in `full`; the
  existing `lua-language-server` profile is unchanged.
- The local i7-12700H workstation is now the authoritative performance
  reference; the current baseline is
  `docs/benchmarks/reference-2026-11-20-local-i7.md`. The planned replacement
  host is no longer a gate.
- `:recent-workspaces` now opens the same start-page workspace list as
  `Space w` instead of printing paths to the status line.
- `anvil-api` schema and TypeScript generation now sit behind the opt-in
  `bindings` feature; default runtime builds no longer pull `schemars` or
  `ts-rs`. CI runs the bindings-gated contract tests separately.
- Release layouts now write an install manifest and ship `uninstall.sh`; an
  install/upgrade/uninstall lifecycle smoke runs in CI and the release
  workflow. Uninstall removes only manifest-recorded files and preserves
  configuration and runtime state.
- The release workflow verifies checksums, the signature, the SPDX SBOM, the
  required installer/uninstaller archive entries, and published provenance
  before completing.
- `anvil doctor --json` now reports a structured `sandbox` capability object:
  Landlock ABI and signal scope, seccomp, cgroup v2, plugin/exec status, LSP
  trust, control transport, and deployment.
  `ANVIL_SANDBOX_PROFILE=auto|strict|off` selects the policy at startup;
  strict refuses degraded boundaries, and off warns without disabling the
  untrusted plugin boundary.
- Gateway startup resolves `anvil-exec-runner` with the same explicit,
  sibling, and `PATH` order as `doctor`; a PATH-resolved gateway no longer
  silently drops `sys.exec`.
- `anvil-mcp` now serves MCP `2026-07-28` and legacy `initialize` versions,
  with `server/discover`, structured tool output, annotations, output schemas,
  deterministic tool/list cache metadata, and revision-guarded edits.
- `anvil-ctl edit replace <range> <text> --expected-revision <N>` now exposes
  the guarded control edit path from the CLI.
- The problems panel is now a filterable diagnostics picker: type to filter by
  path, message, source, or code, `Ctrl-u` clears, and `Enter` jumps to the
  selected diagnostic. `Space d`, `:diagnostics`, and `:problems` open it.
- Added a filterable changed-files picker over the workspace Git status.
  `Space g` and `:changed-files` open it; `Enter` opens the selected path.
- Added a jump-list picker over back and forward navigation history.
  `Space j` and `:jumps` open it; `Enter` jumps to the selected entry.
- Added a bounded buffer-search history with a replay picker. `Space H` and
  `:search-history` open it; `Enter` re-runs the selected search.
- `Space :` now records Ex commands entered at the `:` prompt and persists up
  to 50 in the recent state; palette recents remain the fallback when no Ex
  history exists.
- Command palette modes now expose recent-command history (`Space :`); the
  command is `:command-history`. `Space K` and `:keymaps` open a keymap list
  covering normal, leader/pending, built-in keys by overlay, and configured
  context overrides; `:keybinds` opens the keymapped-commands palette subset.
- Added `:languages` / `Space L`, a read-only readiness page for syntax
  grammar availability, LSP profile/root markers/server lookup, formatter
  fallback, and unconfigured linter/test/debug surfaces.
- Added read-only `anvil setup` / `setup check` / `setup plan` discovery for
  language servers using the shared settings-aware resolver, including
  `ANVIL_LSP_*`, `[lsp.commands]`, and `[lsp.roots]`. `setup apply` installs
  missing user-level providers after confirmation; root/manual providers are
  blocked before changes. `--yes` enables non-interactive apply, and successful
  installs are recorded in `$XDG_DATA_HOME/anvil/tools/state.json`.
- Added build-time `language-pack-rust`, `language-pack-typescript`, and
  `language-pack-c-family` composition features; `full` includes all three.
- Added the opt-in `official-language-web` syntax feature for TypeScript,
  TSX, JavaScript/JSX, and CSS files and Markdown code fences. The `full`
  distribution feature includes it; default builds stay unchanged.
- Added the opt-in `official-language-c-family` syntax feature for C and C++,
  including Markdown code fences. The `full` distribution feature includes it;
  default builds stay unchanged.
- Release layouts now build the full official composition (`--features full`),
  including the web and C/C++ syntax packs; plain local builds remain standard.
- LSP workers, status, retries, and inlay-hint support are now keyed by the
  owning workspace root. Each worker opens its LSP connection through that
  root's gateway, uses the project root as cwd, and keeps `workspaceFolders`,
  `workspace/configuration`, and `didChangeConfiguration` in sync. User
  `[lsp.roots."<path>".<language>]` profiles can override command, args, root
  markers, env, and settings, and removing or untrusting a root stops only that
  root's workers.
- Added `anvil modules [--json]`, a read-only view of the compiled-in modules,
  contract range, command count, and source; no marketplace or network access.
- The workspace tree now uses case-insensitive natural ordering, filters
  `.git`/`target`/`.anvil-write-*` consistently in gateway and fallback paths,
  reveals collapsed ancestors with `:tree-reveal`, and supports tree-local
  find/navigation with `/` and `n`/`N`.
- Tree watcher changes now use generation-scoped per-directory listings instead
  of rereading every expanded directory; stale listings are discarded and the
  viewport keeps a three-row scrolloff with tree-focused mouse wheel movement.
- Added `:plugin-inventory`, a read-only installed-plugin page with id, version,
  enabled state, source, and digest; `:plugin-list` remains the mutable manager.
- `anvil-task` now keeps progress in per-task slots instead of one global
  `BTreeMap` mutex, adds cancellation-aware backpressure send helpers, and
  warns in debug builds when a dropped group still has running tasks.
- Comment toggling now covers C/C++/C#/Java/Kotlin/Swift, SQL/Lua, Ruby/PHP,
  common dotfiles, CRLF files, and rejects overlapping, out-of-bounds, and
  unsupported marker input instead of producing ambiguous edits.
- Large-workspace gates now cover 200 MiB open and scroll latency, and
  ADR-0044 records the polling/inotify comparison.
- `anvil-core` caches `PieceTable` length, uses O(1) end-insert/append paths,
  scans word motions over chunk streams once, and preserves/updates line-column
  checkpoints across edits.
- Workspace watching now covers open buffers, visible tree directories, and
  (when Git is provided by the distribution) `.git/HEAD`/`.git/index`.
  External clean edits reload and resync LSP, dirty edits still require
  confirmation, deletions send `textDocument/didClose`, and `.git`/`target`
  tree watches are ignored. Git metadata watches and status refreshes are
  enabled only when the distribution provides Git. External open-buffer edits
  schedule one 300 ms-coalesced Git status refresh instead of one request per
  file event. `[editor] file_watching` and `ANVIL_EDITOR_FILE_WATCHING`
  provide the disable switch.
- `anvil-distro-minimal` now declares `ConfigContract::SharedTui`; it reuses
  the TUI settings/theme/keymap parser instead of implying a second
  `anvil-config` consumer.
- Added `anvil doctor [--json] [--profile auto|dev|installed]` for
  installation and terminal diagnostics: companion binaries, locale,
  truecolor, runtime/config paths, Landlock/seccomp, and language servers.
  JSON schema version 1 has stable check ids/remediation; required failures
  exit `1`.
- Added SSH/tmux color-forwarding diagnostics to `anvil doctor` and a
  [remote/SSH workflow](docs/remote-ssh.md) guide covering remote install,
  environment forwarding, control sockets, and diagnostics.
- `:settings` now shows the effective origin of each value (`default`,
  environment, user file, or trusted project file), including ignored project
  files and the reason; `:config-reload` reports loaded and ignored files.
- Added `:settings-export [--effective]` and `:settings-import`. Export now
  writes the sparse user patch by default; `--effective` preserves the previous
  resolved-value output. Added `anvil settings schema` and the generated
  `share/anvil/settings.schema.json` release artifact.
- Added `:help recover` and [recovery semantics](docs/recovery.md) covering
  save durability, session/swap recovery, undo reset, and external conflicts.
- Added the gateway-side native workspace watcher API
  (`sys.watch_start`, `sys.watch_status`, `sys.watch_stop`) with bounded
  inotify directory watches, excludes, directory budgets, status, and
  coalesced `watch.events`. The TUI consumes the native stream for open-buffer
  change notifications and tree refresh; `[watch] mode`, `max_dirs`, and
  `cross_mounts` configure it, and `:watch-status` reports the active settings.
  Live `watch.status` updates now feed coverage, watched/skipped directory
  counts, overflow count, and the degraded reason into `:watch-status`.
  `ENOSPC`/`EMFILE` during registration enters a bounded cooldown instead of
  retrying in a tight loop.
  Polling remains the fallback.
- Added `:help start` and a first-run [onboarding guide](docs/onboarding.md)
  with a task list and compact keymap card.
- Command-based LSP code actions now dispatch through
  `workspace/executeCommand`, including actions that also carry a workspace
  edit. Sessions now fail restore on unreadable files, unreadable swap files,
  or a missing dirty swap instead of opening an empty buffer.
- Switching language servers now stops the previous gateway-supervised process
  before the replacement starts; failed starts name the missing server and
  point at `[lsp.commands]`.
- `anvil-module` now exposes a reusable `LanguageToolingProviders` composition
  for distributions that enable multiple language providers.
- Buffers now carry a stable, monotonically allocated identity; plugin snapshots
  and edits reject requests after the active buffer changes or while a read-only
  page or buffer is active.
- Workspace edits now stage resource operations, keep rollback backups for
  create/rename/delete, and restore completed files when a later operation
  fails. Read-only buffers and pager views reject workspace edits, and project
  replace rolls back failed writes in reverse order.
- `DocumentSession` no longer exposes `DerefMut`; mutation now goes through
  explicit methods, and plugin/format application uses guarded session helpers.
- `anvil-control` clients now return typed `OpenResult`, `SaveResult`, and
  `QuitResult` DTOs instead of raw JSON values.
- `anvil-control` exposes `ControlCommandId` and verifies manifest commands
  against capability lookup and TUI dispatch.
- `anvil-editor` now exposes additive `PositionError`/`EditError` types and
  `try_*` mutation methods; `anvil-session` mirrors the fallible surface.
- `DocumentSession` boolean mutation wrappers are deprecated; the TUI now
  calls the fallible `try_*` methods directly.
- Added the API surface snapshot gate and [API 1.1 ADR](docs/adr/0043-plugin-api-1-1.md);
  the existing plugin v1 contract remains unchanged.
- Buffer tabs and the buffer picker now show file-type icons; picker paths are
  workspace-relative instead of absolute.
- The status line adds the current file icon plus read-only and missing-file
  indicators. The completion popup now filters against the current prefix,
  ranks prefix matches first, uses right-aligned kind labels, a scrollbar, and
  a side details panel for documentation or auto-imports; accepting an
  auto-import completion applies the LSP additional text edit.
- Unified byte, scalar, grapheme, display-cell, and UTF-16 conversion in the
  line index. Editor mutations now reject invalid UTF-8 boundaries, LSP and
  workspace edits apply as one revision-checked transaction, and vertical
  movement, pointer hit testing, and bracket matching snap to cluster
  boundaries. Added randomized Unicode action-sequence property tests.
- Moved built-in language-server discovery into `anvil-lsp-client`, leaving
  only the settings-file override binding in the TUI. The single active LSP
  worker is retained across language switches; clangd and jdtls remain
  on-demand defaults. Completion labels highlight the matched prefix, inline
  diagnostics include LSP codes, workspace root labels and file-name
  validation moved to their domain owners.
- Added a manual real-server switching smoke for clangd, jdtls,
  rust-analyzer, and gopls with explicit skip when binaries are absent, plus a
  deterministic fake-gateway worker lifecycle and bounded-drop test in CI.
- Added `:lsp-status` with tool, root, markers, retries, and server info. The
  status bar now persists starting/running/restarting/failed/stopped state and
  clears stale worker state on language switches.
- Character selections now render a Helix-style selection background, a
  warning-colored primary anchor bar, and a block cursor at the selection head.
- Implemented the existing `event.subscribe` v1 method. The editor now
  publishes `host.event` notifications through the plugin host, and
  `diagnostics.changed` delivers coalesced path/generation/count/first-error
  snapshots with monotonic per-subscription sequences.
  Subscription count and pending delivery are bounded per plugin.
  Event definitions and payloads are registry-backed, the host command/event
  queues are bounded, and the first event for a subscription is buffered
  until Lua registers its handler.
  Event delivery coalesces queued `diagnostics.changed` values by path, exposes
  per-plugin delivered/dropped/handler-error stats, and includes a
  subscribe-to-Lua-to-ui.status end-to-end test.
- Added the std-only `anvil-task` crate with one bounded `TaskGroup` owner
  per background workload. It owns threads, coalesces progress, enforces
  live-task, per-task event, global event, and global byte limits, reports
  final states and leaked task ids, and shuts down with a timeout. The first
  migration slice, both short-lived TUI workers, and the LSP worker use it;
  LSP diagnostics are coalesced by path, and overflowed LSP responses return
  explicit errors. Oversized file results are reported as an `Oversized`
  event instead of stopping the worker, and `FileWorker`
  closes a cloned gateway stream on drop to interrupt blocked I/O.
- Added `anvil-process` with one `ProcessTask` owner per child process group.
  It supports piped stdio, process-group termination, graceful shutdown, and
  reaping; the DAP client and PTY shell use it. The plugin host retains its
  cgroup/pidfd owner so cgroup placement and cgroup kill happen before exec.
- Model teardown now stops task, file, LSP, DAP, and PTY workers before
  dropping gateway clients. `GatewaySession::shutdown_gracefully` sends
  `SIGTERM`, waits for exit, removes the socket once, and falls back to
  kill-and-wait after 250 ms.
- Added `scripts/architecture-check.sh` and a CI gate that rejects new raw
  production threads, new unbounded `mpsc::channel()` sites, and forbidden
  kernel/platform dependencies. Existing legacy sites use an explicit
  allowlist that shrinks as migration proceeds.
- Extracted `anvil-lsp-client` and `anvil-workspace` from `anvil-tui`.
  The LSP transport/worker and workspace file/search/preview/format services
  now have independent crate boundaries; TUI callers import them directly and
  the old local shim modules are removed.
- Imported the boxpigma theme family: `boxpigma`, `dracula`, `gruvbox`,
  `solarized`, `tokyo-night`, `catppuccin`, `monokai`, `rose-pine`,
  `kanagawa`, `github-light`, `gruvbox-light`, `one-light`,
  `catppuccin-latte`, `cyberpunk-volt`, `cyberpunk-fury`, and
  `cyberpunk-hot`.
- Trusted workspaces may add a read-only `.anvil.toml` project settings layer;
  it merges over the user file but cannot set `[lsp.commands]`.
- Added the std-only `anvil-platform` crate with a `Clipboard` trait, a
  Wayland/X11 system backend, and `MemoryClipboard`; the TUI now uses it.
- `anvil-syntax` now owns Tree-sitter grammars, highlighting, and scope
  extraction; `anvil-tui` no longer depends on the grammar crates directly.
- HTML and HTM files now use the Tree-sitter HTML grammar for tag, attribute,
  string, constant, and comment highlighting, with HTML fenced blocks in
  Markdown.
- `official.language-typst` adds a `tinymist` LSP profile for `.typ` files and
  Typst Tree-sitter highlighting through the `typst` syntax feature. It is
  opt-in through `official-language-typst` and introduces no new process path.
- LaTeX/Typst grammars are now feature-gated in `anvil-syntax`; feature-free
  builds no longer compile them. `official-language-latex` and
  `official-language-typst` enable the corresponding highlighting features.
- Added the UI-neutral provider foundation in `anvil-session`: provider ids,
  provenance, priorities, cancellation tokens, typed errors, and an ordered
  multi-provider registry. This is internal groundwork for the `0.6.0.x`
  provider protocol slices.
- The buffer picker now runs through the `PickerSource` protocol with the
  multi-provider registry; behavior is unchanged. This is the first real
  provider integration.
- Completion now carries a source id; LSP items are tagged `builtin.lsp`,
  while `builtin.buffer-words` and `builtin.paths` provide local buffer and
  gateway-enumerated path candidates. Results merge without duplicates.
- Added `sys.exec.v2` and `ExecRequestV2` for bounded one-shot tools with
  optional workspace-relative cwd, stdin, and output cap. Existing `sys.exec`
  callers are unchanged.
- Added the UI-neutral `FormatterProvider` protocol and `FormatService` with
  priority fallback, cancellation, and revision-checked replacement.
- `:format` now prefers bounded `rustfmt`/`gofmt` execution through the file
  worker and falls back to LSP formatting when no external formatter applies.
- Debug adapters now validate cwd/program paths and kill their process group on
  shutdown; runtime plugin DAP providers are rejected by the session manager.
- Added a builtin Rust `lldb-dap` provider; `:dap-attach` without an adapter
  selects it for `.rs` buffers.
- Added data-only API DTOs and a registry for runtime-plugin picker and
  completion providers, with namespace, duplicate-id, and plugin cleanup checks.
- Added a bounded provider request state machine with single-use request ids,
  cancellation, and item caps. Runtime plugins now receive picker/completion
  requests through the persistent host Lua runtime with transport-id
  correlation. Disabling or stopping a plugin now also cancels its pending
  provider requests, including queued supervisor calls. Plugin completion results are discarded when the editor
  revision or cursor changed while the host round-trip was in flight; picker
  plugin-picker overlay is reused for plugin results and Enter sends
  `provider.action` to the registered action handler.
- `anvil-distro-standard` adds a `full` feature that enables every official
  module at runtime. `--all-features` selects it; the default `anvil` binary
  remains the standard Git-only composition.
- Control sockets are now per workspace: `anvil`, `anvil-ctl`, and `anvil-mcp`
  select the instance by canonical workspace path instead of one global
  `control.sock`. File arguments outside the cwd derive their parent as the
  workspace.
- `editor.sidebar_percent` and `ANVIL_EDITOR_SIDEBAR_PERCENT` control the file
  tree width as a percentage (10-40%), shared by rendering, wrapping, and mouse
  hit-testing.
- The desktop entry registers common text and source-code MIME types, so
  Anvil appears in file-manager open-with menus.
- Plugin commands registered at runtime through `editor.register_command` now
  appear in the command palette and can launch their host. Ids are
  plugin-namespaced, duplicate ids are rejected, and the session is capped at
  512 dynamic commands.
- LSP navigation adds declaration (`gD`), type definition (`gy`), and
  implementation (`gI`) alongside definition (`gd`).
- Plugins can call `ui.notify` and `ui.status` through the Luau bridge.
  Notifications retain the existing severity, deduplication, and 50-entry cap;
  status messages are bounded to 1024 bytes.
- Official plugin packaging accepts `piv:SLOT` signing keys. The SHA-256
  digest is signed by `yubico-piv-tool` on the YubiKey, and `official.pub` is
  read from the same PIV slot; the existing PEM path remains unchanged.
- Quick-open can consume the optional `official.workspace-index` service,
  cancelling in-flight index builds on close and falling back to gateway file
  enumeration when the module is absent. The standard edition keeps the feature
  opt-in.
- The opt-in `official.desktop` module adds `:open-url <http(s)-url>` with a
  fixed `xdg-open` handoff, strict URL validation, and a bounded opener count.
  It does not add a general command runner or change gateway `sys.exec` policy.
- The module contract adds an additive `LanguageToolingService` slot. Language
  settings prefer a provider spec and fall back to the built-in catalog.
- The opt-in `official.language-latex` module supplies `texlab` profiles for
  LaTeX and BibTeX, and LaTeX documents get Tree-sitter syntax highlighting.
- `Alt-1` through `Alt-9` switch directly to the matching open buffer while
  leaving bare digits available as count prefixes.
- `Space b` and `:buffers` open a filterable buffer picker; `Enter` switches
  and background WorkspaceEdit buffers are excluded.
- Foreground buffers are capped at `editor.max_buffers` (default 9, max 99);
  opening or creating one beyond the cap reports
  `E: buffer limit reached (N)` instead of evicting anything. WorkspaceEdit
  ghost buffers live separately and do not consume these slots.
- Started the ADR-0024 state refactor: command palette, settings, theme
  browser, fuzzy, preview, debug panel, test explorer, command completion, and
  command-palette recents, and the pipe-location panel now live under
  `Model.overlays`; breakpoints, DAP runtime state, diagnostics, LSP workers,
  LSP config, restart backoff, request bookkeeping, edit snapshots, document
  versions moved under `Model.language`; completion/completion-resolve moved
  to `Model.overlays.lsp_results`.
- Settings is now a full page with top categories, a section sidebar,
  highlighted values, and a description footer.
- Theme choice is now part of typed `Settings` and `settings.toml` instead of a
  parallel parser; `:theme`, `:theme-auto`, previews, and `:theme-save` all
  persist through the settings registry.
- Help, Git diff/blame/conflict, task output, and terminal output now share
  `Model.pager`; the old `BufferState.help_back` fields are gone.
- Multi-root file tree roots use disambiguated short labels and mark untrusted
  roots; mutations refresh the owning root, keep expansion and pending
  selection, drop stale root results, and remove the root node, fuzzy entries,
  preview, Git state, and pending project search when a root is removed.
- The Settings overlay adds an Appearance theme row with live apply and reset.
- The command palette renders title/detail columns, a result count, and a key
  hint footer.
- `:themes` opens a two-pane theme browser with live preview, Enter apply, and
  Esc restore.
- Interactive overlays now share the focused border style; the debug panel
  carries its own key hints.
- Modern built-in themes: `tokyonight`, `omp-kde`, and `helix-base16`, all
  held to the existing WCAG-AA contrast test.
- Settings are grouped into Editor / Search / LSP / Accessibility, and active
  overlays use the focused border token.
- The start page supports mouse selection of recent files with the same
  trusted-root filtering as `:recent-files`.
- The start page lists openable recent files and `r` opens the recent-files
  picker; recent files outside trusted workspace roots are omitted.
- The settings overlay now also edits `ui.reduced_motion` and
  `ui.high_contrast`.
- Mouse-wheel scrolling now targets the split pane under the pointer and uses
  that pane's text width when advancing soft-wrapped display rows.
- Accessibility settings add `[ui] reduced_motion` and `high_contrast` (env
  `ANVIL_REDUCED_MOTION`/`ANVIL_HIGH_CONTRAST`); reduced motion freezes the
  spinner, high contrast lifts text/muted/border contrast without changing the
  selected palette.
- Startup detects truecolor from `COLORTERM`/`TERM` (override with
  `ANVIL_TRUECOLOR=0|1`); built-in themes are quantized to ANSI-256 when the
  terminal does not advertise direct color.
- Recent workspaces and files persist in `$XDG_STATE_HOME/anvil/recent.json`;
  `:recent-files` opens the fuzzy picker over recent files and
  `:recent-workspaces` lists recent workspaces.
- Fuzzy quick-open ranks basename matches first, then frecency, then shorter
  paths; opening a result records its usage.
- Theme files can style mode badges beyond normal: `mode_insert`,
  `mode_command`, `mode_search`, `mode_rename`, `mode_create`, and
  `mode_confirm` under `[status]`.
- Breadcrumbs render as a clickable row: the path segment reveals the file in
  the workspace tree, and the symbol segment opens document symbols.
- Sticky scroll shows the deepest enclosing LSP document symbol over the first
  visible source row; symbol data refreshes in the background after edits and
  path changes.
- Inline diagnostics append the highest-severity message to the end of the
  affected source line, only when horizontal space remains and not for
  horizontally scrolled rows.
- `:debug` opens a background-refreshed debug panel for threads, stack frames,
  scopes/variables, and watch expressions; queries never block the editor.
- Debug commands are registered in the shared command manifest, including
  `:breakpoint`, `:dap-attach`, and `:dap-detach`.
- Directory rows in the file tree roll up child git status and diagnostics,
  using the highest-priority marker/severity.
- `:dap-attach <adapter> [args]` initializes an external DAP adapter on a
  background thread, reports pending/timeout state, and syncs existing
  breakpoints; `:dap-detach` releases it.
- `:breakpoint` toggles a gutter breakpoint on the current line. Breakpoint
  state is per-file and mirrors through an attached DAP client.
- `editor.format_on_save` (`ANVIL_EDITOR_FORMAT_ON_SAVE`) saves normally, then
  requests LSP formatting and writes the formatted result if edits arrive.
  Missing LSPs or formatting errors keep the saved buffer and report a status
  warning; formatting never blocks the initial save.
- Auto-pairs for `()`, `[]`, `{}`, quotes, and backticks in Insert mode.
  Typing an existing closer skips it, Backspace removes an empty pair, and
  typing an opener over a selection wraps the selection.
- Opt-in soft wrap (`editor.soft_wrap` / `ANVIL_EDITOR_SOFT_WRAP=1`)
  projects long source lines into multiple display rows. Cursor and
  secondary-selection rows, mouse positioning, gutter numbering, scrolling,
  search/diagnostic underlines, and inlay placement use the wrapped
  projection; horizontal scroll is disabled while wrapped.
- The file tree marks modified open buffers with `*`, per-root git status with
  `M/A/D/R/C`, and files with diagnostics with `!`, using semantic colors.
- Custom themes load from `$XDG_CONFIG_HOME/anvil/themes/*.toml`
  (`ANVIL_THEMES` override). Theme files support hex, named ANSI, and
  `color0`..`color15` colors, syntax `bold`/`italic`, and
  `[status].mode_normal` colors; missing or invalid files keep the previous
  theme and report an error. Active custom themes reload on a one-second mtime
  poll and redraw the UI. `:theme list` lists built-in and custom themes.
- File tree and fuzzy-open icons use native Nerd Font glyphs with an ASCII
  fallback (`ANVIL_ASCII_MODE=1`) and width-aware padding. Project
  `.anvil.toml` `[ui.icons]` entries can override individual glyphs and reload
  on a one-second mtime poll.
- Named register storage moved from the TUI `Model` into `anvil-editor`.
  Frontends can read and write named registers through `EditorState`.
- Word, quote, bracket, paragraph, and tag text-object range semantics moved
  into `anvil-editor::EditorState`.
- Macro storage moved into `anvil-editor::EditorState` as frontend-neutral
  `MacroStep` entries; the TUI key adapter still performs recording/replay.
  BackTab round-trips; unrepresentable recorded steps report an error instead
  of being silently dropped.
- Quick-open now enumerates trusted secondary workspace roots through their
  owning `FileWorker`, labels them as `<root-name>/<relative>`, and routes
  preview reads through the same root.
- Project search fans out to every trusted root, waits for all answers, merges
  absolute paths with a global cap, deduplicates results, ignores stale
  generations after cancel, and opens secondary-root locations through the
  owning root.
- The file tree shows secondary workspace roots as top-level nodes, expands
  them through the owning worker or local fallback, splits tree watches across
  roots, and preserves secondary subtrees across primary refreshes.
- Create, rename, remove, and same-root copy/transfer route through the owning
  root. Cross-root rename/copy/transfer is rejected before mutation.
- `Ctrl+/` and `:comment` toggle line comments for the selected lines in
  Rust, Go, shell, Python, TOML, YAML, JSONC, Makefile, Dockerfile, and
  `go.mod` files. The operation is one undo transaction and refuses
  read-only buffers.
- `Ctrl-o`/`Alt-o` and `Ctrl-i`/`Alt-i` navigate a bounded 256-entry jump
  history across definitions, references, call hierarchy, symbols,
  diagnostics, quick-open, search, and stdin locations. A new jump clears the
  forward stack.
- `g;`/`g,` navigate a bounded edit-position change list. Key-driven edits
  record their before/after positions; `g;` skips the current position.
- `%` jumps to the matching bracket with a bounded lexical scan; matching
  bracket pairs are emphasized in the editor. Strings and line/block comments
  are skipped.
- The plugin host enables `LANDLOCK_SCOPE_SIGNAL` when Landlock ABI >= 6;
  seccomp remains the fallback signal control on older kernels.
- `:config-reload` reloads settings, keymap, and theme atomically. Invalid
  files keep the last good configuration; it does not restart LSP, plugins, or
  gateways.
- Opt-in `editor.auto_save` writes the active named buffer on buffer
  switch/close, split focus/close changes, opening another file, focus loss
  (including while Insert is active), and `:q`.
  Failed saves keep the
  buffer dirty and abort the focus change. Configure via `settings.toml`,
  `:settings`, or `ANVIL_EDITOR_AUTO_SAVE`.
- `anvil-ctl` is a JSON-capable control plane for a running TUI: `open`,
  `save`, `edit set|insert`, bounded `query`, allowlisted Ex `command`, and
  `quit`, with stable exit codes, timeouts, and typed errors. The control
  protocol is version 3, exposes capabilities in `status`, and supports
  revision-guarded range replacement and `edit --stdin`.
- `anvil-mcp` is a thin stdio MCP adapter over `anvil-control`, with
  capability-filtered tools and no direct workspace file access.
- `scripts/ctl-smoke.sh` and `scripts/mcp-smoke.sh` drive a real TUI through
  the CLI and MCP adapters and assert file, buffer, undo, Git index, and
  session state.
- Markdown (`marksman`), TOML (`taplo`), HTML, CSS/SCSS/Less, and Zig
  (`zls`) language-server discovery.
- LSP references panel on `gr`, with bounded sorted locations and cross-file
  jump-to-reference.
- LSP document highlights on `gh`, with stale-revision and buffer guards.
- LSP incoming/outgoing call panels on `gi`/`go`, with sorted bounded results
  and cross-file jumps.
- LSP folding ranges with `za`/`zM`/`zR`, folded-line rendering, and gutter markers.
- LSP semantic token colors with server legend mapping.
- LSP `completionItem/resolve` documentation for the selected completion item.
- LSP snippet completions with numbered placeholders and `Tab`/`Shift-Tab`
  navigation.
- `G` moves the cursor to the last line; `count G` jumps to that line.
- Grapheme-aware command-line and search input with Left/Right/Home/End/
  Delete, mid-string editing, paste insertion, and a physical terminal cursor.
- Shell highlighting for `.zsh`/`.brush` through the Bash grammar and `.fish`
  through `tree-sitter-fish`.
- `--editor` external-editor mode with a workspace outside the cwd, `:cq`
  exit status 1, and sticky save failures that survive `:q!`.
- stdin pipeline mode (`git diff | anvil` / `anvil -`), `/dev/tty`
  reattachment for keyboard input, `[stdin]` unnamed buffers, and
  `--read-only`.
- stdin location parsing for `path:line:column`, `--> path:line:column`, and
  `path(line,column)` diagnostics, with a bounded Locations panel, keyboard
  selection, mouse rows, and workspace-checked open.
- stable single-instance control socket (`$XDG_RUNTIME_DIR/anvil/control.sock`)
  with peer-UID checks, a held lock file, stale-socket cleanup, allowlisted
  remote commands, and the separate `anvil-ctl` binary.
- buffers now retain their own syntax spans, so switching back after global
  cache eviction or a stale parse keeps the last highlights instead of showing
  plain text (`BUG-010`).
- LSP completions honor `sortText`, `filterText`, and `preselect`; the preset
  item is selected initially and filtering uses server-provided filter text.
- snippet parsing preserves repeated tabstop indices; sessions group them as
  mirrors, navigate each unique tabstop once, and apply typed insertions and
  deletions to all mirrors.
- LSP code lens requests are bounded and rendered as virtual rows above source
  lines, with per-line and viewport truncation; clicking a lens sends
  `workspace/executeCommand` and refreshes the lens set.
- `:git-diff` opens a bounded unified diff against HEAD, with
  `:git-diff --staged`, hunk navigation, and `:git-stage-hunk` /
  `:git-unstage-hunk` applying one hunk to the index through a bounded patch
  file. The exec sandbox now permits the temporary-file syscalls Git needs.
- workspace sessions restore buffers, cursors, active tab, tree visibility,
  split ratio, and open split panes with focus; dirty buffers use atomic 0600
  swap files with mtime conflict detection, periodic snapshots, and preserved
  conflict swaps until resolved.
- `:git-blame` opens a bounded read-only blame view. Index-mutating
  `git apply --cached` is restricted to the editor connection; plugin
  connections are denied even with `sys.exec`.
- `:git-conflicts` lists unmerged paths from the index; conflict resolution
  operations remain separate.
- `:git-conflict-base`, `:git-conflict-ours`, and `:git-conflict-theirs`
  preview index stages with fixed `git show` arguments; missing stages fail
  visibly instead of falling back to working-tree markers.
- Restored buffers start with a fresh undo stack; offset-based edit records are
  not serialized, while exact buffer or swap text is preserved.
- External-file swap conflicts are resolved explicitly with
  `:conflict-recover` or `:conflict-discard`; saving an unmodified conflicted
  buffer, or saving it elsewhere, is refused until resolved so swaps are not
  discarded silently.
- `WorkspaceEdit` now applies `documentChanges` create/rename/delete
  operations, checks per-document LSP versions and request-time buffer
  revisions before mutation, and refuses overwriting open buffers or deleting
  modified buffers.
- Typed settings registry for `editor.scroll_margin`, `search.case_sensitive`,
  and `lsp.inlay_hints`, with env overrides and validation errors.
- `:settings` overlay with live preview, changed markers, reset, and atomic save.
- Luau `editor.get_view()` / `editor.apply_edit()` bridge over the host editor
  UDS, including a cursor-printing fixture.
- Signed `.anvil-plugin` installs via `anvil-gateway -install-plugin`, with
  Ed25519 verification, SHA-256 pinning, bounded gzip/tar extraction, and
  `fsroot` path confinement.
- Bundled official plugin discovery under `share/anvil/official/plugins`, with
  managed packages taking precedence and release-layout assembly.
- Shipped `install.sh` plus idempotent `anvil-gateway -install-bundled`
  bootstrap for signed official packages; upgrades preserve enable/disable
  state when permissions are stable.
- Tag-driven release workflow with signed `SHA256SUMS`, SPDX SBOM, and GitHub
  build-provenance attestation.
- Gateway package-download jobs with bounded concurrency, progress and failure
  notifications, capability review, trust-on-first-use key approval, and
  digest-pinned installation through `:pkg install <url>`.
- Typed `GatewayClient::fetch` for bounded installer-role `sys.fetch`
  requests, with response-status/body validation and a client-side body cap.
- Manifest-declared plugin commands in the command palette, with per-command
  host activation and `plugin.command` exposed to Luau.
- `editor.get_snapshot()` Luau binding and the first official plugin,
  `plugins/official/trim-whitespace`.
- Content-addressed plugin store with `state.json`, exclusive `state.lock`
  serialization, `pkg.installed` discovery, and install-without-enable.
- Installed-plugin manager overlay with enable/disable, confirmed uninstall,
  rollback, and store-size display.
- `:pkg install-file` local signed-package inspection/install and a separate
  fetch-only installer gateway role.
- Host stop/revoke before package update, with permission changes clearing
  grants and disabling the updated plugin.
- Semantic built-in themes (`anvil-dark`, `one-dark`, `gruvbox-dark`, `nord`,
  `catppuccin-mocha`, `solarized-light`), command-palette preview/selection,
  `:theme` / `:theme-auto` with OSC 11 / `COLORFGBG` detection, automatic
  persistence in `settings.toml`, and `ANVIL_THEME` startup selection.
- Tab completion for Ex command names, `:theme` / `:theme-auto` arguments, and
  `:plugin-enable` IDs at the `:` prompt, with a filtered candidate list,
  Up/Down selection, and Esc dismissal.
- `J` joins lines with indentation removed, and `>` / `<` indent or outdent
  the current line or selected lines by four spaces.

### Changed
- Split `anvil-distro-standard` CLI parsing into `args.rs` and `anvil doctor`
  diagnostics into `doctor.rs`; no behavior change.
- Settings saves now write only explicit user overrides, environment values
  are no longer persisted, and reset falls back to project/environment/default
  values. `ANVIL_THEME` is reported as a startup override. The settings
  overlay is registry-driven and now exposes `editor.sidebar_percent`.
- Gateway frame reads now have a 30-second deadline, LSP initialize has a
  10-second budget, failed LSP document syncs retain the latest text for the
  next poll, closing a document drops matching pending text, and server
  configuration/workspace requests receive protocol results instead of
  unconditional `null`.
- Editor transactions preserve same-offset insert order, and undo history is
  capped at 1,000 records or 50 MiB.
- Closing an LSP document removes its version counter, so reloads restart
  `didOpen` version tracking at 1; closing also drops matching pending sync text.
- Luau hosts enforce a 5-second CPU budget per entry, a 128 MiB Lua memory
  limit, and bounded event pending/sequence tables.
- Gateway copies are bounded by the source size observed at stat time;
  recursive removal rejects trees deeper than 128 levels; startup reclaims
  stale `.anvil-write-*` files older than 24 hours.
- Buffer-tab hit-testing now uses the renderer’s shared icon/marker/window/pager
  geometry; tab stops use one absolute-column model across wrapping, rendering,
  cursor placement, and mouse mapping.
- Tree git/diagnostic rollups are precomputed in one bottom-up pass instead of
  rescanning overlapping subtrees for each visible directory row.
- Custom theme color errors now degrade only the affected field, keep loading
  sibling files, and surface file/field context in status and the theme browser;
  high-contrast text selection now uses WCAG relative luminance.
- Project `[ui.icons]` tables no longer invalidate settings, invalid icon files
  keep the last valid overrides with a warning, and glyph overrides wider than
  two cells are rejected in favor of the built-in icon.
- The editor chrome now uses one persistent frame; the file tree keeps its
  stale/watch-limit/hidden header states without a nested border. Transient
  status and notifications share an auto-expiring bottom message band, while
  the status line keeps stable diagnostics/file/LSP/position segments. The
  start dashboard is denser, and the command palette shows category groups,
  recent markers, and direct key hints.
- Deduplicated the default keymap: removed numbered `Space` focus aliases,
  `Ctrl-o`/`Ctrl-i` jump aliases, `Alt-s`, and the alias-only Helix preset.
  `S` splits selections, `C`/`Alt-C` add cursors below/above, `Ctrl-/` is the
  canonical comment key, and the Space leader timeout is now 1200 ms.
- Removed the legacy single control socket and its client fallback; control
  clients now select the workspace-keyed socket. Removed the unsupported
  `Ctrl-v` block-selection key.
- Rebuilt the default `anvil-dark` theme with a deep-navy, steel, cyan,
  violet, and amber palette. Syntax colors now use stronger hue separation
  and targeted bold/italic/underline modifiers while keeping WCAG-AA contrast.
- Closed the `0.6.0` internal-polish phase and advanced the workspace to the
  `0.7.0` release candidate.
- Cursor visibility now uses the rendered text viewport instead of the full
  terminal, and indent guides reuse one visible-row projection per frame. This
  removes scroll jumps and repeated row layout on large files.
- The theme browser now scrolls its list to keep the selected theme visible
  with the keyboard or mouse wheel; previously the last themes were clipped
  below the panel.
- Extracted `TerminalSession` and `SelectionTable` into the reusable
  `anvil-tui-kit` crate; terminal startup now restores focus and mouse capture
  correctly when a later setup step fails.
- Extracted semantic themes into `anvil-tui-theme`; text search, snippets, and
  line comments into `anvil-editor`; workspace-root routing into
  `anvil-workspace`; and terminal key normalization into `anvil-tui-kit`.
  Compatibility re-exports keep existing paths working; `Model`, dispatch, and
  rendering stay in `anvil-tui`.
- Split TUI runtime/layout/plugin, renderer, LSP settings/responses, and tree
  navigation/mutations/fuzzy modules; split TUI tests by domain; split gateway
  handlers into `rpc.go`, `fs.go`, `lsp.go`, and `process.go`. No behavior
  change.
- Start-dashboard rendering moved from `render/editor.rs` into
  `render/welcome.rs`; `truncate_width` is shared from the render module.
- Split-pane and pipe-location rendering moved into `render/panes.rs`.
- File-tree rendering and its Git/diagnostic rollups moved into
  `render/tree.rs`; completion/LSP/settings/tool overlays moved into
  `render/overlays.rs`. `render/editor.rs` is now 357 lines.
- Bounded preview read/decode moved into `file_worker/preview.rs`; Git
  status/diff/blame/conflict/hunk helpers moved into `file_worker/git.rs`;
  file enumeration, project search, and replace helpers moved into
  `file_worker/search.rs`; request/event types moved into
  `file_worker/protocol.rs`; the worker loop and watchers moved into
  `file_worker/worker.rs`. `file_worker.rs` is now 355 lines.
- Model construction, config reload, plugin/keymap loading, dispatch, count,
  leader, read-only guard, and syntax polling moved into `model.rs`; LSP worker
  lifecycle, polling, and diagnostic helpers moved into `lsp_runtime.rs`;
  keymap context resolution moved into `keymap.rs`; TUI state types moved into
  `state.rs`. `lib.rs` is now 219 lines.
- Keymap tests moved into `keymap/tests.rs`; `keymap.rs` is now 1,182 lines.
- Editing operations split into `editing/objects.rs`, `editing/motions.rs`,
  `editing/selection.rs`, and `editing/input.rs`; `editing.rs` is now 62 lines.
- Built-in semantic palettes moved into `theme/palette.rs`; custom theme file
  parsing moved into `theme/custom.rs`; style accessors and color
  quantization moved into `theme/style.rs` and `theme/color.rs`. `theme.rs` is
  now 664 lines.

### Fixed
- Remote `anvil_command` no longer reports an unrelated pre-existing status
  error, such as a missing-Git `E: git status`, as a command failure. Status is
  cleared before dispatch, with a regression test.
- `anvil doctor` now derives Landlock status from the probed sandbox report
  instead of reading `/sys/kernel/security/lsm`; Snap and Flatpak sandboxes no
  longer report a false Landlock error when securityfs is hidden.
- Editor-originated saves no longer re-enter the workspace watcher as external
  changes. The watcher ignores events whose mtime matches the just-written
  file, so saving an externally watched buffer no longer prompts for a reload
  or marks the buffer conflicted.
- The initial file now opens after the primary gateway session is attached, so
  LSP workers started by that open can connect instead of failing with
  `lsp gateway connection unavailable`. The initial LSP connection created at
  gateway startup is reused when available.
- Switching between workspace roots with the same language now keeps the
  existing root's LSP worker; only tool or language changes stop the active
  worker.
- Atomic session and swap writes now remove their temporary file when write,
  sync, or rename fails. Recovery tests cover truncated session JSON and a
  failed save leaving the restored dirty buffer intact.
- Exhausted LSP crash retries now clear the pending retry timer instead of
  scheduling one more restart after the failure. A successful initialize
  resets the crash budget; Git status failures and stale generations are
  covered by tests.
- `anvil DIR` now opens `DIR` as the workspace instead of trying to open the
  directory as a file. Explicitly opening a directory reports `is a directory`
  instead of the gateway's generic `stat unavailable`.
- Git status no longer passes the unsupported `--relative` flag to
  `git status`. The gateway policy allows the exact read-only
  `git status --porcelain=v1 -z` invocation, so toggling the file tree no
  longer reports `git status: gateway error -32002`.
- `anvil-ctl` now caps `--stdin` reads at 4 MiB plus one byte, detects a
  prematurely exited `--spawn` child instead of waiting the full timeout, kills
  a timed-out child before returning, and honors `--timeout-ms` for the actual
  control request read/write.
- Plugin hosts no longer forward raw terminal control sequences: `print`,
  `host.event` handler failures, and Lua load/runtime errors all pass through
  one escaping sink that renders ESC/C0/C1 as `\xNN` and caps each line at
  8 KiB. Seccomp now requires `CLONE_THREAD` for `clone`, denies `fork`,
  `vfork`, and process-style clone flags, and reports `clone3` as `ENOSYS` so
  glibc still falls back to `clone` for normal thread creation.
  `HostProcess::terminate` falls back to the process group when the pidfd
  signal fails and reaps with a 2 s bound, and the spawn `pre_exec` path no
  longer allocates.
- The control server now bounds concurrent client threads, keeps socket write
  timeouts and whole-frame deadlines on accepted and client streams, and maps
  frame timeouts to a typed control timeout instead of a generic I/O error.
- Editor service connections have an admission cap, handshake and per-frame
  deadlines, and a bounded request queue; the plugin host reconnects after an
  idle close without replaying mutating editor calls. Host RPC rounds use a
  30 s deadline and skip stale response ids instead of failing the session.
- DAP attach and refresh results carry a generation token so stale workers
  cannot replace a newer client; concurrent attaches are rejected, unsolicited
  events are capped, and timed-out attach/refresh workers kill the adapter
  process group before being joined.
- The Helix preset fallback binds `Space 3` to the right split instead of
  leaving it unbound.
- Bracket lookup and caret motion clamp to UTF-8 character boundaries,
  fixing a crash on CJK lines when pressing Down near multibyte text.
- The settings overlay now shows its key bindings, including `s` to save and
  `Esc` to cancel, so theme changes are not silently discarded.
- `?` opens the help page from normal editor mode, not only the start
  dashboard.
- Panel borders now use padded titles and rounded corners; the focused panel
  and active split separator use a heavy border for clearer focus. The current
  line number uses the accent color and bold weight on the same current-line
  background band while relative numbers stay muted. The indentation guide at
  the current line's indent level is drawn in the accent color.
- Split view now gives the active editor its own half of the available width
  instead of drawing the second pane over a full-width active editor. Sidebar
  width is shared by rendering, soft wrap, and mouse hit-testing. Panes share
  one title row and a single separator, and soft-wrap width is refreshed when
  the tree or split geometry changes, including closing the tree. Split panes
  now wrap long lines with the same soft-wrap behavior as the active pane.
- LSP discovery now also searches the executable directory and common user
  bin directories, so desktop-launched sessions find tools such as `marksman`.
- The Go toolchain requirement is now 1.26.11, fixing
  GO-2026-6859 in the standard library `net/http` server.
- Signed plugin indexes and revocation documents now return
  `ErrDocumentExpired` after `expires_at`; the parsed document remains
  available for stale cached browsing but is rejected for new updates.
- Installed plugin manifest hashes now cover the exact `plugin.json` bytes
  instead of a re-encoded manifest.
- Package downloads now enforce an indexed digest, package size, publisher
  key, manifest hash, and permission set when those values are supplied.
- The file tree now opens the command line on `:`, so commands such as
  `:workspace-remove` work while the tree has focus.
- Cursor motions, including `j`/`k`, section jumps, `G`, `w`/`b`/`e`, and
  `h`/`l`, now skip closed folds instead of landing on hidden lines and forcing
  the fold open. Nested folds move between their outermost visible rows, and
  count motions preserve the same folds.
- Settings edits no longer replace the active `ANVIL_THEME`; theme changes are
  applied only when the settings theme value changes, cancel restores the
  active theme, and unrelated saves keep the file theme.
- Backspace after wide/emoji input no longer panics when stale Tree-sitter
  highlight boundaries fall inside a UTF-8 character; span slices clamp to
  grapheme boundaries.
- Mouse tab switches now sync the current buffer before loading the target;
  `:w` followed by a tab round-trip no longer restores a stale dirty snapshot.
- `HostSession` now receives the editor RPC socket from the supervisor and
  passes `--editor-socket` to plugin hosts, so production `editor.*` calls
  reach the active buffer instead of returning `-32008`.
- TypeScript LSP startup now passes a `tsserver.path`, using the project
  compiler, the global `tsc` installation, or `ANVIL_TSSERVER`.
- Inlay hints are requested only after the server advertises
  `inlayHintProvider`, avoiding `Method not found` noise from Markdown and
  other servers without that capability.
- The editor RPC socket now authenticates each host with `session.hello`
  (`plugin_id` plus the per-host token) before accepting `editor.*` requests.
  Command registration, source attribution, and notifications use the
  authenticated identity instead of inferring a plugin from the command id or
  trusting request parameters. Identity is registered only after gateway
  registration succeeds and is dropped when the host is reclaimed.

## [0.6.0] - 2026-11-06

### Added
- Plugin API v1 (`1.0.0`) with complete DTOs for every catalog method,
  generated JSON Schema, TypeScript, and LuaLS stubs.
- Test-only plugin-host editor proxy for `editor.get_view`, `get_snapshot`,
  `set_mode`, and `apply_edit`, with DTO/revision/error/cancellation/timeout
  parity tests. `HostSession` wiring remains pending, so normal startup still
  answers `editor.*` with `-32008`.
- `session.hello` now negotiates API `1.0.0`; plugins declare `api` as `1`,
  `1.0`, or `1.0.0`.
- Manifest path frozen to `plugin.json`; the v1 contract is documented in
  `docs/spec/plugin-api-v1.md` and ADR-0014.

### Changed
- Plugin API manifests using the `0.1` line are no longer accepted.

## [0.5.10] - 2026-11-06

### Added
- Sparse column checkpoints for lines over 64 KiB, keeping cursor column
  lookup bounded to one 16 KiB checkpoint window.
- `ANVIL_SKIP_TERMINAL_QUERY=1` bypasses crossterm's blocking keyboard
  enhancement query.

### Fixed
- Horizontal viewport rendering now materializes only the visible line window,
  including cursor-at-end and large-hscroll cases.
- Single-line inserts update only the affected line chunk instead of rebuilding
  the line index; 100 MB A1 p99 fell from 105 µs to 2.3 µs.
- 100 MB A3 merge-delete p99 fell from 175 µs to 92.6 µs.
- Horizontally windowed lines preserve diagnostics underlines and visible
  inlay hints instead of dropping annotations.


## [0.5.9] - 2026-11-06

### Added
- Mouse capture with a single pointer state machine, wide-character hit-testing,
  click-to-focus, drag selection with edge scrolling, wheel scrolling, split
  resize, overlay click trapping, and an ASCII icon/severity fallback.

### Fixed
- Mouse drag selection and split resizing now share the editor layout and
  account for the tree sidebar.
- Clicking outside command palette, fuzzy finder, code actions, symbols, and
  completion popups now closes them.
- Search no longer builds a full-document grapheme-boundary set; 100 MB
  search dropped from 15 s to 22–30 ms.
- Split hit-testing no longer overflows at extreme terminal widths.
- Dense searches now cap at 10,000 matches, and oversized global
  substitutions fail instead of accumulating an unbounded range vector.

## [0.5.8] - 2026-11-06

### Added
- Debounced incremental literal/regex search with case and whole-word options,
  multi-selection matches, current/total counts, `n`/`N`, and Esc cursor
  restoration; replace one/all are atomic and read-only safe.

## [0.5.7] - 2026-11-06

### Added
- Bounded, deduplicating notifications; non-blocking progress with completion,
  failure, and cancellation; a problems panel sourced from the existing LSP
  cache; and Diagnostic Peek v2 with merged same-line diagnostics, related
  locations, revision pruning, focus restoration, rendered-line scrolling, and
  cursor-safe narrow-viewport placement.

## [0.5.6] - 2026-11-06

### Added
- Buffer tab bar with active, dirty, missing-file, and read-only states,
  active-centered overflow scrolling, and per-buffer visual selections.
- Split focus and close restore missing/visual buffer state and resync the
  active tab.

## [0.5.5] - 2026-11-06

### Added
- Word, quote, bracket/parenthesis/brace, paragraph, and tag text objects
  support `d`/`y`/`c`; word and paragraph objects honor counts.

- `v`/`V` map character and line selection onto the selection-first model;
  block selection is rejected explicitly.

- Visual selection supports operation-scoped surround: `s` then a bracket,
  quote, or backtick wraps the selection; `Esc` cancels.

- `d`, `y`, and `c` over word, quote, bracket, paragraph, and tag objects now
  share one transaction path, keeping register and undo behavior aligned.

- Quick-open previews the selected file through the gateway worker, capped at
  64 KiB, 200 lines, and 500 ms; stale generations are discarded. Previews sit
  beside the list when wide and stack below it when narrow; failures also
  surface in the status line. File icons come from name/extension rules with a
  width-aware text fallback. `anvil --help` documents the quick-open preview and
  the gateway/ripgrep overrides.

- Breadcrumbs use workspace-relative paths with middle ellipsis and fall back to
  cached Tree-sitter scopes when no symbol is selected. The status bar now uses
  ordered segments with width caps, a 200 ms diagnostic hysteresis, and a
  cached git branch label.

- Markdown fenced code blocks render with a full-width background, and inline
  code spans use the same subtle background without replacing syntax colors.

- Semantic last-change repeat: `.` replays the last successful delete, change,
  or insert session; an explicit count overrides the recorded delete count.
  The scope is session-wide and applies to the active buffer.

- Space leader is deadline-bound and exposes `Space 1` focus editor /
  `Space 2` file tree; user keymaps may bind `f1`–`f12` while defaults remain
  F-key free.

- Keymap context identifiers define strict overlay/command-line/editor/tree
  routing priority, with a derived context-stack projection for the TUI.
  Contextual TOML sections now validate commands and apply editor/global
  bindings, and bundled ergo/Vim presets can be selected as a config base.

- Pending prefixes render a dynamic which-key panel from the active keymap
  continuations, including `i`/`a` text-object extenders and their nested
  object actions.

- `Space p` opens a command palette backed by the shared command registry with
  derived titles, query filtering, context labels, and recent-command ordering.

### Fixed

- Empty insert sessions no longer replace the previous last change; backspace
  removes the deleted text from the captured insert session.
- Bracketed paste is enabled while the TUI owns the terminal and disabled on
  exit or unwind; startup cleanup restores the alternate screen. Pasted CRLF/CR
  is normalized, and multi-line paste preserves line breaks and undoes as one
  transaction.
- Paste input strips control characters and is capped at 8 MiB; insert sessions
  are invalidated after cursor movement or selection deletion so `.` cannot
  replay a stale partial edit.
- Added regression coverage for repeated backspace at document start/line
  boundaries and for wide-character display-column motion.

## [0.5.0] - 2026-11-04

### Fixed

- `C`/`Alt-C` multi-cursor creation now seeds the active caret from the moved
  cursor instead of a stale file-start selection.
- Alt-modified ASCII letter bindings normalize case before dispatch, so
  terminals reporting either `Alt+C` or `Alt+c` resolve to the same action.
- Repeated key events are dispatched like presses, so keyboard protocols that
  report hold/repeat do not lose autorepeat.
- `Esc` in normal mode merges multiple cursors back into the primary cursor.
  Insert-mode `Esc` only returns to normal mode and preserves the cursors.
- Text-entry and pending states reject Ctrl/Alt/Super-modified characters
  instead of treating them as plain input.
- Command matching uses a shared key wrapper for modifier flags and Alt
  character normalization.
- Ignored key-release events no longer count as syntax activity.
- `0` now moves to the line start when no count is pending instead of being
  consumed as a count.
- Pending change, split, and space-prefixed commands consume the count, so
  `2cw` does not leak the count into the next motion.
- Syntax highlights use a 32-entry LRU path cache and are restored before the
  first frame; uncached files keep the normal text color until parsing completes.
- Inlay hint viewport ranges clamp to the last line, avoiding the language
  server's invalid EOF offset error.
- Workspace tree entries include inode identity, so selection follows an
  external rename when the new path remains visible.
- `plugin.json` now reads the documented camelCase `activationEvents` key;
  the previous snake_case spelling remains accepted as an alias.

### Added

- Plugin activation matching for `onStartup`, `onLanguage`, and
  `onWorkspaceContains`; matching hosts start with an empty grant set.
- `onStartup` activation requires `ANVIL_PLUGIN_ALLOW_STARTUP=1`; workspace
  activation can be disabled with `ANVIL_PLUGIN_DISABLE_WORKSPACE_ACTIVATION=1`.
- Multiple plugin hosts keyed by plugin id, with per-plugin status and revoke
  on stop or failure.
- Plugin host supervisor with explicit lifecycle states, single-flight startup,
  bounded running and queued-start caps, start timeout, crash backoff,
  queue-until-ready requests, drain-before-kill, and response-id validation.
- Plugin hosts are placed in a unique cgroup v2 before `execve`; memory,
  memory.swap, pids, and CPU limits are mandatory. Host stop uses `pidfd`,
  falls back to `cgroup.kill`/process groups, and disables core dumps.
- Idle plugin hosts are drained and stopped after 60 seconds without activity;
  their token, socket, and cgroup are removed.
- The plugin host connects its editor Unix socket before Landlock/seccomp, so
  the post-sandbox transport no longer needs `socket` or `connect`.
- Plugin-host seccomp now denies signal delivery, SIGIO registration,
  memory-execution, namespace/mount, introspection, `clone3`, and
  post-sandbox socket/connect paths; `clone` rejects namespace flags.
- API method catalog (`METHODS`) plus document snapshot and apply-edit DTOs;
  generated JSON, TypeScript, and Lua method bindings are driven from the same
  table.
- Plugin hosts now preconnect the authenticated gateway before sandboxing and
  forward `sys.*` requests through the gateway token and grants.
- Editor-side plugin document contract: active-buffer snapshots and
  revision-checked transactions; stale or invalid edits are rejected without
  mutation and reported in the status bar.
- Host `session.hello` now carries `api_version`; plugin manifests declare and
  validate the supported `0.1` API line before loading.
- `anvil-session` owns active document state, the command registry, focus and
  overlay state, revision-checked plugin transactions, plugin host lifecycle,
  and the bounded effect queue. The TUI is the input adapter and renderer.
- Repeated host failures disable a plugin until `:plugin-enable <id>` is run;
  `:plugin-list` reports each discovered plugin's host state.
- Language activation is deduplicated per plugin; workspace-marker activation
  reads workspace root entries through the gateway.

### Changed

- Keyboard enhancement flags are enabled only when the terminal reports
  support, and popped on exit. Unsupported terminals keep the legacy key path.

- Normal-mode and operator-pending bindings resolve through a dedicated keymap
  module; terminal key normalization lives in an input adapter.
- Normal and operator-pending commands have stable namespaced IDs and a
  read-only registry. An optional TOML keymap validates overrides against those
  IDs and applies them atomically.
- Debug plugin hosts now register a per-session grant before spawn and revoke
  on stop or failure through a dedicated plugin session coordinator.
- Local plugin manifests are discovered from `ANVIL_PLUGIN_DIR` with duplicate
  id rejection; activation is not wired yet.
- Command IDs and the read-only command registry now live in `anvil-session`;
  the TUI keeps only key resolution and keymap state.
- `anvil-session::effect::EffectQueue` now provides bounded typed effects;
  Ex commands, key-driven LSP requests, tree filesystem operations, save/reload,
  plugin commands, and gateway reconnect route through it. Internal lifecycle
  calls still execute their implementation helpers directly.
- Dependency baselines moved to criterion 0.8, tree-sitter 0.26, sha2 0.11,
  ts-rs 12, base64 0.23, `golang.org/x/sys` 0.49, and setup-node 7. The
  Landlock ruleset uses `Handled_access_fs`; tree-sitter child indexes are
  `u32`.

## [0.4.5] - 2026-11-02

This release bundles the `0.4.4` gateway capability control plane and the
`0.4.5` manifest/host-loading slice.

### Added

- `plugin.register`/`plugin.revoke` on an editor-authenticated gateway session.
- Per-plugin token hashes and grants; plugin handshakes receive only their own
  permission set, and revoke immediately removes access.
- `GatewayClient::register_plugin`/`revoke_plugin`.
- Bounded `plugin.json` manifest validation and a sandboxed `--plugin <dir>`
  host path that loads one Luau entry after token read and sandbox setup.
- Plugin-root read access in the host Landlock rules; manifest validation
  rejects escaping entries, duplicate commands, and unknown activation events.

## [0.4.3] - 2026-11-01

### Added

- Markdown highlighting with `tree-sitter-md` block and inline grammars.
- Heading, link, code, emphasis, list, quote, table, and frontmatter classes.
- Fenced code blocks use the nested language grammar when available.

### Fixed

- TOML keys no longer color the entire key/value pair; strings and values keep
  their own classes.
- Tree watcher smoke instructions now create a new file instead of touching an
  existing file.

## [0.4.2] - 2026-11-01

### Added

- Tree-sitter coverage for TOML, Go, Bash, Python, YAML, JSONC, Makefile, and
  Dockerfile, detected by extension or by the `Makefile`/`Dockerfile`/`go.mod`
  file name.
- A bounded `go.mod` directive/comment/string highlighter.

### Changed

- Syntax queries are compiled once in a static registry instead of on every
  parse request; Rust keyword walking is limited to Rust.
- JSONC comments and booleans now use the JSON grammar's comment support.

### Fixed

- Unsupported formats continue to fall back to plain text, and the existing
  256 KiB highlight limit still applies.

## [0.4.1] - 2026-11-01

### Added

- Workspace tree watcher: the gateway exposes `sys.read_dir_signature`, and
  the TUI polls the root plus expanded directories through `FileWorker`.
  External create, delete, and rename now refresh the tree while preserving
  selection and expansion; watchers are capped and marked stale when the
  gateway cannot observe the tree. See
  [ADR-0018](docs/adr/0018-workspace-tree-watch.md).

### Fixed

- In-flight tree snapshots no longer collapse a directory expanded after the
  snapshot was requested; missing listings queue a follow-up refresh.

### Changed

- Added [INSTALL.md](INSTALL.md) with source build, sibling-binary, runtime,
  verification, and uninstall instructions.

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

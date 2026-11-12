# Changelog

All notable changes to Anvil are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Pre-1.0 version
lines are release-series labels, not a compatibility promise; `0.5.x` is the
post-`0.5.0` feature line.


## [Unreleased]
### Added
- Started the ADR-0024 state refactor: command palette, settings, theme
  browser, fuzzy, preview, debug panel, test explorer, command completion, and
  command-palette recents, and the pipe-location panel now live under
  `Model.overlays`; breakpoints and DAP runtime state moved under
  `Model.language`.
- Settings is now a full page with top categories, a section sidebar,
  highlighted values, and a description footer.
- The command palette renders title/detail columns, a result count, and a key
  hint footer.
- Saving a theme edits only `[theme]` and preserves every other settings
  section.
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

### Changed
- Version frozen at `0.6.0` for long-term internal polish; no release tags or
  public releases during this phase.
- Split TUI runtime/layout/plugin, renderer, LSP settings/responses, and tree
  navigation/mutations/fuzzy modules; split TUI tests by domain; split gateway
  handlers into `rpc.go`, `fs.go`, `lsp.go`, and `process.go`. No behavior
  change.

### Fixed
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

# Anvil

Anvil is a Linux-first terminal editor. The project targets a reusable Rust
kernel, a per-session Go policy gateway, and a capability-scoped plugin host.
The official standard edition is one composition; third-party distributions
can compose the kernel with their own UI and trusted Rust code. Runtime plugins
remain untrusted and sandboxed.

Status: **0.6.0 frozen for long-term internal polish; no public release or
tag during this phase**. See [internal polish plan](docs/internal-polish-plan.md).
Development uses Rust from `rust-toolchain.toml` and Go **1.26.10 or newer**;
CI installs the exact Go version from the root `go.mod`. The ratatui editor now
includes insert
mode (`i`/`a`/`A`/`o`/`O`), multi-cursor selection (`s`/`Alt-s`/`C`),
`:w`/`:wa`/`:wq`/`:w file`/`:q!`, an unsaved-change guard, undo/redo,
`x`/`dd`/`yy`/`p`, `w`/`b`/`e` word motions with
`dw`/`db`/`de`/`yw`/`yb`/`ye`, `G` to the last line, literal `:s`/`:%s`
substitution, `/` search
with `n`/`N`, multi-buffer switching, vertical splits with independent pane cursors,
read-only `:git-diff` views with hunk staging, workspace session restore,
dirty-buffer swap conflict commands, and a fresh
undo stack after restore, horizontal scrolling, and dirty-only redraws,
insert-mode Tab indentation, a workspace file tree, and a fuzzy file
finder. Rust, TOML, Go, Bash/Zsh/Brush, Fish, Python, YAML, JSON/JSONC,
Markdown, Makefile,
and Dockerfile use Tree-sitter highlighting on a debounced background thread,
capped at 256 KiB so large files keep plain-text latency.
Trusted language servers (rust-analyzer, gopls, pyright/pylsp,
typescript-language-server/vtsls, clangd, jdtls, lua-language-server,
vscode-json-language-server, yaml-language-server, bash-language-server,
marksman, taplo, vscode-html-language-server, vscode-css-language-server, zls)
run without the plugin sandbox; diagnostics, completion with resolved
documentation and snippets (`Ctrl+Space`),
definition (`gd`), references (`gr`), document highlights (`gh`),
incoming/outgoing calls (`gi`/`go`), folding ranges (`za`/`zM`/`zR`),
semantic token colors,
hover (`K`), rename (`Space r`), code actions/quick fixes (`Space a`),
formatting (`Space F`), document symbols (`Space s`), signature help, and
workspace symbols (`Space S`) use canonicalized LSP UTF-16 positions. Rename,
code-action, and formatting edits go through a shared `WorkspaceEdit` engine
that checks document versions and request-time buffer revisions, applies
`documentChanges` create/rename/delete operations, keeps unopened files as
background buffers, and does not write files during an edit; `:wa` saves every
modified buffer.
Supported files start their language server automatically; each
`(tool, language_id)` keeps its own worker while the session is open.
Language servers are not bundled; install them on `PATH`.

- TypeScript prefers a project-local compiler (`npm install -D typescript`).
  Without one, the global `tsc` installation is used; override the compiler
  path with `ANVIL_TSSERVER=/path/to/tsserver.js`.
- C/C++ support works best with a compilation database: `bear -- make`, or
  configure CMake with `-DCMAKE_EXPORT_COMPILE_COMMANDS=ON`. Without it, clangd
  may report unresolved includes.

Built-in semantic themes are `anvil-dark`, `one-dark`, `gruvbox-dark`,
`nord`, `catppuccin-mocha`, and `solarized-light`. Use `:theme` to list them.
`:theme <name>` and `:theme-auto [dark] [light]` switch and persist to
`$XDG_CONFIG_HOME/anvil/settings.toml` (override with `ANVIL_SETTINGS`);
`:theme-save` re-saves the runtime choice explicitly.
`ANVIL_THEME` selects the startup theme.
The command palette (`Space p`) lists themes and previews them while arrowing.

The same file owns typed editor/search/LSP settings:

```toml
[editor]
scroll_margin = 3

[search]
case_sensitive = false

[lsp]
inlay_hints = true
```

`ANVIL_EDITOR_SCROLL_MARGIN`, `ANVIL_SEARCH_CASE_SENSITIVE`, and
`ANVIL_LSP_INLAY_HINTS` override the file for the current process.
`:settings` opens the minimal overlay: `j`/`k` select, `h`/`l` or `Enter`
adjust, `r` resets the row, `s` saves, and `Esc` cancels.
External file edits are read at startup; restart Anvil to apply them.

At the `:` prompt, `Tab` completes Ex command names, `:theme` /
`:theme-auto` arguments, and `:plugin-enable` IDs; `Up`/`Down` selects a
candidate and `Enter` accepts it. `Esc` closes the completion list first.

The plugin-host path has a sandboxed host with token-FD delivery,
`close_range`, `no_new_privs`, Landlock read/no-exec rules, native-ABI seccomp
with a preconnected editor socket and dangerous-syscall denial, cgroup-before-exec v2 limits with OOM classification,
`sys.ping`/`sys.exec` dispatch, and a TUI debug `PluginSession`
(`:plugin-start`/`:plugin-exec`/`:plugin-stop`/`:plugin-oom`).
Official release layouts install plugin sources under
`share/anvil/official/plugins` and, when signed, packages under
`share/anvil/packages`. `install.sh` copies them and runs
`anvil-gateway -install-bundled`; managed packages take precedence over the
bundled fallback and support enable/disable, uninstall, and rollback.
`:pkg install <url>` downloads a signed package through the gateway, shows its
declared capabilities, and installs only the reviewed SHA-256 digest. Downloads
require `-allow-fetch` and an explicit `-fetch-allow` host.
`:pkg install-file <package> <signature> <public-key>` reviews and installs a
local signed package without network access.
For development, point `ANVIL_PLUGIN_DIR` at `plugins/official`, open the
command palette (`Space p`), and run `Trim Trailing Whitespace`.
`onLanguage` and `onWorkspaceContains` events start matching sandboxed hosts
with no grants. `onStartup` requires explicit opt-in via
`ANVIL_PLUGIN_ALLOW_STARTUP=1`; workspace activation is disabled with
`ANVIL_PLUGIN_DISABLE_WORKSPACE_ACTIVATION=1`. Manifest-declared `onCommand`
entries are dispatched from the command palette; `editor.register_command` is
still pending. The optional `LANDLOCK_SCOPE_SIGNAL` layer is also pending.
This is not a VM or public plugin API.

Phase 0 still consists of the `anvil-api` contracts, `anvil-core` PieceTable,
`anvil-editor` snapshots, `anvil-ipc` framing/JSON-RPC/Unix transport, and the
Go gateway with `openat2`, Landlock/seccomp exec sandboxing, SSRF-constrained
fetch, and a synchronous Rust gateway client. The TUI discovers the gateway
beside the binary or via `ANVIL_GATEWAY`.

Set `ANVIL_SKIP_TERMINAL_QUERY=1` for CI or synthetic PTYs; it bypasses
crossterm's keyboard-enhancement query and the OSC 11 background query.

## Usage

```text
anvil [OPTIONS] [FILE|-]

  -w, --workspace <DIR>  Workspace directory; --workspace=DIR is also accepted
  --editor              External-editor mode; allows one explicit file outside
                        the cwd by using its parent as the default workspace
  --read-only           Reject mutating commands
  -h, --help             Show help and exit
  -V, --version          Show the version and exit
  --                     Treat the next argument as FILE
```

`FILE` must be inside the selected workspace. `--editor` is intended for
`$EDITOR`-style invocations; `:cq` exits 1 and a failed save keeps the final
exit status nonzero. `anvil -`, or any non-terminal stdin such as
`git diff | anvil`, creates a `[stdin]` buffer and reattaches keyboard input to
`/dev/tty`; combine with `--read-only` for logs. Use `--` before a file name
that starts with a dash. Launching without `FILE` shows the start dashboard;
the first key dismisses it.

Stdin text containing `path:line:column: message`, `--> path:line:column`, or
`path(line,column): message` shows a bounded Locations panel. `Alt-j` /
`Alt-k` selects a row and `Enter` opens it; locations outside the workspace
are rejected.

## Control

`anvil-ctl` controls the running instance through
`$XDG_RUNTIME_DIR/anvil/control.sock`:

```text
anvil-ctl status
anvil-ctl open src/main.rs +12:3
anvil-ctl exec format
```

`open` is workspace-relative; `exec` accepts only `format`, `w`, `wq`, and
`q`. A second `anvil FILE` inside the active workspace forwards to the running
editor and exits; a different workspace is rejected. `--spawn` starts Anvil
only when a terminal is attached. `--editor` and stdin instances do not claim
the control socket, so they can run alongside a normal interactive instance.

Quick open: `Space f` filters workspace files and previews the selected file
through the gateway. Wide terminals show the preview beside the list; narrow
terminals stack it below.

## Documents

- [Installing Anvil](INSTALL.md)
- [Documentation index](docs/README.md)
- [Project charter](PROJECT_CHARTER.md)
- [Known issues](docs/known-issues.md)
- [High-frequency command architecture plan](docs/command-architecture-plan.md)
- [0.3.0 → 0.4.0: Polish & Specification](docs/polish-and-specification.md)
- [0.3.0 → 0.4.0 / Phase 0.7 plan](docs/phase-0-7-plan.md)
- [0.4.x feature plan](docs/0.4.x-plan.md)
- [0.5.x interaction and UI plan](docs/0.5.x-plan.md)
- [0.6.x distribution and marketplace plan](docs/0.6.x-plan.md)
- [0.7.x official optional Rust modules plan](docs/0.7.x-plan.md)
- [Linux platform plan](docs/linux-platform-plan.md)
- [Keymap configuration](docs/keymap.md)
- [Plugin-host isolation ADR](docs/adr/0012-plugin-host-isolation.md)
- [Threat model](docs/threat-model.md)
- [Architecture](docs/architecture.md)
- [Current and future architecture maps](docs/architecture-visual.md)
- [Deployment model](docs/deployment.md)
- [Ratatui TUI Elm loop plan](docs/ratatui-elm-loop-plan.md)
- [Performance baseline](docs/performance.md)
- [Go gateway benchmarks](docs/benchmarks/go-gateway.md)
- [File operation benchmarks](docs/benchmarks/file-operations.md)
- [Text model benchmark](docs/benchmarks/text-model.md)
- [Editor benchmarks](docs/benchmarks/editor.md)
- [Line index stress](docs/benchmarks/line-index-stress.md)
- [TUI edit-loop latency](docs/benchmarks/tui.md)
- [Phase 0.5 plan](docs/phase-0-5-plan.md)
- [Phase 0.6 plan](docs/phase-0-6-plan.md)
- [Upstream dependencies](docs/adr/0006-upstream-dependencies.md)
- [JSON-RPC and token contract](docs/adr/0007-jsonrpc-envelope-and-capability-token.md)
- [Kernel/distribution boundary](docs/adr/0013-kernel-distribution-boundary.md)
- [Selection-first interaction](docs/adr/0008-selection-first-interaction.md)
- [Text-model snapshot gate](docs/adr/0009-text-model-snapshot-gate.md)
- [Cursor and snapshot ownership](docs/adr/0010-cursor-and-snapshot-ownership.md)
- [Workspace file I/O protocol](docs/adr/0011-workspace-chunked-file-io.md)
- [Workspace tree watching](docs/adr/0018-workspace-tree-watch.md)
- [Terminal keyboard enhancement](docs/adr/0019-terminal-keyboard-enhancement.md)
- [Architecture decision records](docs/adr/)
- [Governance](GOVERNANCE.md)
- [Maintainers](MAINTAINERS.md)
- [Release process](RELEASE.md)
- [Support](SUPPORT.md)
- [Roadmap](ROADMAP.md)
- [Security policy](SECURITY.md)
- [Contributing](CONTRIBUTING.md)
- [Code of conduct](CODE_OF_CONDUCT.md)
- [Changelog](CHANGELOG.md)

## Current decisions

- Plugins are untrusted by default and run in a separate plugin-host process,
  never in the editor process.
- The Rust v1 stack uses `ratatui` + `crossterm` directly, with no Tokio or
  Ferriso runtime dependency. Ferriso is reference material.
- Lua plugins use `mlua` with Luau; TS uses `rquickjs` (QuickJS-NG), not V8.
- The Go gateway is a per-session policy gateway, not the plugin sandbox. One
  gateway process owns one session socket; closing the server removes the
  socket.
- `sys.exec` routes through `anvil-exec-runner`. The Phase 0.6 gateway uses a
  one-shot 32-byte bootstrap token: the gateway holds only its SHA-256 verifier,
  the raw token appears only in `session.hello`, and a second handshake is
  rejected. `SO_PEERCRED` UID plus the token are both required. `-allow-exec`
  is currently per gateway instance; per-session permission negotiation remains
  open.
- The public API starts small and is defined from Rust types plus generated JSON
  Schema and TypeScript/Lua stubs. Smithy is deferred until the API justifies
  its code-generation cost.
- The text model is `PieceTable`, exposed as `anvil-core::TextDocument`; Rope and
  Gap Buffer remain benchmark-only. See
  [ADR-0009](docs/adr/0009-text-model-snapshot-gate.md).

- Syntax highlighting is derived state, not stored in `PieceTable`. Parsing runs
  on a background worker after a 50 ms debounce; stale spans remain visible
  until the replacement arrives. Files over 256 KiB are intentionally plain
  text.
- The plugin host is a real process/sandbox boundary. cgroup v2 limits are
  applied before `execve`; a missing delegated cgroup2 root fails closed rather
  than running without the promised limits.
- The native editor/API interaction model is selection-first. The current TUI
  exposes that core through a Vim-like keymap implemented in Rust. Normal,
  operator-pending, and editor/global context bindings can be remapped through
  [keymap.toml](docs/keymap.md), and bundled ergo/Vim presets can be selected as
  a base. Lua/TS keymap plugins are not implemented yet.
- Full untrusted-plugin support targets Linux 5.13+ (Landlock). Linux 5.6-5.12
  may run the editor with trusted plugins only.

## License

Licensed under either of Apache-2.0 or MIT, at your option.
See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT).

<p align="center">
  <img src="resources/icons/com.github.GBLMX.Anvil.png" alt="Anvil desktop icon" width="96" height="96">
</p>

# Anvil

Anvil is a Linux-first terminal editor. The project targets a reusable Rust
kernel, a per-session Go policy gateway, and a capability-scoped plugin host.
The official standard edition is one composition; third-party distributions
can compose the kernel with their own UI and trusted Rust code. Runtime plugins
remain untrusted and sandboxed.

Status: **`0.6.0` remains the frozen workspace/release version while the
`0.7.x` optional-module line is developed in-tree; no public release or tag
during this phase**. See [internal polish plan](docs/internal-polish-plan.md)
and the [0.7.x module plan](docs/0.7.x-plan.md).
Development uses Rust from `rust-toolchain.toml` and Go **1.26.11 or newer**;
CI installs the exact Go version from the root `go.mod`. The ratatui editor now
includes insert
mode (`i`/`a`/`A`/`o`/`O`), multi-cursor selection (`s`/`Alt-s`/`C`),
`:w`/`:wa`/`:wq`/`:w file`/`:q!`, an unsaved-change guard, undo/redo,
`x`/`dd`/`yy`/`p`, `w`/`b`/`e` word motions with
`dw`/`db`/`de`/`yw`/`yb`/`ye`, `G` to the last line, literal `:s`/`:%s`
substitution, `/` search
with `n`/`N`, multi-buffer switching, vertical splits with independent pane cursors,
read-only `:git-diff` views with hunk staging, `:git-blame`, conflict listing,
base/ours/theirs previews, and whole-file `:git-resolve-ours` /
`:git-resolve-theirs` resolution (with `!` to discard a modified buffer),
workspace session restore, dirty-buffer swap
conflict commands, and a fresh undo stack after restore, horizontal scrolling,
and dirty-only redraws, insert-mode Tab indentation, a workspace file tree, and a fuzzy file
finder. Rust, TOML, Go, Bash/Zsh/Brush, Fish, Python, YAML, JSON/JSONC,
HTML (`.html`, `.htm`), Markdown, Makefile, Dockerfile, and LaTeX
(`.tex`, `.ltx`, `.sty`, `.cls`)
use Tree-sitter highlighting on a debounced background thread, capped at
256 KiB so large files keep plain-text latency.
Trusted language servers (rust-analyzer, gopls, pyright/pylsp,
typescript-language-server/vtsls, clangd, jdtls, lua-language-server,
vscode-json-language-server, yaml-language-server, bash-language-server,
marksman, taplo, vscode-html-language-server, vscode-css-language-server, zls,
texlab)
run without the plugin sandbox; diagnostics, completion with resolved
documentation and snippets (`Ctrl+Space`),
definition (`gd`), references (`gr`), document highlights (`gh`),
incoming/outgoing calls (`gi`/`go`), folding ranges (`za`/`zM`/`zR`),
code lens, semantic token colors,
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

Built-in semantic themes are `anvil-dark`, `tokyonight`, `omp-kde`,
`helix-base16`, `one-dark`, `gruvbox-dark`, `nord`, `catppuccin-mocha`, and
`solarized-light`. The boxpigma family adds `boxpigma`, `dracula`, `gruvbox`,
`solarized`, `tokyo-night`, `catppuccin`, `monokai`, `rose-pine`, `kanagawa`,
`github-light`, `gruvbox-light`, `one-light`, `catppuccin-latte`,
`cyberpunk-volt`, `cyberpunk-fury`, and `cyberpunk-hot`. Use `:theme` to list
them.
`:theme <name>` and `:theme-auto [dark] [light]` switch and persist to
`$XDG_CONFIG_HOME/anvil/settings.toml` (override with `ANVIL_SETTINGS`);
`:theme-save` re-saves the runtime choice explicitly.
`ANVIL_THEME` selects the startup theme.
The command palette (`Space p`) lists themes and previews them while arrowing.

The same file owns theme and typed editor/search/LSP settings:

```toml
[theme]
mode = "fixed"
name = "anvil-dark"

[editor]
scroll_margin = 3
max_buffers = 9
auto_save = false
sidebar_percent = 20

[search]
case_sensitive = false

[lsp]
inlay_hints = true

[lsp.commands]
# Optional command override; the first argv element is the executable.
json = ["node", "/path/to/vscode-json-language-server", "--stdio"]
```

`[lsp.commands]` is keyed by language id. A value may be a command string or
an argv array; array arguments are passed unchanged.

A trusted workspace may add `.anvil.toml`. Its `theme`, `editor`, `search`,
`ui`, and `lsp.inlay_hints` values merge over the user file for that
workspace. Project settings are read-only and cannot set `[lsp.commands]`;
that table is rejected because it would let a repository select an executable.
An explicit `ANVIL_SETTINGS` file remains a complete override and disables the
project layer.

`ANVIL_EDITOR_SCROLL_MARGIN`, `ANVIL_EDITOR_MAX_BUFFERS`,
`ANVIL_EDITOR_AUTO_SAVE`, `ANVIL_EDITOR_SIDEBAR_PERCENT`,
`ANVIL_SEARCH_CASE_SENSITIVE`, and `ANVIL_LSP_INLAY_HINTS` override the file
for the current process.
`:settings` opens the minimal overlay: `j`/`k` select, `h`/`l` or `Enter`
adjust, `r` resets the row, `s` saves, and `Esc` cancels. The Appearance
section includes the active theme row.
Auto-save is off by default; when enabled it writes the active named buffer
on buffer switch/close, split focus/close changes, opening another file, focus
loss, and `:q`. Insert/search/rename input modes are eligible; only
confirmation prompts and modal overlays are excluded.
External file edits are read at startup; `:config-reload` applies settings,
keymap, and theme changes without restarting. File watching is not enabled.

At the `:` prompt, `Tab` completes Ex command names, `:theme` /
`:theme-auto` arguments, and `:plugin-enable` IDs; `Up`/`Down` selects a
candidate and `Enter` accepts it. `Esc` closes the completion list first.

The plugin-host path has a sandboxed host with token-FD delivery,
`close_range`, `no_new_privs`, Landlock read/no-exec rules, native-ABI seccomp
with a preconnected editor socket, a plugin-id/token handshake on that socket,
and dangerous-syscall denial, cgroup-before-exec v2 limits with OOM classification,
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
entries and dynamic `editor.register_command` commands are dispatched from the
command palette. Plugins can also call `ui.notify` and `ui.status`. The optional
`LANDLOCK_SCOPE_SIGNAL` layer is pending.
This is not a VM or public plugin API.

## Distribution and modules

`anvil-distro-standard` owns the process entrypoint; `anvil-distro-minimal` is
the independent second consumer. Official modules are trusted source-level Rust
crates compiled into the binary. There is no dynamic Rust ABI, runtime module
loading, or marketplace-installed Rust module. Module contract v1 is frozen;
the `0.7.1`-`0.7.3` module, workspace-index, and compatibility-gate slices and
the `0.7.4` desktop, LaTeX, and Typst slices are complete.

The `full` Cargo feature enables every official module at runtime; building
with `--all-features` selects it. The default `anvil` binary remains the
standard Git-only composition unless `full` is selected.

- `official.git` is part of the default standard composition. `:git-*` dispatch
  and completion are gated off when the Git service is absent.
- `official.workspace-index` is opt-in through the
  `official-workspace-index` feature. When a distribution installs it,
  quick-open prefers its bounded index and cancels in-flight builds when the
  picker closes. Without it, quick-open falls back to gateway enumeration.
- `official.desktop` is opt-in through `official-desktop`. It enables
  `:open-url <http(s)-url>` with a fixed `xdg-open` handoff and no general
  command runner.
- `official.language-latex` is opt-in through `official-language-latex`. It
  supplies `texlab` profiles for LaTeX and BibTeX; `.tex`, `.ltx`, `.sty`, and
  `.cls` get Tree-sitter highlighting through the `latex` syntax feature.
- `official.language-typst` is opt-in through `official-language-typst`. It
  supplies a `tinymist` profile for Typst; `.typ` files get Tree-sitter
  highlighting through the `typst` syntax feature.
- `anvil-distro-minimal` composes none of these modules.

Module compatibility, caps, and deprecation rules are in
[module compatibility](docs/module-compatibility.md).

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

`anvil-ctl` controls the running instance for one workspace. Each workspace
gets its own socket under `$XDG_RUNTIME_DIR/anvil/`; `--workspace` selects it
and defaults to the current directory. `anvil-mcp` uses `ANVIL_WORKSPACE` or
its own cwd.

```text
anvil-ctl status
anvil-ctl --workspace /path/to/project status
anvil-ctl --json open src/main.rs +12:3
anvil-ctl --json edit set $'alpha\nbeta\n'
printf 'alpha\nbeta\n' | anvil-ctl --json edit set --stdin
anvil-ctl --json query 1:200
anvil-ctl --json command ':git-diff'
anvil-ctl --json quit --force
```

`--json` emits one machine-readable object; failures include `ok: false`,
`exit_code`, `kind`, `message`, and `rpc_code`. Exit codes: 0 success, 2
usage, 3 no instance, 4 timeout, 5 editor RPC error. `open`, `save`, `edit`,
`query`, `command`, and `quit` all pass through the control socket into the
TUI actor; they do not edit files directly. `query` reports buffer text,
cursor, selection, revision, undo depth/bytes, tabs, and active buffer, and
accepts a bounded `START[:END]` line range. `command` executes allowlisted Ex
commands through the same editor state machine; admin commands such as `:pkg`
and `:plugin-*` are denied on the remote surface.

The control protocol is version 3 (`protocol_version` and `capabilities` are
returned by `status`). `open` is workspace-relative. `--spawn` starts Anvil
only when a terminal is attached. `--editor` and stdin instances do not claim
the control socket, so they can run alongside a normal interactive instance.
`scripts/ctl-smoke.sh` drives a real TUI through this CLI and asserts files,
buffer state, undo, Git index state, and session output.

`anvil-mcp` is a thin stdio MCP adapter over `anvil-control`. It exposes
`anvil_status`, `anvil_open`, `anvil_query`, `anvil_edit`, `anvil_save`,
`anvil_command`, and `anvil_quit`; it never reads or writes workspace files
directly. `scripts/mcp-smoke.sh` starts the adapter against a real TUI.

Quick open: `Space f` filters workspace files and previews the selected file
through the gateway. Wide terminals show the preview beside the list; narrow
terminals stack it below. `Space b` filters the open buffers; `Enter` switches
to the selected buffer.

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
- [Module compatibility and deprecation policy](docs/module-compatibility.md)
- [Module contract](docs/adr/0028-module-contract.md)
- [Desktop URL opener module](docs/adr/0031-desktop-url-opener-module.md)
- [LaTeX language provider](docs/adr/0033-latex-language-provider.md)
- [Workspace index module extension points](docs/adr/0030-workspace-index-module.md)
- [External integration and MCP plan](docs/external-integration-plan.md)
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
- Official Rust modules are source-level, feature-gated, and registered through
  a typed `ModuleRegistry`; contract v1 is frozen. Modules receive no ambient
  plugin capability, cannot shadow kernel commands, and may not depend on
  `anvil-tui` internals. See
  [module contract](docs/adr/0028-module-contract.md).
- The native editor/API interaction model is selection-first. The current TUI
  exposes that core through a Vim-like keymap implemented in Rust. Normal,
  operator-pending, and editor/global context bindings can be remapped through
  [keymap.toml](docs/keymap.md), and bundled ergo, Vim, and Helix presets can
  be selected as a base. Lua/TS keymap plugins are not implemented yet.
- Full untrusted-plugin support targets Linux 5.13+ (Landlock). Linux 5.6-5.12
  may run the editor with trusted plugins only.

## License

Licensed under either of Apache-2.0 or MIT, at your option.
See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT).

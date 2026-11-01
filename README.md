# Anvil

Anvil is a Linux-first terminal editor. The project targets a reusable Rust
kernel, a per-session Go policy gateway, and a capability-scoped plugin host.
The official standard edition is one composition; third-party distributions
can compose the kernel with their own UI and trusted Rust code. Runtime plugins
remain untrusted and sandboxed.

Status: **0.4.0 released; `0.4.x` feature line active**.
Release model: **0.3.0 → 0.4.0 maintenance closed**.
Development uses Rust from `rust-toolchain.toml` and Go **1.26.10 or newer**;
CI installs the exact Go version from the root `go.mod`. The ratatui editor now
includes insert
mode (`i`/`a`/`A`/`o`/`O`), multi-cursor selection (`s`/`Alt-s`/`C`),
`:w`/`:wa`/`:wq`/`:w file`/`:q!`, an unsaved-change guard, undo/redo,
`x`/`dd`/`yy`/`p`, `w`/`b`/`e` word motions with
`dw`/`db`/`de`/`yw`/`yb`/`ye`, literal `:s`/`:%s` substitution, `/` search
with `n`/`N`, multi-buffer switching, vertical splits with independent pane cursors,
horizontal scrolling, and dirty-only redraws, insert-mode Tab indentation, a workspace file tree, and a fuzzy file
finder. Rust and JSON files use Tree-sitter highlighting on a debounced
background thread, capped at 256 KiB so large files keep plain-text latency.
Trusted language servers (rust-analyzer, gopls, pyright/pylsp,
typescript-language-server/vtsls, clangd, jdtls, lua-language-server,
vscode-json-language-server, yaml-language-server, bash-language-server) run
without the plugin sandbox; diagnostics, completion (`Ctrl+Space`), definition (`gd`),
hover (`K`), rename (`Space r`), code actions/quick fixes (`Space a`),
formatting (`Space F`), document symbols (`Space s`), signature help, and
workspace symbols (`Space S`) use canonicalized LSP UTF-16 positions. Rename, code-action, and formatting edits go through a shared
`WorkspaceEdit` engine; unopened files become background buffers, and `:wa`
saves every modified buffer without silently writing files during an edit.
Supported files start their language server automatically; each
`(tool, language_id)` keeps its own worker while the session is open.
Language servers are not bundled; install them on `PATH`.

- TypeScript support requires a project-local compiler: run
  `npm install -D typescript` (or `pnpm add -D typescript`) in the project.
- C/C++ support works best with a compilation database: `bear -- make`, or
  configure CMake with `-DCMAKE_EXPORT_COMPILE_COMMANDS=ON`. Without it, clangd
  may report unresolved includes.

The plugin-host path has an empty sandboxed host with token-FD delivery,
`close_range`, `no_new_privs`, Landlock read/no-exec rules, native-ABI seccomp
with AF_UNIX-only sockets, cgroup v2 limits with OOM classification, framed
`sys.ping`/`sys.exec` dispatch, and a TUI debug `HostSession`
(`:plugin-start`/`:plugin-exec`/`:plugin-stop`/`:plugin-oom`). This is not yet
a VM or public plugin API.

Phase 0 still consists of the `anvil-api` contracts, `anvil-core` PieceTable,
`anvil-editor` snapshots, `anvil-ipc` framing/JSON-RPC/Unix transport, and the
Go gateway with `openat2`, Landlock/seccomp exec sandboxing, SSRF-constrained
fetch, and a synchronous Rust gateway client. The TUI discovers the gateway
beside the binary or via `ANVIL_GATEWAY`.

## Usage

```text
anvil [OPTIONS] [FILE]

  -w, --workspace <DIR>  Workspace directory; --workspace=DIR is also accepted
  -h, --help             Show help and exit
  -V, --version          Show the version and exit
  --                     Treat the next argument as FILE
```

`FILE` must be inside the selected workspace. Use `--` before a file name that
starts with a dash. Launching without `FILE` shows the start dashboard; the first
key dismisses it.

## Documents

- [Project charter](PROJECT_CHARTER.md)
- [Known issues](docs/known-issues.md)
- [High-frequency command architecture plan](docs/command-architecture-plan.md)
- [0.3.0 → 0.4.0: Polish & Specification](docs/polish-and-specification.md)
- [0.3.0 → 0.4.0 / Phase 0.7 plan](docs/phase-0-7-plan.md)
- [0.4.x feature plan](docs/0.4.x-plan.md)
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
- The empty plugin host is a real process/sandbox boundary, but not yet a plugin
  runtime. cgroup limits require a delegated cgroup2 root; without one the host
  fails closed rather than pretending limits are active.
- The native editor/API interaction model is selection-first. The current TUI
  exposes that core through a Vim-like keymap implemented in Rust; Lua/TS
  keymap plugins and user remapping are not implemented yet.
- Full untrusted-plugin support targets Linux 5.13+ (Landlock). Linux 5.6-5.12
  may run the editor with trusted plugins only.

## License

Licensed under either of Apache-2.0 or MIT, at your option.
See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT).

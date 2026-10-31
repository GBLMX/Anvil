# Anvil

Anvil is a Linux-first terminal editor built around a small, capability-scoped
plugin contract.

Status: **0.2.5; Phase 0.7 in progress**. The ratatui editor now includes insert
mode (`i`/`a`/`A`/`o`/`O`), `:w`/`:wq`/`:w file`/`:q!`, an unsaved-change
guard, undo/redo, `x`/`dd`/`yy`/`p`, `w`/`b`/`e` word motions with
`dw`/`db`/`de`/`yw`/`yb`/`ye`, literal `:s`/`:%s` substitution, `/` search
with `n`/`N`, multi-buffer switching, horizontal scrolling, and dirty-only
redraws, insert-mode Tab indentation, a workspace file tree, and a fuzzy file
finder. Rust and JSON files use Tree-sitter highlighting on a debounced
background thread, capped at 256 KiB so large files keep plain-text latency.
Trusted language servers (rust-analyzer, gopls, pyright/pylsp) run without the
plugin sandbox; diagnostics, completion (`Ctrl+Space`), definition (`gd`), and
hover (`K`) use canonicalized LSP UTF-16 positions.

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

## Documents

- [Project charter](PROJECT_CHARTER.md)
- [0.1.1 / Phase 0.7 plan](docs/phase-0-7-plan.md)
- [Plugin-host isolation ADR](docs/adr/0012-plugin-host-isolation.md)
- [Threat model](docs/threat-model.md)
- [Architecture](docs/architecture.md)
- [Deployment model](docs/deployment.md)
- [Ratatui TUI Elm loop plan](docs/ratatui-elm-loop-plan.md)
- [Performance baseline](docs/performance.md)
- [Go gateway benchmarks](docs/benchmarks/go-gateway.md)
- [Text model benchmark](docs/benchmarks/text-model.md)
- [Editor benchmarks](docs/benchmarks/editor.md)
- [Line index stress](docs/benchmarks/line-index-stress.md)
- [TUI edit-loop latency](docs/benchmarks/tui.md)
- [Phase 0.5 plan](docs/phase-0-5-plan.md)
- [Phase 0.6 plan](docs/phase-0-6-plan.md)
- [Upstream dependencies](docs/adr/0006-upstream-dependencies.md)
- [JSON-RPC and token contract](docs/adr/0007-jsonrpc-envelope-and-capability-token.md)
- [Selection-first interaction](docs/adr/0008-selection-first-interaction.md)
- [Text-model snapshot gate](docs/adr/0009-text-model-snapshot-gate.md)
- [Cursor and snapshot ownership](docs/adr/0010-cursor-and-snapshot-ownership.md)
- [Workspace file I/O protocol](docs/adr/0011-workspace-chunked-file-io.md)
- [Architecture decision records](docs/adr/)
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
- Phase 0 has one native interaction model: selection-first. Motions and
  actions live in the Rust core; keymaps ship as Lua/TS plugins.
- Full untrusted-plugin support targets Linux 5.13+ (Landlock). Linux 5.6-5.12
  may run the editor with trusted plugins only.

## License

Licensed under either of Apache-2.0 or MIT, at your option.
See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT).

# Anvil

Anvil is a Linux-first terminal editor built around a small, capability-scoped
plugin contract.

Status: **pre-alpha; Phase 0.6 in progress**. There is no runnable editor yet.
Phase 0 code currently consists of the `anvil-api` interaction and view
contracts, the `anvil-core` text buffer and selected PieceTable backend, the
`anvil-editor` snapshot layer, the `anvil-ipc` frame codec, JSON-RPC envelope,
Unix socket transport, and `SO_PEERCRED`; Phase 0.6 adds the Go gateway
skeleton, exec runner, Landlock confinement with per-tool execute and a positive
seccomp allowlist, supervised process groups, and `rg`/`fd` argv policy, plus the
`openat2`-confined workspace reader in `go/internal/fsroot`.

## Documents

- [Project charter](PROJECT_CHARTER.md)
- [Threat model](docs/threat-model.md)
- [Architecture](docs/architecture.md)
- [Performance baseline](docs/performance.md)
- [Text model benchmark](docs/benchmarks/text-model.md)
- [Editor benchmarks](docs/benchmarks/editor.md)
- [Line index stress](docs/benchmarks/line-index-stress.md)
- [Phase 0.5 plan](docs/phase-0-5-plan.md)
- [Phase 0.6 plan](docs/phase-0-6-plan.md)
- [Upstream dependencies](docs/adr/0006-upstream-dependencies.md)
- [JSON-RPC and token contract](docs/adr/0007-jsonrpc-envelope-and-capability-token.md)
- [Selection-first interaction](docs/adr/0008-selection-first-interaction.md)
- [Text-model snapshot gate](docs/adr/0009-text-model-snapshot-gate.md)
- [Cursor and snapshot ownership](docs/adr/0010-cursor-and-snapshot-ownership.md)
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
- The Go gateway is a per-session policy gateway, not the plugin sandbox.
- `sys.exec` remains disabled until the Go gateway routes tool calls through
  `anvil-exec-runner`; the runner's Landlock/seccomp path and kernel sandbox
  tests are implemented, but gateway integration is still pending.
- The public API starts small and is defined from Rust types plus generated JSON
  Schema and TypeScript/Lua stubs. Smithy is deferred until the API justifies
  its code-generation cost.
- The text model is `PieceTable`, exposed as `anvil-core::TextDocument`; Rope and
  Gap Buffer remain benchmark-only. See
  [ADR-0009](docs/adr/0009-text-model-snapshot-gate.md).
- Phase 0 has one native interaction model: selection-first. Motions and
  actions live in the Rust core; keymaps ship as Lua/TS plugins.
- Full untrusted-plugin support targets Linux 5.13+ (Landlock). Linux 5.6-5.12
  may run the editor with trusted plugins only.

## License

Licensed under either of Apache-2.0 or MIT, at your option.
See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT).

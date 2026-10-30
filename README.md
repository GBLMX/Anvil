# Anvil

Anvil is a Linux-first terminal editor built around a small, capability-scoped
plugin contract.

Status: **pre-alpha, Phase 0**. There is no runnable editor yet. Phase 0 code
currently consists of the `anvil-ipc` frame codec with Unix socket transport
and `SO_PEERCRED`, and the `openat2`-confined workspace reader in
`go/internal/fsroot`.

## Documents

- [Project charter](PROJECT_CHARTER.md)
- [Threat model](docs/threat-model.md)
- [Architecture](docs/architecture.md)
- [Performance baseline](docs/performance.md)
- [Upstream dependencies](docs/adr/0006-upstream-dependencies.md)
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
- `sys.exec` is disabled until `anvil-exec-runner` applies Landlock/seccomp to
  the tool child. The gateway never execs a tool from its own unsandboxed space.
- The public API starts small and is defined from Rust types plus generated JSON
  Schema and TypeScript/Lua stubs. Smithy is deferred until the API justifies
  its code-generation cost.
- The text model is selected by a Phase 0 benchmark. A rope or piece table is the
  default candidate; Gap Buffer ships only if it meets the random-edit targets.
- Full untrusted-plugin support targets Linux 5.13+ (Landlock). Linux 5.6-5.12
  may run the editor with trusted plugins only.

## License

Licensed under either of Apache-2.0 or MIT, at your option.
See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT).

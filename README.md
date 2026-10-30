# Anvil

Anvil is a Linux-first terminal editor built around a small, capability-scoped
plugin contract.

Status: **pre-alpha, Phase 0**. There is no runnable editor yet. This repository
currently contains the project charter and the architecture decisions that must
be proven before implementation expands.

## Documents

- [Project charter](PROJECT_CHARTER.md)
- [Threat model](docs/threat-model.md)
- [Architecture](docs/architecture.md)
- [Performance baseline](docs/performance.md)
- [Architecture decision records](docs/adr/)

## Current decisions

- Plugins are untrusted by default and run in a separate plugin-host process,
  never in the editor process.
- The Go gateway is a per-session policy gateway, not the plugin sandbox.
- The public API starts small and is defined from Rust types plus generated JSON
  Schema and TypeScript/Lua stubs. Smithy is deferred until the API justifies
  its code-generation cost.
- The text model is selected by a Phase 0 benchmark. A rope or piece table is the
  default candidate; Gap Buffer ships only if it meets the random-edit targets.
- Full untrusted-plugin support targets Linux 5.13+ (Landlock). Linux 5.6-5.12
  may run the editor with trusted plugins only.

## License

Licensed under either of Apache-2.0 or MIT, at your option.
See [LICENSE](LICENSE) and [LICENSE-MIT](LICENSE-MIT).

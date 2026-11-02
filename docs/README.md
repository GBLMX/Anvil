# Documentation Index

Status: index for current architecture, plans, ADRs, and resolved bug records.

## Start here

- [README](../README.md) — project overview and usage
- [Install](../INSTALL.md) — local build and installed-binary smoke test
- [Roadmap](../ROADMAP.md) — directional priorities
- [Architecture](architecture.md) — process model and boundaries
- [Architecture maps](architecture-visual.md) — current/future diagrams and
  delivery DAG
- [Threat model](threat-model.md) — adversaries, mitigations, invariants
- [Performance baseline](performance.md) — gates and re-run triggers

## Active plans

| Plan | Scope | Status |
|---|---|---|
| [0.4.x](0.4.x-plan.md) | plugin lifecycle, I/O, session/protocol | active |
| [0.5.x](0.5.x-plan.md) | modal editing and modern TUI interaction | planned |
| [0.6.x](0.6.x-plan.md) | signed packages, download manager, marketplace | planned |
| [0.7.x](0.7.x-plan.md) | official optional Rust modules (source-level, trusted) | planned |
| [Linux platform](linux-platform-plan.md) | seccomp/cgroup, atomic install, watcher measurements | planned |
| [Command architecture](command-architecture-plan.md) | command registry, keymap trie, effects | partially implemented |
| [Keymap configuration](keymap.md) | TOML overrides and key syntax | current |
| [Deployment](deployment.md) | gateway, exec-runner, cgroup boundaries | current |

## Historical plans

- [Phase 0.5](phase-0-5-plan.md) — core evidence and editing primitives
- [Phase 0.6](phase-0-6-plan.md) — security and host boundaries
- [Phase 0.7](phase-0-7-plan.md) — 0.3.0 → 0.4.0 polish
- [Polish and specification](polish-and-specification.md) — 0.4.0 train
- [Ratatui Elm loop](ratatui-elm-loop-plan.md) — TUI runtime target

Historical plans are records. Do not silently rewrite their decisions; add a
new plan or ADR when the current direction changes.

## Bugs and limitations

- [Known issues](known-issues.md) — resolved BUG ledger plus current
  cross-cutting limitations
- [Security policy](../SECURITY.md) — disclosure and reporting
- [Support](../SUPPORT.md) — support boundaries

## ADRs

Architecture decisions live in [`adr/`](adr/). Start with:

- [0001 plugin isolation](adr/0001-plugin-isolation.md)
- [0003 IDL / ten-method API](adr/0003-idl.md)
- [0007 JSON-RPC and capability token](adr/0007-jsonrpc-envelope-and-capability-token.md)
- [0008 selection-first interaction](adr/0008-selection-first-interaction.md)
- [0013 kernel/distribution boundary](adr/0013-kernel-distribution-boundary.md)
- [0014 plugin WIT seams](adr/0014-plugin-wit-seams.md)
- [0017 activation and lifecycle](adr/0017-plugin-activation-and-lifecycle.md)
- [0018 workspace tree watch](adr/0018-workspace-tree-watch.md)
- [0019 terminal keyboard enhancement](adr/0019-terminal-keyboard-enhancement.md)

## Benchmarks

- [Go gateway](benchmarks/go-gateway.md)
- [File operations](benchmarks/file-operations.md)
- [Text model](benchmarks/text-model.md)
- [Editor](benchmarks/editor.md)
- [Line index stress](benchmarks/line-index-stress.md)
- [TUI edit loop](benchmarks/tui.md)

## Document conventions

- **ADR**: why a decision was made; accepted ADRs are not rewritten silently.
- **Plan**: what a release slice will deliver, its dependencies, and gates.
- **SPEC**: current normative contract; generated artifacts are projections.
- **Known issues**: resolved regressions and current limitations, not a
  duplicate roadmap.
- A plan that depends on an API freeze or signing gate must link that gate
  explicitly.

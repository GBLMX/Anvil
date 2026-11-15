# Documentation Index

Status: canonical map for architecture, plans, ADRs, SPECs, and reference
material. Last reviewed 2026-11-15.

## Start here

- [Project overview](../README.md) — what Anvil is and how to build it
- [Install](../INSTALL.md) — local build and installed-binary smoke test
- [Roadmap](../ROADMAP.md) — directional priorities
- [Architecture](architecture.md) — process model and layer boundaries
- [Architecture maps](architecture-visual.md) — current/future diagrams
- [Threat model](threat-model.md) — adversaries, mitigations, invariants
- [Internal polish](internal-polish-plan.md) — frozen `0.6.0` phase
- [Performance baseline](performance.md) — gates and re-run triggers

## Sources of truth

Use one owner per question; other docs link instead of restating status.

| Question | Canonical source |
|---|---|
| Product direction | [ROADMAP.md](../ROADMAP.md) |
| Internal-polish scope and exit gates | [internal-polish-plan.md](internal-polish-plan.md) |
| Architecture and layer rules | [architecture.md](architecture.md), [ADR-0042](adr/0042-selection-first-session-frontend.md) |
| Architecture decisions | [ADR index](adr/README.md) |
| Plugin API contract | [plugin-api-v1.md](spec/plugin-api-v1.md) |
| Security model | [threat-model.md](threat-model.md), [SECURITY.md](../SECURITY.md) |
| Performance gates | [performance.md](performance.md) |
| Resolved bugs and current limits | [known-issues.md](known-issues.md) |

## Document types

- **ADR**: why a decision was made. IDs and file names are immutable; the ADR
  index owns status and version rules.
- **Plan**: what a release slice will deliver, in what order, and with which
  gates. A plan owns its own status.
- **SPEC**: current normative contract. Generated artifacts are projections.
- **Reference**: stable operational or design information; not a schedule.
- **Known issues**: resolved regressions and current limitations; not a second
  roadmap.
- **Historical**: phase records and superseded plans. Do not silently rewrite
  their decisions; add a new plan or ADR when direction changes.

## Status vocabulary

Plans and proposals use one of:

| Status | Meaning |
|---|---|
| `Proposed` | Not scheduled; needs a decision or consumer |
| `Active` | Current work with an owner |
| `Landed` | Delivered; kept as a design record |
| `Deferred` | Intentionally not scheduled |
| `Superseded` | Replaced; link the replacement |

Do not duplicate detailed per-slice status in this index. The linked plan is
the owner.

## Active work

| Plan | Scope | Status |
|---|---|---|
| [Internal polish](internal-polish-plan.md) | P0/P1 polish under frozen `0.6.0` | Active; exit gates open |
| [0.6.x distribution](0.6.x-plan.md) | packages, signing, download manager, marketplace | Active; marketplace deferred |
| [0.7.x modules](0.7.x-plan.md) | optional source-level Rust modules | Active; 0.7.1–0.7.4 landed |
| [Command architecture](command-architecture-plan.md) | registry, keymap trie, effects | Active; partial |
| [Linux platform](linux-platform-plan.md) | seccomp/cgroup, atomic install, watcher measurements | Active; per-section status |
| [Small features](small-features-plan.md) | Vim-like motions, operators, display polish | Proposed |
| [Plugin tactical interaction](plugin-mouse-canvas-plan.md) | scoped mouse, overlays, selection, alerts | Proposed; API 1.1 |
| [AI telemetry HUD](ai-telemetry-plan.md) | local EPM/APM/Burst telemetry | Future P3 |

## Landed plans and design records

- [0.4.x](0.4.x-plan.md) — plugin lifecycle, I/O, session/protocol
- [0.5.x](0.5.x-plan.md) — modal editing and TUI interaction
- [0.6.0.x provider protocols](0.6.0.x-provider-protocol-plan.md) — picker,
  completion, formatter, test, and DAP providers
- [0.6.1 package store](0.6.1-package-store-plan.md) — content-addressed store
  and installed state
- [IDE capabilities](ide-capability-plan.md) — P0/P1 LSP, search, session, Git,
  and adapters
- [Multi-root routing](multi-root-routing-plan.md) — routed tree, quick open,
  search, and mutations; cross-root move/copy stays rejected
- [Task lifecycle](task-lifecycle-plan.md) — bounded background work,
  cancellation, and shutdown
- [Theme and settings](theme-settings-plan.md) — built-in themes and typed
  settings
- [External integration](external-integration-plan.md) — `$EDITOR`, stdin,
  control socket, and MCP adapter

## Historical plans

Historical plans are records, not current schedules:

- [Phase 0.5](phase-0-5-plan.md) — core evidence and editing primitives
- [Phase 0.6](phase-0-6-plan.md) — security and host boundaries
- [Phase 0.7](phase-0-7-plan.md) — 0.3.0 → 0.4.0 polish
- [Polish and specification](polish-and-specification.md) — 0.4.0 train
- [Ratatui Elm loop](ratatui-elm-loop-plan.md) — TUI runtime exploration; the
  current runtime is typed-effect based, not an Elm `Msg`/`Cmd` loop

## Reference

- [Deployment](deployment.md) — gateway, exec-runner, and cgroup boundaries
- [Keymap configuration](keymap.md) — TOML overrides and key syntax
- [Keymap routing analysis](keymap-router-analysis.md) — preset and routing notes
- [Module compatibility](module-compatibility.md) — contract v1 matrix and
  deprecation policy
- [TUI design](tui-design.md) — visual tokens, layout, and interaction rhythm
- [Threat model](threat-model.md) — trust boundaries and failure modes
- [Known issues](known-issues.md) — resolved regressions and current limits
- [PGO](pgo-bolt.md), [PGO workload](../scripts/pgo-workload.py) — profile-guided build
  workflow

## SPECs

- [Plugin API v1](spec/plugin-api-v1.md) — normative wire contract
- [Plugin API pointer](spec/plugin-api.md) — former draft path; points to v1

## Benchmarks

- [Go gateway](benchmarks/go-gateway.md)
- [File operations](benchmarks/file-operations.md)
- [Text model](benchmarks/text-model.md)
- [Editor](benchmarks/editor.md)
- [Line index stress](benchmarks/line-index-stress.md)
- [TUI edit loop](benchmarks/tui.md)

## ADRs

Architecture decisions live in [`adr/`](adr/). The canonical status table,
numbering rule, and `x.y.z` version rule are in
[`adr/README.md`](adr/README.md).

Architecture reading order, not a second status table:

1. [ADR-0008](adr/0008-selection-first-interaction.md) — selection-first core
2. [ADR-0013](adr/0013-kernel-distribution-boundary.md) — kernel/distribution
3. [ADR-0037](adr/0037-kernel-api-baseline.md) — kernel API baseline
4. [ADR-0042](adr/0042-selection-first-session-frontend.md) — target boundaries

## Document conventions

### Naming

- ADR: `NNNN-topic.md`; `NNNN` is immutable.
- Plan: `<version-or-phase>-topic-plan.md`.
- SPEC: `spec/<contract>-v<major>.md`.
- Reference: `<topic>.md`; no `plan` suffix.
- Do not rename a document only to reorganize it; update the index instead.

### Status

- The document header owns `Status`.
- This index owns only category and coarse status.
- Roadmap owns direction, not slice status.
- ADR index owns ADR status.

### Version axes

- Product version: workspace/Cargo version (`0.6.0`).
- Plan slice: internal labels such as `0.6.0.1`; not semver or release tags.
- ADR version: `x.y.z`; see [`adr/README.md`](adr/README.md).

### Redundancy rules

- Do not copy decision text from an ADR into a plan; link it.
- Do not maintain a second roadmap inside a plan.
- Do not copy the full ADR table here; link the ADR index.
- A plan may summarize dependencies, but the dependency ADR/SPEC remains
  canonical.
- Prefer deleting a stale duplicate over keeping two “current” statements.

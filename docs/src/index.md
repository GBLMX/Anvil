# Documentation Index

Status: canonical map for architecture, plans, ADRs, SPECs, and reference
material. Last reviewed 2026-11-26.

## Start here

- [Project overview](https://github.com/GBLMX/Anvil/blob/main/README.md) — what Anvil is and how to build it
- [Install](INSTALL.md) — local build and installed-binary smoke test
- [Multi-host install lessons](multi-host-install.md) — Arch, Debian, Ubuntu, and Fedora compatibility findings
- [Roadmap](ROADMAP.md) — directional priorities
- [Analysis backlog](plans/proposed/analysis-backlog.md) — research queue, not a release commitment
- [Architecture](architecture.md) — process model and layer boundaries
- [Threat model](threat-model.md) — adversaries, mitigations, invariants
- [Release process](RELEASE.md) — candidate gates and publication checklist
- [Local CI with act and Docker](local-ci.md) — reproducible local workflow runs
- [Performance baseline](performance.md) — gates and re-run triggers

## Sources of truth

Use one owner per question; other docs link instead of restating status.

| Question | Canonical source |
|---|---|
| Product direction | [ROADMAP.md](ROADMAP.md) |
| Release gates and candidate checklist | [RELEASE.md](RELEASE.md) |
| Release cadence, freeze policy, and synchronization | [ADR-0080](adr/0080-release-cadence-and-synchronization.md), [release cycle plan](plans/active/release-cycle-plan.md) |
| Architecture and layer rules | [architecture.md](architecture.md), [ADR-0042](adr/0042-selection-first-session-frontend.md) |
| TUI structural budget | [tui-maintainability.md](tui-maintainability.md) |
| TUI view boundary and extraction gates | [history/analysis/tui-view-boundary.md](history/analysis/tui-view-boundary.md) |
| Architecture decisions | [ADR index](adr/index.md) |
| Plugin API contract | [plugin-api-v1.md](spec/plugin-api-v1.md) |
| API surface and versioning | [api-surface.md](api-surface.md) |
| Security model | [threat-model.md](threat-model.md), [SECURITY.md](https://github.com/GBLMX/Anvil/blob/main/SECURITY.md) |
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
| `Future` | Direction accepted, but explicitly outside the current phase |
| `Historical` | Closed phase or superseded design record |
| `Superseded` | Replaced; link the replacement |

Legacy historical documents may still use `closed`, `complete`, `done`,
`planned`, or `current`. Treat them as historical aliases:

- `closed` / `complete` / `done` → `Landed` for delivered work, or
  `Historical` for a phase record.
- `planned` → `Proposed` unless the document is actively being worked on.
- `current` → not a plan status; reference docs use it to mean “matches the
  implementation”.

Do not introduce these legacy words in new plans; keep the original wording in
historical records rather than rewriting them.

Do not duplicate detailed per-slice status in this index. The linked plan is
the owner.

## Active work

| Plan | Scope | Status |
|---|---|---|
| [Architecture modernization](plans/active/architecture-modernization-plan.md) | TUI ownership, state boundaries, headless proof, optional extraction | Active; Phase 1 theme-browser slice landed |
| [Command architecture](plans/active/command-architecture-plan.md) | registry, keymap trie, effects | Active; partial |
| [IDE platform](plans/active/ide-platform-plan.md) | project model, workspace index, integration descriptors, debug depth, remote | Active; design accepted, implementation not started |
| [Linux platform](plans/active/linux-platform-plan.md) | seccomp/cgroup, atomic install, watcher measurements | Active; per-section status |
| [Task lifecycle](plans/active/task-lifecycle-plan.md) | bounded background work, cancellation, shutdown | Active; slice 1 landed |
| [Release cycle](plans/active/release-cycle-plan.md) | cadence, freeze policy, sync rails, release checklist | Active; slices 1-5 landed |
| [External integration](external-integration.md) | `$EDITOR`, stdin, control socket, MCP adapter | Current contract; no remaining tasks |

## Proposed work

| Plan | Scope | Status |
|---|---|---|
| [Analysis backlog](plans/proposed/analysis-backlog.md) | research queue | Proposed |
| [AI telemetry HUD](plans/proposed/ai-telemetry-plan.md) | local EPM/APM/Burst telemetry | Future P3 |
| [Plugin tactical interaction](plans/proposed/plugin-mouse-canvas-plan.md) | scoped mouse, overlays, selection, alerts | Proposed; API 1.1 |
| [Small features](plans/proposed/small-features-plan.md) | Vim-like motions, operators, display polish | Proposed |

## Landed plans and design records

- [0.5.x](history/plans/0.5.x-plan.md) — modal editing and TUI interaction
- [0.6.0.x provider protocols](history/plans/0.6.0.x-provider-protocol-plan.md) — picker,
  completion, formatter, test, and DAP providers
- [0.6.1 package store](package-store.md) — content-addressed store
  and installed state
- [0.6.x distribution](history/plans/0.6.x-plan.md) — packages, signing,
  download manager, marketplace
- [0.7.x modules](history/plans/0.7.x-plan.md) — optional source-level Rust modules
- [IDE capabilities](history/plans/ide-capability-plan.md) — P0/P1 LSP, search,
  session, Git, and adapters
- [Internal polish](history/plans/internal-polish-plan.md) — 0.7.0 closure record
- [Multi-root routing](history/plans/multi-root-routing-plan.md) — routed tree,
  quick open, search, and mutations
- [Theme and settings](history/plans/theme-settings-plan.md) — built-in themes
  and typed settings

## Historical plans

Historical plans are records, not current schedules:

- [0.4.x](history/plans/0.4.x-plan.md) — plugin lifecycle, I/O, session/protocol
- [Phase 0.5](history/plans/phase-0-5-plan.md) — core evidence and editing primitives
- [Phase 0.6](history/plans/phase-0-6-plan.md) — security and host boundaries
- [Phase 0.7](history/plans/phase-0-7-plan.md) — 0.3.0 → 0.4.0 polish
- [Polish and specification](history/plans/polish-and-specification.md) — 0.4.0 train
- [Ratatui Elm loop](history/plans/ratatui-elm-loop-plan.md) — TUI runtime exploration; the
  current runtime is typed-effect based, not an Elm `Msg`/`Cmd` loop

## Historical release records

- [Release dry run — 2026-11-20](history/releases/release-dry-run-2026-11-20.md)
- [Release dry run — 2026-11-22](history/releases/release-dry-run-2026-11-22.md)
- [YubiKey release rehearsal — 2026-11-22](history/releases/release-yubikey-rehearsal-2026-11-22.md)

## Reference

- [Offline signing and verified publish](RELEASE.md#offline-signing-and-verified-publish) — release runbook
- [Deployment](deployment.md) — gateway, exec-runner, and cgroup boundaries
- [Keymap configuration and routing](keymap.md) — TOML overrides, key syntax, and contextual dispatch
- [API surface](api-surface.md) — frozen contracts, version lines, and compatibility gates
- [Modules, compatibility, and inventory](modules.md) — contract v1 matrix, deprecation policy, and `anvil modules`
- [TUI design](tui-design.md) — visual tokens, layout, and interaction rhythm
- [Threat model](threat-model.md) — trust boundaries and failure modes
- [Known issues](known-issues.md) — resolved regressions and current limits
- [Getting started](onboarding.md) — first-run tasks and keymap card
- [Recovery and save semantics](recovery.md) — durability, undo, swaps, and
  external conflicts
- [Remote/SSH workflow](multi-host-install.md#remote-ssh-workflow) — remote install, color forwarding,
  tmux, control socket, and diagnostics
- [PGO and BOLT](performance.md#pgo-and-bolt-workflow), [PGO workload](https://github.com/GBLMX/Anvil/blob/main/scripts/pgo-workload.py) — profile-guided build
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

Architecture decisions live in [`adr/`](adr/index.md). The canonical status table,
numbering rule, and `x.y.z` version rule are in
[`adr/index.md`](adr/index.md).

Architecture reading order, not a second status table:

1. [ADR-0008](adr/0008-selection-first-interaction.md) — selection-first core
2. [ADR-0013](adr/0013-kernel-distribution-boundary.md) — kernel/distribution
3. [ADR-0037](adr/0037-kernel-api-baseline.md) — kernel API baseline
4. [ADR-0042](adr/0042-selection-first-session-frontend.md) — target boundaries

## Document conventions

### Naming

- ADR: `NNNN-topic.md`; `NNNN` is immutable.
- Plan: `<version-or-phase>-topic-plan.md` for release/phase plans, or
  `<topic>-plan.md` for continuous cross-cutting plans.
- SPEC: `spec/<contract>-v<major>.md`.
- Reference: `<topic>.md`; no `plan` suffix.
- Do not rename a document only to reorganize it; update the index instead.

### Templates

- [Plan template](templates/plan.md)
- [ADR template](templates/adr.md)

### Verification

- `python3 scripts/docs-check.py` checks relative links and local anchors,
  ADR identity/date/version, plan status, locations, review freshness, and
  index links, template sections, and the root Markdown budget.

### Status

- The document header owns `Status`.
- This index owns only category and coarse status.
- Roadmap owns direction, not slice status.
- ADR index owns ADR status.

### Version axes

- Product version: workspace/Cargo version (`0.8.0`).
- Plan slice: internal labels such as `0.7.0.1`; not semver or release tags.
- ADR version: `x.y.z`; see [`adr/index.md`](adr/index.md).

### Redundancy rules

- Do not copy decision text from an ADR into a plan; link it.
- Do not maintain a second roadmap inside a plan.
- Do not copy the full ADR table here; link the ADR index.
- A plan may summarize dependencies, but the dependency ADR/SPEC remains
  canonical.
- Prefer deleting a stale duplicate over keeping two “current” statements.

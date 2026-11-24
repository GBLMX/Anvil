# Architecture Modernization Plan

Status: Active; Phase 0 complete; Phase 1 in progress.
Owner: maintainers.
Scope: ownership, state boundaries, and optional crate extraction for the Anvil
kernel/frontends. This is not a rewrite and not a feature program.

## Declaration

Anvil enters an architecture modernization and modularization program. Work is
measured, incremental, and behavior-preserving. The target is narrower
ownership and a headless-verifiable view/session boundary, not a larger crate
count or a lower line count.

The program follows [ADR-0074](adr/0074-anvil-tui-state-decomposition-and-crate-boundary.md)
and the analysis in [TUI view boundary](tui-view-boundary.md).

## Invariants

- `main` remains releasable after every slice.
- User-visible behavior is unchanged unless a slice explicitly carries a
  separate behavior change with its own tests and decision record.
- No long-lived refactor branch.
- Tests, clippy, rustfmt, architecture checks, API surface checks, and docs
  checks stay green.
- A new public API, crate, or contract requires an ADR or an amendment.
- The terminal boundary is explicit: view state must not depend on
  `ratatui`/`crossterm`, gateway clients, PTYs, filesystem workers, or LSP/DAP
  handles.
- Metrics are evidence, not a schedule: `&mut Model` methods, `impl Model`
  blocks, dependency edges, and headless transactions matter more than LOC.

## Baseline

Recorded 2026-11-24 by `scripts/tui-metrics.sh`:

```text
files=112
loc=61,242
deps=32
impl_model=71
impl_model_files=66
model_mut_methods=547
model_new=557
page_model_setups=2
files_over_800=19
```

The settings inventory is maintained in
[settings-sources.md](settings-sources.md#environment-inventory). In
particular, `ANVIL_SETTINGS` is a complete-file override, not a per-field
settings migration target.

## Phases

### Phase 0: baseline and inventory

- Keep `docs/tui-maintainability.baseline` and its Markdown table current.
- Record `cargo build -p anvil-tui --timings` and one-file incremental checks.
- Keep the settings environment inventory current.
- Reconcile settings/LSP work with ADR-0075 and ADR-0076 before extracting that
  code.

### Phase 0 measurements (2026-11-24)

Warm dev-profile measurements on the current workspace:

| Command | Wall | Cargo report |
|---|---:|---|
| `cargo build -p anvil-tui --timings` | 8.12s | 8.08s |
| `cargo check -p anvil-tui` (no-op) | 0.10s | 0.07s |
| `touch crates/anvil-tui/src/folding.rs && cargo check -p anvil-tui` | 3.21s | 3.17s |

The build timing report was written under `target/cargo-timings/`; it is a
local artifact, not a committed input. These are warm measurements: dependency
compilation was already cached. The one-file edit cost is measurable, but it
does not yet justify a crate extraction. Phase 1 starts with page state and
keeps the same measurement command for each structural slice.

### Phase 1 progress

The theme-browser page now returns `ThemeBrowserEffect` values; the page owns
selection, original-theme state, and the transition outcome. One host function
applies the effects to `Model`.

```text
impl Model:                    71 -> 70
impl Model files:              66 -> 65
&mut Model methods:            547 -> 543
free functions taking Model:   56 (new baseline)
source lines:                  61,242 -> 61,326
```

The line increase is the effect type and explicit host application, not a
behavior change. `cargo test -p anvil-tui --lib` passes.

### Phase 1: page state

Extract page-owned state, update logic, and tests from `impl Model` for the
smallest clear pages first:

- `command_palette`
- `theme_browser`
- `start_page`
- `settings_ui`
- `debug_panel`

Each page state must be constructible without `Model`, terminal setup, or a
gateway.

### Phase 2: domain state

Move remaining behavior into owned groups:

- `SettingsState`
- `LanguageUiState`
- `WorkspaceUiState`
- `PluginUiState`
- terminal/runtime adapters

Handles and effects remain in runtime/domain owners. Page state records intent,
selection, and view state only.

### Phase 3: pure presentation state

Extract or isolate pure state that does not know the terminal exists:

- layout/split tree
- folding ranges
- Git diff view model
- other presentation-only state

`anvil-tui-theme` remains the owner of icon/theme mapping.

### Phase 4: headless proof

Add an integration transaction that runs without a terminal:

```text
open Document
-> apply one WorkspaceEdit
-> save
-> query revision
```

Use `anvil-session`, `anvil-editor`, and `anvil-ipc`. The existing
`examples/anvil-distro-minimal` only proves a second composition descriptor;
it is not the headless transaction fixture.

### Phase 5: optional crate extraction

Extract a crate only when at least one gate is true:

- a second consumer exists;
- the boundary has no `Model`, terminal, gateway, worker, or handle
  dependency;
- `cargo build --timings` or one-file incremental measurements identify a
  real serialization point;
- the dependency graph stays acyclic without a `session -> view` edge.

Prefer a narrow `anvil-config` before a general `anvil-view`. Do not rename
`anvil-tui` to `anvil-term` until the view boundary is stable.

## Non-goals

- Rewriting the editor.
- Changing plugin API, control protocol, settings schema, or data formats as
  part of a structural slice.
- Treating file count or LOC as completion criteria.
- Moving gateway/worker/plugin handles into view state.
- Adding a crate per module or using features as a substitute for ownership.

## Related records

- [ADR-0024](adr/0024-tui-state-and-domain-boundaries.md)
- [ADR-0074](adr/0074-anvil-tui-state-decomposition-and-crate-boundary.md)
- [TUI maintainability budget](tui-maintainability.md)
- [TUI view boundary](tui-view-boundary.md)
- [Settings sources](settings-sources.md)
- [ADR-0075](adr/0075-settings-environment-compatibility-surface.md)
- [ADR-0076](adr/0076-built-in-lsp-profile-scope.md)

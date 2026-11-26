# ADR-0074: Anvil-TUI State Decomposition and Crate Boundary

- Status: Proposed; dependency-direction gate implemented; state/page extraction pending
- Date: 2026-11-24
- Related: [ADR-0013](0013-kernel-distribution-boundary.md),
  [ADR-0024](0024-tui-state-and-domain-boundaries.md),
  [ADR-0027](0027-distribution-composition-crate.md),
  [ADR-0028](0028-module-contract.md),
  [ADR-0041](0041-tui-primitive-extraction.md),
  [ADR-0057](0057-tui-surface-model-and-focus-contract.md),
  [ADR-0068](0068-tui-visual-language-and-markers.md)
- Analysis: [TUI view boundary and extraction decision](../history/analysis/tui-view-boundary.md).

## Context

`anvil-tui` is the composition root for the editor frontend. It is working, but
its ownership model has become too broad:

| Metric | Current |
|---|---:|
| Rust files under `src/` | 112 |
| Physical lines under `src/` | 61,236 |
| Tokei Rust code for the crate, including tests/benches | 70,218 |
| Files over 800 lines | 19 |
| `impl Model` blocks | 71 |
| Files containing `impl Model` | 66 |
| Mutable `Model` methods | 547 |
| Direct dependencies | 31 |

These counts include test modules. They are not a production-only LOC claim.
The important signal is not raw size alone: behavior is still spread across
many `impl Model` blocks even though `Model` already groups fields into named
state domains.

`anvil-process` is not an untested crate: it has four unit tests and `Drop for
ProcessTask`. Its remaining gap is integration depth, not the existence of
cleanup. `TUI_LOC` and file counts are informational.
`scripts/tui-metrics.sh` gates exactly `deps`, `impl_model`,
`model_mut_methods`, `model_new`, `page_model_setups`, and
`files_over_800` through `check_max`. `impl_model_files` is computed and
printed for comparison only; it has no `check_max` line. `model_new` is a
source-text count, not a runtime constructor count; `page_model_setups`
covers page source files, not every page test. Baseline increases in feature
commits are visible and need a separate governance rule.

The largest source files are, with inline tests included where present:

| File | Lines |
|---|---:|
| `settings.rs` | 3,464 |
| `state.rs` | 1,716 |
| `layout.rs` | 1,379 |
| `lsp_runtime.rs` | 1,243 |
| `render.rs` | 1,242 |
| `file_tree_ui.rs` | 1,166 |
| `keys/mod.rs` | 1,036 |
| `keymap.rs` | 983 |
| `render/overlays.rs` | 966 |

ADR-0024 chose the right direction: keep the process and crate model, compose
`Model` from owned state groups, and defer crate extraction. Its 2026-11-13
implementation note deferred further page/state extraction because there was no
measured ownership failure. The current file and method distribution is that
measured failure: a page change still tends to require `&mut Model`, and a page
cannot be tested from a small state object.

The Rust Performance Book recommends `cargo build --timings` before splitting
large crates because only a crate that serializes compilation justifies its
own boundary. Cargo workspaces make extraction mechanically cheap but public
API and cyclic-dependency costs remain. Matklad's large-workspace guidance also
favours a flat `crates/` layout and real boundaries rather than a deep tree of
one-module crates.

External references:

- [Cargo Book: Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
- [Cargo Book: Features](https://doc.rust-lang.org/cargo/reference/features.html)
- [Large Rust Workspaces](https://matklad.github.io/2021/08/22/large-rust-workspaces.html)
- [Rust Performance Book: Compile Times](https://nnethercote.github.io/perf-book/compile-times.html)
- [Ratatui: Component Architecture](https://ratatui.rs/concepts/application-patterns/component-architecture/)
- [Ratatui: Flux Architecture](https://ratatui.rs/concepts/application-patterns/flux-architecture/)

## Layer audit

The crate graph is acyclic; that satisfies the Acyclic Dependencies Principle,
but not the Stable Dependencies Principle. A layer edge is acceptable only when
it points toward a more stable contract. The following edges need an explicit
decision:

| Edge | Current shape | Decision |
|---|---|---|
| `anvil-workspace -> anvil-mod-git` | The implementation removed the `official-git` default feature and the `anvil-mod-git` dependency; `FileWorker::start` no longer constructs a Git service. | Keep it removed; the TUI or distribution injects the official service. |
| `anvil-runtime -> anvil-mod-workspace-index` | `Runtime` is documented as a UI-neutral composition root and stores the concrete index. | Composition-root exception. If third-party replacement is required, inject `Arc<dyn WorkspaceIndexService>` or construct the index in the distribution. |
| `anvil-session -> anvil-control` | Completion now receives colon command names through `CommandCompletionData`; session no longer imports `anvil-control`. | Keep the injection; the control manifest remains the source for that list. Do not reintroduce the server dependency. |
| `anvil-module -> anvil-session` | Modules import `CommandId`/`CommandSpec` from session. | Either document command metadata as part of the module contract, or move it to `anvil-api`/a data-only crate. Do not duplicate command IDs. |

`anvil-control::COMMANDS` (colon commands) and
`anvil-session::command::COMMANDS` (editor key commands) are different
manifests. They are not a single source of truth.

No current edge transfers authorization across a process boundary. The issue is
compile-time coupling, replaceability, and accidental recompilation, not a
sandbox bypass.

See [Package Coupling Principles](https://learn.microsoft.com/en-us/archive/technet-wiki/18068.package-coupling-principles).

## Decision

### 1. Do not split `anvil-tui` into many crates now

Keep `anvil-tui` as the application crate. Do not create a crate per module.
`render`, `layout`, `commands`, `mouse`, and the command dispatcher remain in
`anvil-tui` until they have a stable non-`Model` interface.

The first refactor is ownership inside the crate. Crate extraction follows only
after a state boundary is stable and one of these is true:

- a second consumer exists;
- the boundary has no dependency on `Model` or terminal application state;
- measured build or test time improves without duplicating state.

### 2. `Model` is a composition root, not the behaviour owner

`Model` may hold state groups and coordinate updates. It must not accumulate
new feature methods. New behaviour belongs to:

- a domain state group (`SettingsState`, `LanguageUiState`,
  `WorkspaceUiState`, `PluginUiState`, `RuntimeUiState`);
- a page state (`StartPageState`, `SettingsPageState`, `ThemePageState`,
  `CommandPaletteState`, `ProblemsPageState`, `DebugPageState`);
- a renderer that reads page/domain state.

Page state must not own `GatewayClient`, filesystem workers, LSP/DAP handles, or
other background handles. It records intent, selection, and view state. Effects
and handles remain in the owning domain/runtime group. This preserves the
ADR-0024 rule that page state stays outside gateways, PTYs, and filesystem
workers.

`ThemeBrowser` and the command-palette `ThemePicker` remain separate surfaces.
Only the original/preview/cancel/commit ownership is shared through a
`ThemePreviewGuard`; list selection, layout, hit testing, and rendering remain
local. This is an implementation slice under this ADR, not a separate ADR.

### 3. Extraction order

1. **Measurement baseline.** Run `cargo build -p anvil-tui --timings`, record
   `scripts/tui-metrics.sh --check`, and measure a one-file incremental
   `cargo check`. Confirm whether the crate or its dependencies dominate.
2. **Page state.** Extract page-owned state, key handling, update, and render
   from `impl Model` for the smallest clear pages first:
   `command_palette`, `theme_browser`, `start_page`, `settings_ui`, and
   `debug_panel`.
3. **Domain state.** Move remaining behaviour into owned groups, in this order:
   `SettingsState` (including splitting `settings.rs` into modules),
   `LanguageUiState` (LSP/DAP/debug), `WorkspaceUiState` (file tree, git,
   previews), `PluginUiState`, then terminal/runtime adapters.
4. **Crate extraction.** Consider only `anvil-config` (settings parsing without
   ratatui/crossterm), `anvil-tui-language`, `anvil-tui-workspace`, or
   `anvil-tui-plugin` after step 3. Keep the flat `crates/*` layout and inherit
   versions/features from `[workspace.dependencies]`.
5. **Feature gating.** Only after `--timings` proves a heavy optional subsystem
   dominates. Do not use `#[cfg(feature)]` as a substitute for state ownership.

### 4. Dependency direction before extraction

The crate graph is acyclic, but acyclicity does not make every edge stable.
No service crate gets a default edge to a module implementation. A composition
root may name concrete modules; a service crate may only depend on contracts.

- Remove `anvil-workspace`'s `official-git` feature and `anvil-mod-git`
  dependency; `FileWorker::start` no longer constructs a Git service. Inject
  `SystemGitService` from the TUI or distribution, and do not keep a shim edge.
- Treat `anvil-runtime`'s concrete workspace index as a documented
  composition-root exception. If a third-party index must be replaceable,
  accept `Arc<dyn WorkspaceIndexService>` or construct the index in the
  distribution.
- Pass the colon command names into `CommandCompletionData`; `anvil-session`
  completion must not import `anvil-control`. The control manifest remains the
  source for that command list; do not duplicate it in session.
- Move `CommandId`/`CommandSpec` out of `anvil-session` only if module compile
  coupling is measured as a problem. Until then, document it as part of the
  extension contract; do not duplicate command IDs in modules.

Cargo features are additive and do not repair dependency direction. An optional
dependency that is enabled still forms the same compile edge.

### 5. Guardrails

- A leaf module may not add a new `impl Model`; expose a state method instead.
- Every page state must be constructible in a unit test without `Model`,
  terminal setup, or a gateway.
- No user-visible behaviour change is bundled into a state split.
- `scripts/tui-metrics.sh` gains downward-only ratchets for `deps`, `impl_model`,
  `impl_model_files`, `model_mut_methods`, `model_new`, `page_model_setups`,
  and `files_over_800`. A baseline increase is a debt decision, not a feature
  side effect; `TUI_LOC` and file counts remain informational.
- Metrics that count source text state their semantics: `model_new` is not a
  runtime constructor count; page tests are counted separately from page
  source files.
- `scripts/architecture-check.sh` rejects new cycles and the forbidden default
  edge `anvil-workspace -> anvil-mod-git`. The
  `anvil-session -> anvil-control` edge is removed when the colon command
  manifest moves.
- `anvil-process` keeps `Drop` as a last-resort cleanup and adds integration
  coverage for child/process-group exit and spawn failure.
- Split PRs must keep `cargo test`, clippy, rustfmt, architecture, and docs
  checks green.

## Consequences

Positive:

- Pages and domains become locally testable.
- New features depend on narrow state instead of the whole `Model`.
- Version-control and review diffs become smaller by ownership area.
- Future crate extraction becomes mechanical rather than speculative.
- Incremental compile may improve after extraction, but only if timings confirm
  `anvil-tui` is a serialization point.

Negative:

- The first phase moves code without reducing total LOC.
- Temporary delegation shims will exist while call sites migrate.
- Too-granular state can fragment ownership; groups must remain cohesive.
- Crate extraction can add public-API overhead and slow clean builds if done
  before a boundary is stable.
- Removing `anvil-workspace`'s `official-git` dependency is an API change for
  direct `FileWorker::start` callers, which must now inject Git explicitly.
- Completion data gains a command-name slice; this avoids a new protocol crate
  without making session depend on the control server.

## Alternatives rejected

- **Split `anvil-tui` into one crate per module now.** Rejected: most modules
  depend on `Model`, creating cycles or a large public API; no measured compile
  win yet.
- **Move UI state into `anvil-session`.** Rejected: `anvil-session` is
  frontend-neutral and must not acquire ratatui/crossterm application state.
- **Adopt a global `Component` trait for every page.** Rejected: the existing
  TEA-style update loop is adequate. Ratatui's component ownership idea is
  useful, but it does not justify a second architecture for the whole app.
- **Only split files and keep `impl Model`.** Rejected: file size falls while
  ownership coupling remains.
- **Do nothing.** Rejected by the measured method/file distribution; the
  ADR-0024 deferral is no longer justified.

- **Hide a layer edge behind an optional feature.** Rejected: Cargo features
  are additive; they do not change the direction of the default graph.
- **Treat `anvil-runtime` as a service-layer violation.** Rejected: it is a
  documented composition root; the actionable decision is injection only when
  replacement is required.
- **Move every command manifest into `anvil-control`.** Rejected: the colon
  command list and the editor key command list are different contracts.

## Acceptance

- `Model` is a composition root; new page/domain behaviour lives on owned state.
- Each extracted page has a unit test that constructs only its state.
- `impl_model` and `model_mut_methods` trend downward; `deps`, `model_new`,
  `impl_model_files`, `page_model_setups`, and `files_over_800` do not rise;
  baseline changes only ratchet downward.
- `anvil-process` tests cover `Drop` fallback, process-group kill, and spawn
  failure.
- `cargo metadata` or `scripts/architecture-check.sh` shows no cycle or
  forbidden edge; `anvil-workspace` has no `anvil-mod-git` dependency.
- Colon command completion receives command names through
  `CommandCompletionData`; session does not depend on the control server.
- Theme browser and command-palette theme picker share preview/cancel
  ownership but keep independent selection and rendering; both paths have
  tests.
- `cargo build -p anvil-tui --timings` is recorded before and after crate
  extraction.
- Full workspace tests, clippy, formatting, architecture, and docs checks pass
  with no intentional user-visible behaviour change.

## Implementation note (2026-11-24)

`scripts/architecture-check.sh` now enforces predicate rules R1-R6 over normal
dependencies only. Dev/build dependencies are excluded; declared optional
dependencies count as architecture edges. `anvil-runtime` is the registered
composition-root exception for `anvil-mod-workspace-index`.

`anvil-workspace` no longer depends on `anvil-mod-git`, and
`anvil-session` no longer depends on `anvil-control`. The workspace Git parser
tests moved to `anvil-mod-git`. State/page extraction and the theme preview
guard remain pending.

## Supersession note

This ADR resumes the page/state extraction deferred by ADR-0024 on 2026-11-13.
It does not supersede ADR-0024's process/crate model, state-group rules, or
primitive-extraction decision.

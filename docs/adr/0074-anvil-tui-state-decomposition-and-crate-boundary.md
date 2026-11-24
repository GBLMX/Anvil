# ADR-0074: Anvil-TUI State Decomposition and Crate Boundary

- Status: Proposed; implementation pending
- Date: 2026-11-24
- Related: [ADR-0013](0013-kernel-distribution-boundary.md),
  [ADR-0024](0024-tui-state-and-domain-boundaries.md),
  [ADR-0041](0041-tui-primitive-extraction.md),
  [ADR-0057](0057-tui-surface-model-and-focus-contract.md),
  [ADR-0068](0068-tui-visual-language-and-markers.md)

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
- [Large Rust Workspaces](https://matklad.github.io/2021/08/22/large-rust-workspaces.html)
- [Rust Performance Book: Compile Times](https://nnethercote.github.io/perf-book/compile-times.html)
- [Ratatui: Component Architecture](https://ratatui.rs/concepts/application-patterns/component-architecture/)
- [Ratatui: Flux Architecture](https://ratatui.rs/concepts/application-patterns/flux-architecture/)

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

### 4. Guardrails

- A leaf module may not add a new `impl Model`; expose a state method instead.
- Every page state must be constructible in a unit test without `Model`,
  terminal setup, or a gateway.
- No user-visible behaviour change is bundled into a state split.
- `scripts/tui-metrics.sh` gains ratchets for `impl_model`,
  `impl_model_files`, `model_mut_methods`, and `files_over_800`.
- Dependency direction remains checked by `scripts/architecture-check.sh`.
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

## Acceptance

- `Model` is a composition root; new page/domain behaviour lives on owned state.
- Each extracted page has a unit test that constructs only its state.
- `impl_model` and `model_mut_methods` trend downward; no new `impl Model` is
  introduced in leaf modules.
- `files_over_800` is ratcheted downward without hiding tests.
- `cargo build -p anvil-tui --timings` is recorded before and after crate
  extraction.
- Full workspace tests, clippy, formatting, architecture, and docs checks pass
  with no intentional user-visible behaviour change.

## Supersession note

This ADR resumes the page/state extraction deferred by ADR-0024 on 2026-11-13.
It does not supersede ADR-0024's process/crate model, state-group rules, or
primitive-extraction decision.

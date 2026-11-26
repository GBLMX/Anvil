# TUI View Boundary and Extraction Decision

Status: analysis supporting [ADR-0074](../../adr/0074-anvil-tui-state-decomposition-and-crate-boundary.md);
not an ADR and not a release commitment.

## Question

Should Anvil follow Helix by introducing an `anvil-view` crate now, and move
`settings`, `keymap`, layout, folding, diff, and session orchestration out of
`anvil-tui`?

No. The boundary is useful, but the extraction order in ADR-0074 is not
optional. Move ownership inside `anvil-tui` first, prove a headless path, and
only then extract a crate if a measured boundary and a consumer exist.

## Evidence

Current structural baseline (`scripts/tui-metrics.sh`, 2026-11-24):

```text
TUI files               112
TUI source lines        61,242
TUI dependencies        32
impl Model blocks       71 (66 files)
&mut Model methods      547
Model::new occurrences  557
files over 800 lines    19
```

The top-level `anvil-tui/src` files account for 37,189 lines. A file's line
count is not an ownership boundary: many candidate files still have
`impl Model` methods and direct access to `Model` state.

Helix's own maintainers describe `helix-view` and `helix-term` as a work in
progress and want more self-contained crates later. The transferable rule is
that the view layer does not know the terminal, not that a particular crate
count is the target.

Rust workspace measurements also show the tradeoff: splitting a crate can
narrow an incremental rebuild while increasing total compilation work,
especially around generic instantiation. A crate split needs a measured reason,
not only a cleaner diagram.

## Decision

1. Keep `anvil-tui` as the application crate for now.
2. Do not create `anvil-view` or rename `anvil-tui` to `anvil-term` in the
   current phase.
3. Extract page and domain state ownership inside `anvil-tui` first, following
   ADR-0074.
4. Consider a small `anvil-config` crate before a general `anvil-view` crate:
   settings and keymap parsing have no terminal dependency and are easier to
   validate independently.
5. Consider `anvil-view` only when at least one extraction gate below is true.

## Extraction gates

`anvil-view` may be created only when all of these hold:

- a second consumer exists (headless frontend, GUI, or a real integration
  fixture), or a measured build/ownership problem requires the crate boundary;
- the view state has no dependency on `Model`, terminal application state,
  `GatewayClient`, filesystem workers, PTYs, or LSP/DAP handles;
- the state can be constructed and tested without a terminal or gateway;
- `cargo build -p anvil-tui --timings` and one-file incremental checks identify
  the crate as a meaningful serialization point;
- the resulting dependency graph is acyclic and does not add a
  `session -> view` back edge.

If those conditions are not met, keep the state as an internal module.

## Ownership map

| Candidate | First home | Notes |
|---|---|---|
| `layout`, `folding`, `git_diff` | `anvil-tui` state modules | Pure presentation state; test without terminal |
| settings source resolution | `SettingsState`, then optional `anvil-config` | Coordinate with ADR-0075 and ADR-0076 |
| keymap binding tables | config/state layer | Crossterm event mapping and dispatch stay in the TUI |
| `icon_theme` | `anvil-tui-theme` | Existing theme owner |
| `buffer` | `anvil-editor`/session | Not a generic view container |
| `lsp_runtime` | `anvil-session` + `anvil-lsp-client` | Worker orchestration, not view state |
| `file_io`, `session`, `pkg_install` | session/IPC | Keep gateway transactions outside view state |
| `remote` | `anvil-control` | Control-plane request handling |
| `plugin_document`, `plugins` | `anvil-session` | Lifecycle and orchestration |

`anvil-view` may depend on data-only contracts from `anvil-ipc`,
`anvil-lsp-client`, and `anvil-api`; it must not own their runtime handles.

## Settings inventory

Do not use the unsourced "63 environment variables" figure. The current
inventory and filter are documented in
[settings-sources.md](../../settings-sources.md#environment-inventory).

The key distinction is:

```text
9 per-field editor/search/LSP behavior variables
1 complete-file override: ANVIL_SETTINGS
3 presentation variables
15 per-language LSP command overrides
```

`ANVIL_SETTINGS` is not a per-field settings migration target. It remains a
complete-file override with its own precedence and trust semantics.

## Headless acceptance

The first proof must be an integration transaction, not a crate count:

```text
open a Document
-> apply one WorkspaceEdit
-> save
-> query revision
```

Run it through `anvil-session`, `anvil-editor`, and `anvil-ipc`. The current
`examples/anvil-distro-minimal` is only a distribution descriptor (25 lines);
it proves a second composition consumer, not document transactions.

## Non-goals

- A LOC target or a target number of crates.
- Moving gateway/worker/plugin handles into view state.
- Using `#[cfg(feature)]` as a substitute for state ownership.
- Renaming `anvil-tui` before a stable view boundary exists.

## References

- [ADR-0074](../../adr/0074-anvil-tui-state-decomposition-and-crate-boundary.md)
- [ADR-0024](../../adr/0024-tui-state-and-domain-boundaries.md)
- [TUI maintainability budget](../../tui-maintainability.md)
- [Settings sources](../../settings-sources.md)
- [Helix crate discussion](https://github.com/helix-editor/helix/discussions/10074)
- [The Unit of Rebuild When You Split a Rust Workspace](https://www.mehdi.cz/blog/the-unit-of-rebuild-when-you-split-a-rust-workspace)
- [Rust Performance Book: Compile Times](https://nnethercote.github.io/perf-book/compile-times.html)

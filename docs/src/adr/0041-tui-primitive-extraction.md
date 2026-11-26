# ADR-0041: TUI Primitive Extraction

- Status: Accepted
- Implementation: theme, text search, snippets, comments, workspace-root
  routing, and terminal key normalization moved out of `anvil-tui`
- Date: 2026-11-14
- Related: [0024](0024-tui-state-and-domain-boundaries.md),
  [0037](0037-kernel-api-baseline.md), [0040](0040-helix-keymap-compatibility.md)

## Context

`anvil-tui` had grown to roughly 39k source lines. Its `Model` already groups
state into 24 owned domains, but several independent primitives still lived in
the frontend crate. That made them unavailable to other frontends and kept
the application crate as the only testing and change boundary for unrelated
logic.

The split must not create a second source of truth for editor behavior, and it
must not move `Model`-bound dispatch or rendering out of the frontend yet.

## Decision

Extract only modules that do not reference `Model` or terminal application
state:

| Extracted module | New owner | Boundary |
|---|---|---|
| Theme and palettes | `anvil-tui-theme` | semantic colors, styles, custom theme parsing |
| Text search and match state | `anvil-editor::search` | byte ranges, regex, grapheme-safe matching |
| LSP snippets | `anvil-editor::snippet` | placeholder parsing and tabstop navigation |
| Line comments | `anvil-editor::comment` | comment markers and non-overlapping toggle edits |
| Workspace root routing | `anvil-workspace::roots` | root ownership, trust, relative paths |
| Terminal key normalization | `anvil-tui-kit::input` | key modifiers and action-key classification |

`anvil-tui` keeps thin aliases/re-exports so existing internal call sites and
public paths continue to resolve. `Model`, command dispatch, layout, rendering,
LSP/DAP UI, plugin UI, and workspace UI remain in `anvil-tui` until they have a
real second consumer or a clean state boundary.

## Consequences

Positive:

- The extracted modules have no dependency on `anvil-tui` and are testable on
  their own.
- `anvil-editor`, `anvil-workspace`, and `anvil-tui-kit` now own their natural
  primitives instead of duplicating them in the frontend.
- The new `anvil-tui-theme` crate separates palette/theming churn from the
  application shell.
- Existing `anvil_tui::workspace_roots` and internal `crate::theme` paths remain
  compatible through re-exports.

Negative:

- `anvil-tui` still contains the application model and most UI domains; this is
  a primitive split, not a full frontend rewrite.
- Re-exports are compatibility seams and must not accumulate new behavior.
- `anvil-editor` now directly depends on `regex` and
  `unicode-segmentation` for search.

## Verification

- `cargo test --workspace --no-fail-fast`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
- `scripts/architecture-check.sh`
- `git diff --check`
- Real-TUI `scripts/ctl-smoke.sh`, `scripts/provider-smoke.sh`, and
  `scripts/mcp-smoke.sh`

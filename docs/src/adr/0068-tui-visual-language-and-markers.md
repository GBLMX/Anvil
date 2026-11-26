# ADR-0068: Unified TUI Visual Language and Markers

- Status: Accepted; shared glyphs and file icons implemented 2026-11-20
- Version: 1.0.0
- Date: 2026-11-20
- Related: [ADR-0016](0016-render-style-layers.md),
  [ADR-0057](0057-tui-surface-model-and-focus-contract.md),
  [ADR-0061](0061-workspace-navigation-and-root-boundary.md)
- Supersedes:
- Superseded by:

## Context

TUI surfaces grew independently. They share the semantic theme, but a few
surface-local glyphs and panel conventions drifted:

- the command palette used `❯` while the start page and breadcrumbs used `›`;
- recent markers and separators were literal strings in several modules;
- panel title/border behavior was mostly shared, but tree headers used a
  separate title path.

The result is functional but visually inconsistent across start page, pickers,
breadcrumbs, status, and settings.

## Decision

### 1. One semantic style source

Render code uses `theme::` semantic colors and styles. Surface-local `Color`
literals are not introduced for UI chrome. Icon colors come from the semantic
syntax palette or icon fallback, not from a second palette.

### 2. One glyph vocabulary

Shared glyphs live in `crates/anvil-tui/src/art.rs`:

| Token | Glyph | Use |
|---|---|---|
| `SELECTED` | `›` | selected row/action marker |
| `SELECTED_SPACED` | `› ` | spaced list-row marker |
| `RECENT_SPACED` | `↺ ` | recent command/history marker |
| `PATH_SEPARATOR` | ` › ` | breadcrumb path/symbol separator |
| `STATUS_SEPARATOR` | ` · ` | status segment separator |
| `DOT` | `·` | informational message marker |

New surfaces use these tokens or add a token here; they do not introduce a
local variant of the same concept.

### 3. Panel conventions

- Unfocused panels use rounded borders via `panel_block`.
- The focused panel uses the thick border via `focused_panel_block`.
- Titles use the shared title style and a single surrounding space.
- Selected rows use `theme::selected` plus `SELECTED`/`SELECTED_SPACED`.
- Muted metadata uses `theme::muted` or `theme::dim`, not a raw gray.

The editor tree remains a gutter with its own header, but its title and
selection styling use the same theme and glyph vocabulary.

### 4. Icon routing

File and folder icons use `icon_theme`; ASCII fallback remains available through
the existing appearance setting. The tree, buffer tabs, status line, fuzzy
file finder, buffer picker, changed-files picker, and start-page recents all
resolve icons through that registry. The TUI does not add a second emoji/icon
registry per surface.

## Consequences

Positive:

- Start page, pickers, breadcrumbs, status, and settings read as one product.
- A glyph or separator change has one owner.
- Theme switching applies uniformly without surface-specific colors.

Negative:

- A few existing render tests that assert literal glyphs must use the shared
  tokens or update their expected output.
- The tree gutter is intentionally not a full bordered panel; its title is
  still part of the common visual language.

## Verification

- Command palette, buffer picker, start page, breadcrumbs, status line, and
  message markers all use the shared `art` tokens.
- Focused/unfocused panels use the shared `panel_block`/`focused_panel_block`
  border convention.
- Theme and high-contrast smoke tests render all surfaces.
- No new hardcoded chrome colors are introduced in render code.
- File/folder icons resolve through `icon_theme` in every file-bearing surface,
  including start-page recents and the changed-files picker.

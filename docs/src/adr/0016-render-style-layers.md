# ADR-0016: Render Style Layers

- Status: Accepted; search composition implemented 2026-11-01
- Date: 2026-11-01
- Related: [ADR-0008](0008-selection-first-interaction.md)

## Context

The editor renders syntax highlighting, search matches, the active search
match, selection, cursorline, and LSP diagnostics in one `Line`. Before this
decision, entering search bypassed Tree-sitter spans entirely and rebuilt text
with search-only styles. The result lost syntax foreground colors and made every
mode-specific renderer responsible for reimplementing color composition.

## Decision

Text behavior is layered in this order:

1. Syntax foreground.
2. Search match background.
3. Active search match background plus bold.
4. Selection background.
5. Cursorline background.
6. LSP diagnostic underline/modifier.

Rules:

- Build the syntax spans first, regardless of the input mode.
- Overlays may set background and modifiers; they must not reset the base
  foreground unless the layer explicitly owns foreground (selection and
  diagnostic error text currently do).
- Compose overlays with `Style::patch` so an unset overlay field preserves the
  base style.
- Search rendering must not create a parallel plain-text path.
- A match that crosses text-buffer chunk boundaries uses the same chunk-safe
  matcher as search navigation.
- Active and normal search matches must remain distinguishable without
  replacing syntax foregrounds.

## Implementation

`visible_lines` always asks `push_syntax_spans` for syntax spans. When a search
query is active, `apply_search_highlights` splits the existing spans at match
boundaries and patches only the background:

- normal match: `theme::search_match()`;
- active match: `theme::search_active()`;
- active match is selected by its start offset.

`apply_diagnostic_underlines` runs after search composition. Cursorline is
applied as the line base style; it remains below span-level search backgrounds.

## Verification

- `search_preserves_syntax_and_adds_background` asserts a matched Rust keyword
  keeps `SYNTAX_KEYWORD` as fg and gains the active search background.
- Search navigation still uses `find_matches_in` across chunks.
- Full `anvil-tui` tests, workspace tests, clippy, fmt, and diff checks pass.

## Consequences

Positive:

- Search mode no longer erases syntax colors.
- Search rendering and search navigation share one matching implementation.
- Future overlays have an explicit layer order and a patch-based composition
  rule.

Negative:

- Search rendering allocates owned slices when splitting syntax spans.
- The full style composer is still incremental; each overlay remains in its
  existing renderer until it needs composition with another overlay.

## Deferred

- A universal `StyleComposer` abstraction is not introduced.
- Selection and cursorline composition with search is documented above but not
  reimplemented in this change.
- Diagnostic squiggle rendering remains a terminal-underline approximation.

# ADR-0022: Core Editing Completion and Semantic Ownership

- Status: Proposed
- Date: 2026-11-12
- Related: [ADR-0008](0008-selection-first-interaction.md), [ADR-0010](0010-cursor-and-snapshot-ownership.md), [ADR-0013](0013-kernel-distribution-boundary.md)

## Context

Anvil already has a selection-first editor core with multi-selection,
grapheme-aware motions, atomic edit transactions, undo/redo, text objects,
surround, registers, macros, folding, LSP snippets, and code lens. The text
model has multiple benchmarked backends (`RopeBuffer`, `GapBuffer`,
`PieceTable`) and does not need a rewrite.

The remaining gap is daily editing completeness, not text-engine capability.
Compared with Helix, Anvil lacks several small but high-frequency primitives:

- toggle comment has no implementation; syntax classification recognizes
  comments, but no command toggles them;
- bracket navigation has bracket text objects, but no matching-bracket
  highlight or jump;
- jump/change history has stdin location navigation, but no session jump list
  or change list;
- soft wrap has no display projection; folding, code lens, inlay hints,
  semantic tokens, cursor placement, and mouse hit-testing currently share a
  source-line-to-display-row map without wrapped rows.

There is also an ownership gap. `anvil-editor` owns selection, text, and undo,
but the TUI owns most register, macro, and text-object behavior. That makes the
kernel claim weaker: a control surface or a second frontend cannot reuse the
same editing semantics without going through the TUI.

Helix is a capability reference, not a keymap target. ADR-0008 explicitly keeps
Anvil selection-first and treats modal key translation as an adapter.

## Decision

### 1. Keep the selection-first core

Do not add a second native modal kernel or an operator-pending core state
machine. The TUI may hold pending key sequences, but it invokes core actions.
Vim/Helix keymap compatibility remains an adapter concern.

### 2. Move editing semantics down, not just features across

Registers, macros, text objects, undo grouping, and selection invariants belong
to `anvil-editor`/`anvil-session`. The TUI owns:

- key sequence recognition and pending-key display;
- rendering and display projections;
- prompt/overlay state.

The TUI may keep input-recording state for macros, but the resulting register
contents and replay semantics must be reusable by another frontend or a
headless consumer. `anvil-editor` must not remain a single unnamed-yank store
while the TUI owns the real register model.

### 3. Add missing primitives in this order

1. **Toggle comment**
   - Works on every selection, including empty carets and mixed indentation.
   - Uses language metadata for line/block comment tokens; the syntax
     highlighter is not the source of truth for comment insertion.
   - A failed or unsupported language leaves text unchanged and reports a
     visible error.
   - The whole operation is one undo group.

2. **Jump list and change list**
   - Records definition, reference, diagnostic, search, quick-open, and
     conflict jumps before moving.
   - `Ctrl-o`/`Ctrl-i` traverse jump entries; change-list traversal is
     separate from file navigation.
   - Per-session bounded history; deduplicate adjacent entries; drop oldest
     entries first.
   - Entries carry file identity and byte offset, not only a display row.

3. **Bracket matching**
   - Highlights the matching bracket when the caret is on or immediately after
     a bracket.
   - Jump uses the existing bracket parser/range logic where possible.
   - Respects nesting, strings, comments, and escaped delimiters; unsupported
     languages degrade to no highlight rather than a wrong match.
   - Scan work is bounded per request.

4. **Soft wrap**
   - Implemented as a TUI display projection, not a mutation of source text.
   - Default off; a setting controls it.
   - The display map must be the single source for cursor placement, selection
     rendering, folds, code lens rows, inlay hints, mouse hit-testing, and
     horizontal scrolling.
   - Wrapped rows are bounded per source line; overflow is visible, not
     silently truncated without a marker.
   - Search and diagnostic positions still use byte offsets in source text.

### 4. Preserve existing behavior and make additions explicit

New commands are additive. Soft wrap defaults off. Existing keybindings are not
changed merely to match Helix. Comment toggling and bracket matching must not
rewrite text when the language is unknown or the selection is read-only.

### 5. Out of scope

- Persistent undo history.
- Vimscript compatibility.
- A stable Rust ABI or dynamic Rust plugin loading.
- Plugin-defined motion/action semantics.
- A full structural/syntax selection engine in this ADR.

Persistent undo remains rejected for the internal-polish phase; adding it
later requires its own ADR because edit records are offset-based and restore
currently starts a fresh undo stack.

## Consequences

Positive:

- The kernel can actually provide reusable editing semantics.
- The most frequent missing editing operations land without a text-model
  rewrite.
- Soft wrap gets one display-map owner instead of ad-hoc coordinate fixes.
- Helix capability parity improves without copying its keymap.

Negative:

- Moving register/macro/text-object semantics out of the TUI is a refactor
  with parity-test cost.
- Soft wrap touches nearly every rendering and hit-testing path.
- Comment toggling needs per-language metadata beyond the current syntax
  queries.
- Jump/change history adds session state that must be restored or explicitly
  discarded.

## Alternatives considered

### Rewrite the text model first (rejected)

The current rope/gap/piece-table model is benchmarked and bounded. The missing
capabilities are interaction and projection problems, not storage problems.

### Copy Helix keybindings and behavior (rejected)

ADR-0008 keeps a selection-first core with an adapter keymap. Copying Helix
bindings would create a second compatibility surface without improving the
kernel.

### Put all editing semantics in the TUI (rejected)

That preserves short-term velocity but makes control/plugin/headless parity
depend on the TUI and weakens ADR-0013's kernel boundary.

### Implement soft wrap as a core text transform (rejected)

Soft wrap must not insert newline characters or change undo history. It is a
display projection.

### Implement persistent undo now (rejected for this phase)

Offset-based edit records are only valid against one exact text state. Restore
keeps the exact buffer or swap text and starts a fresh undo stack. Persistent
undo needs its own storage and migration design.

## Verification expectations

- Core tests cover multi-selection comment toggling, empty selections,
  read-only buffers, unknown languages, and undo grouping.
- Jump-list tests cover definition/search/diagnostic/quick-open entries,
  deduplication, bounds, and file deletion.
- Bracket tests cover nested pairs, strings, comments, escaped delimiters, and
  unsupported languages.
- Soft-wrap tests cover cursor movement across wrapped rows, selection,
  folding, code lens, inlay hints, mouse hit-testing, horizontal scroll, and
  narrow terminals.
- Control/plugin parity tests exercise comment toggling, register contents, and
  macro replay without a TUI key event.
- `cargo test --workspace --no-fail-fast`, clippy, fmt, and a real TUI smoke
  test pass for each slice.

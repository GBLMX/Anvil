# ADR-0010: Core Owns Coordinates; the Editor Layer Owns Snapshots

- Status: Proposed for owner sign-off
- Date: 2026-10-30
- Related: [ADR-0004](0004-text-model.md), [ADR-0008](0008-selection-first-interaction.md), [ADR-0009](0009-text-model-snapshot-gate.md)

## Context

M4 needs three things that do not exist yet:

- byte-to-line and line-to-byte lookup;
- byte/character/grapheme/display-cell conversion for selections;
- an immutable editor snapshot that can produce `editor.get_view`.

The text backend is selected (`TextDocument = PieceTable`), but it only exposes
whole-buffer edits and line counts. `anvil-api` owns the wire-facing
`TextPosition`, `Selection`, `Viewport`, and `EditorView` types, and deliberately
has no editor dependency. There is no TUI or editor binary yet.

If the TUI owns coordinate conversion, the same position will be computed twice:
once for selection semantics and once for rendering. If `anvil-api` owns it, the
contract crate acquires text-buffer and Unicode dependencies. Both outcomes make
the eventual editor the wrong kind of client.

## Decision

1. **`anvil-core` owns coordinate and cursor primitives.** It exposes
   `offset_to_line`, `line_to_offset`, and byte/grapheme/display conversion for a
   document. The line index is maintained incrementally on insert/delete; it is
   not rebuilt per frame.

2. **`anvil-core` gains `unicode-segmentation` and `unicode-width`.** These are
   text semantics, not UI dependencies. `anvil-core` must not depend on
   `anvil-api`, Tokio, `ratatui`, the plugin runtime, or the gateway.

3. **A small `anvil-editor` layer owns mutable editor state and snapshots.** It
   depends on `anvil-core` and `anvil-api`, and constructs
   `anvil_api::EditorView`. Neither core nor the TUI constructs the wire view.

4. **`anvil-api` stays a pure DTO crate.** It may be used by the editor layer,
   but it must not depend on `anvil-core` or the TUI. Wire types remain stable
   and generated bindings continue to come from this crate.

5. **Editor snapshots carry full positions.** `EditorSnapshot` contains:
   - `Arc<TextDocument>` and `Arc<LineIndex>`;
   - immutable revision and viewport;
   - selections whose anchor/head carry byte offset, line, grapheme column, and
     display column;
   - `interaction_mode`.

   The snapshot is cheap to clone and must never copy the document. The TUI
   receives these positions; it does not recompute them.

6. **Display columns use a fixed Phase 0 policy.** Lines are LF-delimited. A tab
   advances to the next multiple of 4 cells. `\r` and other control characters
   have zero display width. Configuration of tab width is deferred; changing it
   is an editor setting, not a snapshot schema change.

7. **Selection mapping across edits is byte-based and deterministic.** For an
   edit replacing `[start, end)`:
   - offsets before `start` stay unchanged;
   - offsets inside the replaced range clamp to `start`;
   - offsets at or after `end` shift by the byte delta;
   - an insertion is the empty-range case at `start`.

   Anchor and head map independently, preserving direction. Empty selections
   stay empty. The editor layer records selection state per command so undo and
   redo restore the pre-command selection; text history itself remains
   selection-free.

8. **Snapshot immutability is structural.** `EditorSnapshot` is built from
   immutable text/index references and owned vectors. No mutable editor handle
   is exposed through the snapshot, and no snapshot path clones the document
   text.

## Consequences

Positive:

- One conversion implementation serves selection, motions, rendering, and the
  plugin API.
- The TUI remains a renderer/input adapter, not a second coordinate engine.
- `anvil-api` stays small and binding-friendly.
- Snapshot creation remains O(1) in document size.

Negative:

- `anvil-core` gains two small Unicode dependencies.
- A new `anvil-editor` crate is required before `EditorView` can be produced.
- Selection-aware undo requires editor-layer bookkeeping in addition to the
  existing text-only `History`.

## Rejected alternatives

### TUI owns coordinates

Duplicates cursor semantics and makes headless tests or plugin calls depend on a
terminal layer. Rejected.

### `anvil-api` owns coordinates

Forces text-buffer and Unicode dependencies into the generated-contract crate,
and makes a wire schema the owner of editor semantics. Rejected.

### Snapshot carries byte offsets only

Would make every consumer recompute line, grapheme, and display columns, and
would allow the TUI and plugin API to disagree. Rejected.

### Rebuild the line index per snapshot

Makes `EditorView` O(document size) and violates the no-full-document-copy
snapshot contract. Rejected.

## Verification expectations

- Core tests cover empty buffers, multi-byte characters, combining marks, wide
  characters, tabs, CRLF, and line boundaries at EOF.
- `offset_to_line` and `line_to_offset` round-trip across every line boundary.
- Selection mapping tests cover both directions, empty selections, insertion at
  a boundary, deletion containing an endpoint, and undo/redo restoration.
- Snapshot tests prove `Arc` sharing and immutability; no document text copy is
  permitted.
- `anvil-editor` tests construct `EditorView` from a snapshot without duplicating
  coordinate logic.
- No test path requires TUI, Tokio, or a plugin runtime in `anvil-core`.

## Sign-off

- Owner: pending
- Reviewer: pending

M4 may implement cursor primitives against this ADR once the owner accepts it.
Until then, `set_selection` and other cursor-related API additions stay blocked.

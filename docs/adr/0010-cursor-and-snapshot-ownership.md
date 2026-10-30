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
   document.

   `LineIndex` is a chunked persistent index, not a flat `Vec<usize>`:
   - each chunk targets 1024 lines and may hold up to 2048 lines; it stores its
     absolute base offset, first line number, byte end, and an `Arc<[usize]>` of
     relative line starts;
   - a snapshot shares the chunk vector and all line-start slices;
   - an edit clones the small chunk vector, copies only the affected chunk's
     line starts, and shifts later chunk bases/line numbers;
   - no edit copies every line start or the document text.

   Complexity:
   - snapshot: `O(1)`;
   - first edit after snapshot: `O(chunks + 2048)`, not `O(lines)`;
   - `offset_to_line`: `O(log chunks + log 1024)`;
   - `line_to_offset`: `O(log chunks)`; after the chunk is found,
     `starts[line - first_line]` is an O(1) array index. A binary search inside
     the chunk is not used.

   `first_line` is mandatory. If an implementation omits it and must search the
   chunk's line starts, the bound becomes `O(log chunks + log 1024)`; that
   variant is not this design.

   Chunk normalization is defined below and keeps chunks bounded.

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
   - for an insertion (`start == end`), offsets at or after `start` shift by the
     inserted length, including a caret exactly at the insertion point.

   Anchor and head map independently, preserving direction. Empty selections
   stay empty. The editor layer records selection state per command so undo and
   redo restore the pre-command selection; text history itself remains
   selection-free.

8. **Snapshot immutability is structural.** `EditorSnapshot` is built from
   immutable text/index references and owned vectors. No mutable editor handle
   is exposed through the snapshot, and no snapshot path clones the document
   text.

## LineIndex maintenance

Constants:

- `TARGET_LINES = 1024`
- `MAX_LINES = 2048`
- `MIN_LINES = 256`

Normalization runs synchronously at the end of every successful edit. There is
no dirty-chunk or lazy-rebuild path:

- a chunk above `MAX_LINES` splits at `TARGET_LINES`;
- a chunk below `MIN_LINES` merges with its smaller neighbor when their sum is
  at most `MAX_LINES`;
- if that sum exceeds `MAX_LINES`, the pair is rebalanced into two chunks at the
  midpoint;
- only the final chunk may remain below `MIN_LINES`; every other chunk is within
  `[MIN_LINES, MAX_LINES]`.

Split/merge rebuilds only the affected chunk headers and `Arc<[usize]>` arrays.
It does not scan the document or rebuild the whole index.

`LineIndex` is independent of PieceTable piece boundaries. An edit is applied by
byte range and a `TextBuffer` reference:

- the affected chunk range is found from the old index;
- the line starts of every intersecting chunk are rebuilt from buffer slices;
- later chunk bases and first-line numbers are shifted by the byte/line delta;
- piece boundaries are irrelevant: one piece spanning multiple chunks rebuilds
  all touched chunks, and multiple pieces inside one chunk rebuild one chunk.

An edit that enters or leaves a line rebuilds the boundary chunk as well as the
directly intersecting chunks. This keeps `offset_to_line` valid immediately
after the edit; it does not defer work to the query path.

With the occupancy invariant, the chunk count is bounded by
`lines / MIN_LINES + 1` (plus the final partial chunk). A 100 MB synthetic file
therefore stays in the low thousands of chunks even after long edit sessions.

## Line semantics

`LineIndex` must agree with `TextBuffer::len_lines` exactly:

- empty text has zero lines and no line starts;
- `\n` is the only line delimiter;
- `\r\n` is one delimiter, and `\r` is ordinary line content with zero display
  width;
- a file without a trailing `\n` still counts its final content line;
- a file ending in `\n` has a final empty line starting at `len`;
- offsets at `\n` belong to the preceding line; offsets immediately after it
  belong to the next line.

| Text | `len_lines` | Line starts |
|---|---:|---|
| `""` | 0 | `[]` |
| `"a"` | 1 | `[0]` |
| `"a\n"` | 2 | `[0, 2]` |
| `"a\r\n"` | 2 | `[0, 3]` |
| `"a\r\nb"` | 2 | `[0, 3]` |
| `"\n"` | 2 | `[0, 1]` |

For empty text, `offset_to_line(0)` returns `None` and `line_to_offset` rejects
line 0. The editor represents an empty-document caret separately; it must not
invent a line that `TextBuffer::len_lines` does not count.


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

### Flat line-start `Vec` with `Arc::make_mut` (rejected)

Copies every line start on the first edit after a snapshot. That converts an
O(chunks) metadata copy into O(lines) and defeats the same retained-snapshot
rule the text buffer follows. Rejected.

## Verification expectations

- Core tests cover empty buffers, multi-byte characters, combining marks, wide
  characters, tabs, CRLF, and line boundaries at EOF.
- `offset_to_line` and `line_to_offset` round-trip across every line boundary.
- Selection mapping tests cover both directions, empty selections, insertion at
  a boundary, deletion containing an endpoint, and undo/redo restoration.
- `LineIndex` tests prove an edit does not clone every line start: unaffected
  chunk `Arc<[usize]>` pointers remain shared, the retained snapshot's offsets
  are unchanged, and the edited index has the new line boundaries.
- Line-index semantics tests assert the exact table above for empty, trailing-LF,
  CRLF, and trailing-content cases, and compare every result with
  `TextBuffer::len_lines`.
- Normalization tests cover split above 2048, merge below 256, rebalance when a
  merge would exceed 2048, and the final-chunk exception.
- Piece-boundary tests cover an edit crossing pieces inside one chunk and an
  edit crossing multiple chunks inside one piece.
- A 100k-edit randomized stress run on the 100 MB reference checks that chunk
  count stays within 2× the initial count, every non-final chunk remains within
  `[256, 2048]`, and `offset_to_line` p99 does not regress by more than 10%.
- The snapshot first-edit benchmark includes `LineIndex` work and reports the
  chunk path cost; it must stay far below a full line-array copy.
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

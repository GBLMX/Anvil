# Lua Plugin Mouse, Overlay, and Dependency-Graph Plan

Status: proposed. The plugin API is frozen at `1.0.0`; these additions require
an additive `1.1.0` contract and a new ADR. The two target plugins are:

- RTS-style rectangle mouse selection that installs multiple cursors.
- Tactical Graph: a force-directed workspace dependency overlay.

They are **not** pure-Lua features today. The Lua code should own the state
machine, layout, and policy; the editor must own coordinate mapping, selection
validation, overlay compositing, and dependency extraction.

## 1. Goals

- Let a plugin capture mouse input only for the lifetime of an explicit,
  scoped command.
- Convert every mouse event to an editor-owned `TextPosition` before it reaches
  Lua.
- Let plugins draw bounded highlights and sparse canvas overlays without
  owning the whole terminal.
- Install multiple selections atomically from plugin-requested byte ranges.
- Expose a bounded workspace dependency graph backed by the workspace index,
  not the gateway.
- Implement both features as Lua plugins with no core graph/layout algorithm.

## 2. Non-goals

- Arbitrary full-screen raw cell matrices.
- Global mouse subscriptions or plugins receiving mouse events without capture.
- Letting plugins construct `TextPosition` values from raw line/column numbers.
- A dependency analyzer in the gateway or plugin host.
- Remote/network graph sources.
- macOS/Windows behavior in this slice.

## 3. Current gaps

| Proposed API | Current state |
|---|---|
| `event.mouse` | Rust TUI handles crossterm mouse events in `keys/mod.rs`; no plugin event channel |
| `event.subscribe` | Declared in bindings, but `plugin_document.rs` has no dispatcher |
| `editor.set_multi_cursors` | Internal multi-cursor exists; no plugin method |
| `ui.draw_highlight` | Absent |
| `ui.render_canvas` | Absent |
| `workspace.get_file_dependencies` | Absent; gateway is not a semantic graph provider |

The current plugin dispatcher supports only `editor.get_view`,
`editor.get_snapshot`, `editor.set_mode`, `editor.apply_edit`,
`editor.register_command`, `editor.register_provider`, `ui.notify`, and
`ui.status`.

## 4. Contract changes (API 1.1.0)

All additions are additive and capability-gated. Existing 1.0.0 plugins remain
valid. Generated Rust/JSON Schema/TypeScript/Luau bindings and the normative
plugin spec must change in the same commit.

### 4.1 Mouse capture

```text
editor.begin_mouse_capture
  request:  { events: ["down", "drag", "up"], cursor: "crosshair" }
  response: { capture_id: u64, revision: u64 }

editor.end_mouse_capture
  request:  { capture_id: u64 }
  response: {}
```

Rules:

- One capture owner at a time. A second request fails with a typed error.
- Capture is scoped to a plugin command; leaving the command, pressing Escape,
  plugin crash, or host shutdown releases it exactly once.
- Mouse events are sent only to the capture owner. No global `event.subscribe`
  for mouse.
- The payload carries `capture_id`, `kind`, `button`, `modifiers`, `revision`,
  and an editor-owned `TextPosition` plus viewport. Raw screen coordinates may
  be included for diagnostics but are not sufficient to construct a selection.

### 4.2 Temporary highlights

```text
ui.set_highlights
  request:  { layer_id: string, generation: u64, ranges: [TextRange],
              style: "selection" | "muted", ttl_ms: u32 }
ui.clear_highlights
  request:  { layer_id: string }
```

Rules:

- Ranges are half-open UTF-8 byte ranges and are clipped to the document.
- `generation` is monotonic per layer; stale generations are ignored.
- The core composites highlights; plugins do not add cells or styles.
- `layer_id` is owned by the calling plugin and disappears on plugin stop.
- Limits: 4096 ranges per layer, 64 KiB serialized request, TTL <= 5 s.

### 4.3 Multiple selections

```text
editor.set_selections
  request:  { base_revision: u64, ranges: [TextRange], primary: u32 }
  response: { revision: u64, applied: u32 }
```

Rules:

- `TextRange` byte offsets, not plugin-constructed line/column pairs.
- Reject on revision mismatch, out-of-bounds offsets, non-UTF-8 boundaries,
  duplicate selections, unsorted overlapping ranges, or more than 4096 ranges.
- Normalize range order and choose a valid primary; set all selections in one
  undoable transaction.
- An empty range list clears to one caret; it never leaves zero selections.

### 4.4 Sparse canvas overlay

```text
ui.create_overlay
  request:  { overlay_id: string, bounds: { x, y, width, height },
              max_cells: u32 }
ui.patch_overlay
  request:  { overlay_id: string, generation: u64,
              cells: [{ x, y, char, style }] }
ui.destroy_overlay
  request:  { overlay_id: string }
```

Rules:

- Overlay bounds are clipped to the editor content area; status/command-line
  rows are not writable.
- Sparse cells only; no full-screen matrix, no raw ANSI, no arbitrary colors.
  Styles come from a small name set mapped to theme colors.
- Limits: 4096 cells per overlay, 256 cells per patch, 60 patches/second,
  generation monotonic, plugin-owned cleanup.
- The core owns invalidation and z-order. Canvas overlays are composited under
  notifications and command-line popups.

### 4.5 Dependency graph

```text
workspace.get_dependencies
  request:  { root: string }
  response: { generation: u64, nodes: [{ id, label, path }],
              edges: [{ from, to }], truncated: bool }
```

Rules:

- Served by the workspace index service, not the gateway and not `sys.exec`.
- Paths must stay inside the requested trusted root.
- Initial provider is workspace-index extraction; language-specific providers
  must be explicit and registered, not guessed.
- Limits: 4096 nodes, 16384 edges, 1 MiB response; truncation is explicit.
- No file contents are returned; only path identity, label, and edges.

### 4.6 Open file from the workspace

```text
workspace.open_file
  request:  { root: string, path: string }
  response: { opened: bool }
```

Rules:

- `root` must identify an existing trusted workspace root.
- `path` is workspace-relative; absolute paths, `..`, NUL bytes, and symlink
  escapes are rejected. The core canonicalizes through the existing
  workspace/file-worker path.
- Only regular text/binary-previewable files are opened; the core uses its
  existing open path, preview policy, and size limits. No shell or `sys.exec`.
- The response is `opened: false` only when the core deliberately leaves the
  current buffer unchanged; validation failures are typed RPC errors.
- Capability: `workspace.open`.

## 5. Deliverable slices

### Slice 1: plugin event transport

- Dispatch `event.subscribe` only for approved non-mouse events.
- Add editor-to-host notification transport with bounded queue and coalescing.
- Lua callback dispatch and host integration tests.

Acceptance: a subscribed event reaches Lua once; overflow drops notifications
without blocking the editor; plugin stop removes the subscription.

### Slice 2: scoped mouse capture

- Implement `editor.begin_mouse_capture` / `editor.end_mouse_capture`.
- Map mouse events through scroll, soft wrap, tabs, wide characters, splits, and
  overlays.
- Release capture on Escape, command completion, plugin crash, and host stop.

Acceptance: only the capture owner sees events; coordinates map correctly in
each layout case; no leaked capture after failure.

### Slice 3: highlights

- Implement `ui.set_highlights` / `ui.clear_highlights`.
- Render over the editor content area with clipping and stale-generation rules.

Acceptance: rectangle drags produce a visible temporary highlight; plugin stop
clears it; 4096 ranges stay within the input bound.

### Slice 4: multi-selection

- Implement `editor.set_selections` with revision and UTF-8-boundary validation.
- Reuse the existing multi-cursor transaction path and undo grouping.

Acceptance: valid ranges create the expected selections; stale, invalid, and
overlapping ranges are rejected without mutating state; undo restores the prior
selection set.

### Slice 5: sparse canvas overlay

- Implement create/patch/destroy with bounds, cell, rate, and style limits.
- Add snapshot/diff tests and a real TUI smoke.

Acceptance: a plugin can draw an 80x24 graph within limits; status line and
command-line rows remain core-owned; invalid/oversized patches are rejected.

### Slice 6: dependency graph

- Add the workspace-index graph provider and `workspace.get_dependencies`.
- Add `workspace.open_file` with trusted-root confinement and the existing
  open/preview/size policy.
- Cache by root + generation; invalidate on workspace changes.

Acceptance: nodes/edges stay inside the trusted root; a large workspace returns
`truncated`; stale generations are rejected; open-file rejects traversal,
absolute paths, symlink escape, and untrusted roots; no gateway RPC is added.

### Slice 7: RTS multiple-cursor plugin

- Command starts capture, mouse-down/drag updates `ui.set_highlights`,
  mouse-up converts the selected positions to `TextRange`s, then calls
  `editor.set_selections` and releases capture.
- Rectangle selection uses display columns; newlines are skipped; selections
  are sorted and deduplicated by core.
- Escape cancels, leaving the original selection intact.

Acceptance: drag-select creates the expected carets; typing applies to all
selections; cancel/undo have no side effects.

### Slice 8: Tactical Graph plugin

- Fetch one bounded graph snapshot; nodes carry a trusted root and a
  workspace-relative path.
- Compute Fruchterman-Reingold layout in a Lua coroutine with an iteration cap.
- Render sparse `●`/`─`/`│` cells through one overlay.
- Mouse hover updates a small highlight; right-drag/scroll update view state.
- Enter calls `workspace.open_file` for the selected node. On success the core
  opens it and the plugin removes the overlay; on a validation/open error the
  overlay stays open and an error notification is shown.

Acceptance: 80x24 overlay renders; graph remains responsive at the configured
node cap; no per-frame full-screen payload; plugin stop removes the overlay;
Enter never opens a path outside the node's trusted root.

## 6. Security and bounds

- New capabilities: `editor.mouse_capture`, `ui.highlights`,
  `ui.overlay`, `workspace.graph`, `workspace.open`.
- Capabilities are declared in the manifest and approved before activation.
- Plugins cannot write status/command-line rows, raw ANSI, or outside overlay
  bounds.
- Mouse capture is exclusive and time-bounded.
- All dimensions and counts are checked before allocation or rendering.
- No direct filesystem or gateway access is added for graph features.

## 7. Tests and verification

- Generated API bindings: `cargo run -p anvil-api --example export_bindings -- --check`.
- Schema/TypeScript/Luau parity and API version negotiation tests.
- Mouse mapping matrix: scroll, soft wrap, tab, wide glyph, split, overlay,
  scrolled horizontal viewport.
- Capture lifecycle: duplicate capture, Escape, command exit, plugin panic,
  host timeout, host restart.
- Selection validation: stale revision, UTF-8 boundary, out-of-range, duplicate,
  overlap, 4096 limit, undo/redo.
- Highlight/overlay: clipping, TTL, stale generation, plugin cleanup, cell and
  rate limits.
- Graph: trusted-root confinement, stale generation, node/edge truncation,
  response size.
- Open file: relative path happy path, absolute path, `..`, symlink escape,
  untrusted root, deleted file, and oversized/binary policy; no shell or
  `sys.exec` path.
- TUI smoke for both plugins; `cargo test --workspace`, clippy, fmt, plugin
  host smoke, and real gateway TUI smoke.
- Performance: input-to-highlight latency, overlay patch frame cost, and no
  editor-frame regression against the temporary reference baseline.

## 8. Exit criteria

- API 1.1.0 is additive, documented, and generated bindings are committed.
- Both plugins work in a real TUI through the control/MCP-capable host.
- Mouse capture, highlight, overlay, and selection limits have tests.
- No new gateway dependency-analysis RPC exists.
- `workspace.open_file` is confined to a trusted root and covered by
  traversal, symlink, and policy tests.
- No status/command-line spoofing or unbounded allocation is possible.
- Normal editing, existing multi-cursor behavior, and plugin API 1.0.0 remain
  unchanged.

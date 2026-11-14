# Lua Plugin Tactical Interaction Plan

Status: proposed. The plugin API is frozen at `1.0.0`; these additions require
an additive `1.1.0` contract and a new ADR. The three target plugins are:

- RTS-style rectangle mouse selection that installs multiple cursors.
- Tactical Graph: a force-directed workspace dependency overlay.
- Alert System: debounced visual/terminal-bell warnings for new errors.

They are **not** pure-Lua features today. Lua owns the state machine, layout,
and alert policy; the editor owns coordinate mapping, selection validation,
overlay compositing, diagnostics summaries, and dependency extraction.

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
- Deliver debounced diagnostics summaries and a core-owned alert/bell primitive.
- Implement all three features as Lua plugins with no core graph/layout
  algorithm.

## 2. Non-goals

- Arbitrary full-screen raw cell matrices.
- Global mouse subscriptions or plugins receiving mouse events without capture.
- Letting plugins construct `TextPosition` values from raw line/column numbers.
- A dependency analyzer in the gateway or plugin host.
- A new inotify watcher implementation; graph refresh consumes bounded
  workspace-change events from the existing watcher path.
- Arbitrary audio playback or plugins drawing outside the editor content area.
  Alerts use a core-owned visual cue and optional terminal bell.
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
| `lsp.diagnostics_updated` | Diagnostics are internal `LspEvent`s; no plugin event subscription |
| `ui.alert` / `ui.bell` | Absent; no terminal flash/bell primitive for plugins |
| incremental file watching | FileWorker polls signatures; no inotify stream |

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
ui.draw_lines
  request:  { overlay_id: string, generation: u64,
              lines: [{ x1: u16, y1: u16, x2: u16, y2: u16,
                        style: string }] }
```

Rules:

- Overlay bounds are clipped to the editor content area; status/command-line
  rows are not writable.
- Sparse cells only; no full-screen matrix, no raw ANSI, no arbitrary colors.
  Styles come from a small name set mapped to theme colors.
- Limits: 4096 cells per overlay, 256 cells per patch, 60 patches/second,
  generation monotonic, plugin-owned cleanup.
- `ui.draw_lines` is rendered by the core with clipping and Bresenham
  rasterization. It is a renderer only: no graph/layout algorithm runs in the
  core. Limits: 512 segments, 2048 cells per segment, and the same 60 Hz
  generation/rate bound.
- The core owns invalidation and z-order. Canvas overlays are composited under
  notifications and command-line popups.

### 4.5 In-memory dependency graph

```text
workspace.get_dependencies
  request:  { root: string }
  response: { generation: u64, nodes: [{ id, label, path, lines }],
              edges: [{ from, to }], truncated: bool }
```

`workspace.graph_changed` is the corresponding subscription event (section
4.7).

Rules:

- The authoritative source is the in-memory workspace graph projection fed by
  session buffer revisions; a disk save is not required to refresh the graph.
- The initial graph is seeded from the workspace index; open buffers replace
  the indexed view immediately. Filesystem watching is reconciliation only.
- Dependency extraction is a trusted workspace/session service using explicit
  language providers (Tree-sitter/import parsing). LSP diagnostics are not a
  generic dependency source.
- Debounce/coalesce per root (target 50 ms), assign a monotonic generation, and
  publish deltas. The parser may reparse a changed buffer, but must not rescan
  the whole workspace on every keystroke.
- Paths must stay inside the requested trusted root.
- Limits: 4096 nodes, 16384 edges, 1 MiB full snapshot; deltas are bounded to
  256 nodes, 4096 edges, and 256 KiB. Truncation is explicit.
- No file contents are returned; only path identity, label, line count, and
  edges. No gateway or `sys.exec` involvement.

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

### 4.7 Diagnostics events and alert primitives

```text
event.subscribe
  allowed events: diagnostics.changed, workspace.graph_changed
  diagnostics.changed payload:
    { root: string, path: string, generation: u64,
      error_count: u32, warning_count: u32,
      first_error: Option<string> }
  workspace.graph_changed payload:
    { root: string, generation: u64,
      changed_nodes: [{ id, label, path, lines }],
      changed_edges: [{ from, to }],
      removed_paths: [string], truncated: bool }

ui.alert
  request:  { severity: "warning" | "error", message: string, ttl_ms: u32 }
  response: {}

ui.bell
  request:  {}
  response: {}
```

Rules:

- The proposal's `lsp.diagnostics_updated` is exposed under the single name
  `diagnostics.changed`; do not add both aliases.
- Diagnostics are coalesced per path before the event; `first_error` is
  truncated to 512 bytes. Full diagnostic payloads are not pushed to plugins.
- The Graph plugin subscribes to `diagnostics.changed` and owns its red node
  markers. The Alert plugin owns only `ui.alert`/`ui.bell`; plugins never
  mutate another plugin's overlay or graph state.
- The Alert plugin tracks transitions and applies its own debounce window; LSP
  storms must not create a notification per publish.
- `ui.alert` is a core-owned visual cue inside the editor frame. It cannot draw
  outside the editor content area or replace the status/command line.
- `ui.bell` is the terminal BEL only. No arbitrary audio process or OSC sound
  sequence is exposed.
- `workspace.graph_changed` comes from the in-memory projection and does not
  require a save; watcher events are reconciliation only. This plan does not
  add an inotify dependency.
- Capabilities: `diagnostics.read`, `ui.alert`, `ui.bell`, `workspace.graph`.

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

### Slice 5: sparse canvas overlay and lines

- Implement create/patch/destroy with bounds, cell, rate, and style limits.
- Implement `ui.draw_lines` with core clipping/Bresenham and segment/cell caps;
  no graph algorithm is added to the core.
- Add snapshot/diff and line-clipping tests plus a real TUI smoke.

Acceptance: a plugin can draw an 80x24 graph within limits; status line and
command-line rows remain core-owned; invalid/oversized patches and line
segments are rejected.

### Slice 6: dependency graph

- Add the session-buffer graph projection and `workspace.get_dependencies`.
- Trigger extraction from buffer revisions, coalesce per root, and publish
  generation deltas through `workspace.graph_changed`; no save required.
- Seed initially from the workspace index; watcher events are reconciliation.
- Add `workspace.open_file` with trusted-root confinement and the existing
  open/preview/size policy.
- Cache by root + generation; reject stale generations.

Acceptance: typing an import into an unsaved buffer adds the edge without
Ctrl+S; deleting it removes the edge; 1000+ nodes coalesce without an event
storm; stale generations are rejected; open-file rejects traversal, absolute
paths, symlink escape, and untrusted roots; no gateway RPC is added.

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

- Fetch one bounded graph snapshot; subscribe to
  `workspace.graph_changed` and `diagnostics.changed`.
- Apply deltas to its own local graph; latest generation wins, stale events are
  dropped. The graph plugin owns red diagnostic markers.
- Compute Fruchterman-Reingold layout in a Lua coroutine with an iteration cap;
  layout runs on the latest snapshot only.
- Render nodes with sparse `●` cells and edges with `ui.draw_lines`; core
  Bresenham/clipping only, no graph algorithm in the core.
- Mouse hover updates a small highlight; right-drag/scroll update view state.
- Enter calls `workspace.open_file` for the selected node. On success the core
  opens it and the plugin removes the overlay; on a validation/open error the
  overlay stays open and an error notification is shown.

Acceptance: an unsaved import edit produces a graph delta and a new edge without
Ctrl+S; deleting it removes the edge; 80x24 overlay renders; red dots come only
from this plugin's diagnostics subscription; graph remains responsive while
typing; no per-frame full-screen payload; plugin stop removes the overlay;
Enter never opens a path outside the node's trusted root.

### Slice 9: Alert System plugin

- Subscribe to coalesced `diagnostics.changed` summaries per path.
- Track new versus repeated errors with a bounded per-path debounce; do not
  alert on unchanged diagnostics.
- Call `ui.alert` for the visual cue and optionally `ui.bell` when enabled.
- The Alert plugin does not mutate the graph overlay. The Graph plugin owns
  red dots via its own `diagnostics.changed` subscription.
- No LSP error text is written to the terminal outside the core alert/status
  surfaces.

Acceptance: an LSP storm produces at most one alert per path/debounce window;
an unchanged republish produces none; the bell is opt-in; plugin stop clears
its alerts; the Alert plugin never mutates another plugin's overlay; no
arbitrary audio process is started.

## 6. Security and bounds

- New capabilities: `editor.mouse_capture`, `ui.highlights`,
  `ui.overlay`, `ui.alert`, `ui.bell`, `workspace.graph`, `workspace.open`,
  `diagnostics.read`.
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
  rate limits; `ui.draw_lines` clipping/Bresenham bounds and segment cap.
- Graph: trusted-root confinement, stale generation, node/edge truncation,
  response size; unsaved buffer edit adds/removes edges; 1000+ node/typing
  coalescing; no save required.
- Diagnostics: coalescing per path, first-error truncation, unchanged
  republish produces no alert, storm/debounce behavior, opt-in bell.
- Ownership: Graph subscribes to diagnostics and owns red dots; Alert only
  calls `ui.alert`/`ui.bell`; no cross-plugin overlay mutation.
- Open file: relative path happy path, absolute path, `..`, symlink escape,
  untrusted root, deleted file, and oversized/binary policy; no shell or
  `sys.exec` path.
- TUI smoke for all three plugins; `cargo test --workspace`, clippy, fmt,
  plugin host smoke, and real gateway TUI smoke.
- Performance: input-to-highlight latency, overlay patch frame cost, and no
  editor-frame regression against the temporary reference baseline; typing
  remains responsive while 1000+ node layout runs.

## 8. Exit criteria

- API 1.1.0 is additive, documented, and generated bindings are committed.
- All three plugins work in a real TUI through the control/MCP-capable host.
- Mouse capture, highlight, overlay, and selection limits have tests.
- Unsaved buffer edits update the graph via `workspace.graph_changed` without
  a save; 1000+ node updates coalesce and keep typing responsive.
- Graph owns red diagnostic dots; Alert owns only `ui.alert`/`ui.bell`; no
  cross-plugin overlay mutation exists.
- No new gateway dependency-analysis RPC exists.
- `workspace.open_file` is confined to a trusted root and covered by
  traversal, symlink, and policy tests.
- Diagnostics alerts are coalesced/debounced and use only core alert/bell
  primitives; no arbitrary audio or full-screen flash exists.
- No status/command-line spoofing or unbounded allocation is possible.
- Normal editing, existing multi-cursor behavior, and plugin API 1.0.0 remain
  unchanged.

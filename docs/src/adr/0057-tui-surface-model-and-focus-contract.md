# ADR-0057: TUI Surface Model and Focus Contract

- Status: Accepted; core implemented 2026-11-20
- Version: 1.1.0
- Date: 2026-11-19
- Implementation: surface taxonomy, focus history, per-surface key routing,
  and shared workbench layout
- Related: [ADR-0024](0024-tui-state-and-domain-boundaries.md),
  [ADR-0040](0040-helix-keymap-compatibility.md),
  [ADR-0041](0041-tui-primitive-extraction.md),
  [ADR-0042](0042-selection-first-session-frontend.md)
- Supersedes:
- Superseded by:

## Context

`anvil-tui` now has a broad surface area: start page, settings, help, file
tree, editor tabs, two-pane split, command palette, pickers, completion, hover,
diagnostics, problems, symbols, code actions, debug panel, test explorer,
terminal, notifications, and progress.

The implementation has outgrown its focus model:

- `anvil_session::ui::FocusState` contains only `tree` and `split` booleans.
- `OverlayKind` in `crates/anvil-tui/src/state.rs` contains sixteen variants;
  `Settings`, `Hover`, `Problems`, and `Completion` all use
  `KeymapContext::Popup`.
- `OverlayState` stores each surface in a separate `Option` field.
- `splits.rs` is explicitly a minimal two-pane vertical split.
- Popup geometry is partly computed in `render.rs`, while mouse and editor
  layout have their own constants.
- The status/message band combines status text, notifications, busy labels,
  and progress.

This creates three recurring risks:

1. adding a surface requires coordinated edits to state, key routing, render,
   mouse hit-testing, and tests;
2. `Esc`, `Tab`, and focus restoration have different meanings depending on
   which ad-hoc field owns the surface;
3. renderer and pointer hit-testing can drift because they do not consume one
   layout result.

`docs/src/0.5.x-plan.md` already specified a single Focus and OverlayStack, with
Layout as a pure function. The current code has not fully converged on that
contract.

## Decision

### 1. Classify every UI region as one of five surface types

| Surface | Examples | Lifetime | Focusable |
|---|---|---|---|
| **Page** | Start, Settings, Help | Full-screen route with a back stack | Yes |
| **Dock** | Tree, Terminal, Problems, Tests, Debug | Persistent, collapsible, resizable | Yes |
| **Editor** | Buffer tabs and the existing two-pane split | Primary workspace | Yes |
| **Overlay** | Palette, fuzzy picker, hover, completion, code actions | Transient, traps input, restores prior focus | Yes |
| **Activity** | Status strip, notifications, progress | Non-focusable summary | No |

The **Activity strip** is not an input surface. If an activity offers
`cancel`, `retry`, or details, activating it opens a focusable **Activity
overlay**; those actions are also reachable through the command palette. A
notification or progress row must never require a mouse-only click.

The classification is semantic, not a requirement to build every Dock in this
ADR. Existing surfaces migrate one at a time; new surfaces must declare their
class instead of adding another ad-hoc `Option` field.

### 2. One concrete focus stack, one semantic projection

`anvil-tui` owns a concrete `SurfaceId` and a bounded `FocusStack`.

```text
SurfaceId
  -> SurfaceClass
  -> SurfaceSpec { keymap context, close action, restore policy }
```

Invariants:

- exactly one focusable surface is on top;
- a focusable `SurfaceId` appears at most once in the stack;
- opening an Overlay records the surface below it;
- closing an Overlay restores that exact `SurfaceId`, not merely the old
  `tree`/`split` booleans;
- if the recorded surface no longer exists, focus falls back to the active
  Editor;
- replacing the active Editor or Dock updates the stack instead of leaving a
  stale identity.

`anvil_session::ui::FocusState` remains a frontend-neutral projection of the
top surface for session consumers. It must not become a second authority for
TUI focus. The TUI updates the projection whenever the stack top changes.

### 3. Key routing is declared per surface

Every surface declares its keymap context. The blanket
`OverlayKind::keymap_context() -> Popup` mapping is removed.

Rules:

- one focused surface receives input first;
- global bindings are the fallback, not a parallel unguarded path;
- `Tab` and `Shift-Tab` cycle focusable surfaces only as a fallback in Normal
  mode after the focused surface and input mode decline the key. Existing
  local bindings win: completion accept, command-line completion, start-page
  focus movement, and settings/debug cycling must not regress;
- `Esc` closes the top transient surface or cancels the current temporary
  input mode; it never closes an editor with unsaved changes;
- `Page` uses an explicit back action and retains its back stack;
- `Ctrl-w` continues to own split navigation; it is not reused for surface
  cycling;
- pointer input may change focus but must not be the only way to invoke an
  activity action.

New keymap contexts require actual bindings and tests. Adding an enum variant
without a distinct input contract is not allowed.

### 4. One layout function feeds render and hit-testing

A pure layout function returns a keyed surface map:

```text
layout(terminal_size, surfaces, focus, persisted ratios)
  -> SurfaceRects { editor: [...], tree: Option<Rect>, dock: Option<Rect>,
                    overlays: [...], activity: Rect, status: Rect }
```

Both rendering and mouse hit-testing consume the same map. No renderer-local
popup rectangle and no separate mouse geometry.

Layout rules:

- the Editor is the only guaranteed work area;
- at narrow widths, collapse in this order: Dock, then Tree, then optional
  status segments; the Editor keeps a documented minimum;
- an Overlay that does not fit becomes a full-width sheet instead of an
  off-screen or zero-area popup;
- zero-size or unavailable surfaces are omitted from the map, not represented
  as empty rectangles that still capture input;
- the existing `40`, `80`, `120`, and `200` column cases are first-class test
  sizes.

This ADR does not add recursive pane trees or horizontal editor splits. The
existing two-pane vertical split remains the editor layout for this slice.

### 5. Overlay state gets one registry, not more fields

New Overlay or Page state is registered through one `SurfaceInstance` map
keyed by `SurfaceId`. Existing `OverlayState` fields may be wrapped during
migration; no new top-level `Option<SomePanelState>` field is added for a new
surface.

The migration is a strangler refactor:

1. wrap existing fields in surface entries;
2. route keys, focus, render, and mouse through the registry;
3. remove the old direct field accesses one surface at a time.

A big-bang rewrite of every overlay is explicitly rejected.

### 6. Persistence follows surface type

Session restore preserves:

- Editor buffers, cursors, active tab, two-pane split, and split ratio;
- Tree visibility and width;
- Dock visibility, active panel, and size.

Session restore does not preserve:

- transient Overlays;
- the Activity overlay;
- Page back stacks or a dismissed start page as a normal overlay.

Start-page and session restoration continue to use the existing restore path.

### 7. Accessibility is a surface invariant

- Every focusable surface shows a focus indicator that is not color-only.
- Every pointer action has a keyboard equivalent.
- `NO_COLOR`, ASCII icon fallback, reduced motion, and high-contrast themes
  remain supported.
- Activity actions are reachable from the keyboard through the Activity
  overlay or command palette.
- A full-screen terminal UI must not claim screen-reader accessibility it has
  not tested; automation continues to use the CLI/control surfaces.

## Consequences

Positive:

- One place owns focus, surface identity, and restore behavior.
- New panels no longer require a new ad-hoc state field and duplicated key
  routing.
- Renderer and mouse hit-testing share one geometry contract.
- Activity actions become keyboard-reachable without making the status strip
  focusable.
- The design can add docks or multi-pane layouts later without changing the
  focus contract.

Negative:

- Migrating existing overlay state is a cross-cutting refactor.
- The session `FocusState` remains a projection that must stay synchronized.
- Surface classification adds upfront design work for every new panel.
- The Activity overlay is additional UI rather than a purely passive strip.

## Verification

- Focus tests:
  - open and close every Overlay; focus returns to the exact prior surface;
  - prior surface disappears; focus falls back to the active Editor;
  - no duplicate focusable surface in the stack;
  - `Tab`/`Shift-Tab` cycle order is stable.
- Key routing tests:
  - `Esc` closes only the intended surface;
  - dirty editors are never closed by `Esc`;
  - `Ctrl-w` remains split-only;
  - completion, command-line, start-page, and settings/debug consume `Tab`
    locally before surface cycling;
  - surface cycling by `Tab` occurs only in Normal mode with no local binding;
  - activity cancel/retry works from the keyboard.
- Layout tests:
  - `40x15`, `80x24`, `120x40`, and `200x60`;
  - no overlapping focusable regions and no zero-area input targets;
  - render and mouse hit-testing agree on every surface boundary;
  - overlays switch to full-width sheets at narrow sizes.
- Accessibility tests:
  - focus is visible without color;
  - `NO_COLOR` and ASCII fallback pass render tests;
  - persistent surfaces remain keyboard reachable.
- Real surface smoke:
  - launch the TUI, resize through each test size, open Settings, Help,
    Problems, Completion, and Activity actions, and verify focus recovery.

## References

- WCAG 2.2 SC 2.4.7 Focus Visible:
  <https://w3c.github.io/wcag/understanding/focus-visible> — every focusable
  surface needs a visible focus indicator.
- FreeBSD accessibility development guidance:
  <https://docs.freebsd.org/en/books/accessibility/development/> — color must
  not be the sole information channel and `NO_COLOR` should be supported.
- Ratatui layout constraints:
  <https://ratatui.rs/concepts/layout/> — nested layouts and
  `Min`/`Percentage`/`Ratio`/`Fill` constraints for one deterministic geometry
  model.
- Helix pickers:
  <https://docs.helix-editor.com/master/pickers.html> — pickers have their own
  keymap and consistent query semantics.
- Helix window mode:
  <https://docs.helix-editor.com/master/keymap.html> — window/split actions
  are a distinct sub-mode rather than scattered global bindings.
- VS Code command palette guidance:
  <https://code.visualstudio.com/api/ux-guidelines/command-palette> — clear
  command names, categories, and shortcuts for discoverability.

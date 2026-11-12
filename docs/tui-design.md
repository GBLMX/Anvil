# TUI Design System

Status: applied to the start page, settings/debug overlays, breadcrumbs, and
status chrome. This is a working visual contract, not a skin.

## References

The texture is a synthesis, not a copy:

- **omp**: dense status information, restrained chrome, one accent for focus.
- **Yazi**: strong pane boundaries, clear selected rows, file identity first.
- **Helix**: minimal editor chrome, keyboard-first hints, low-noise palette.
- **herdr**: workspace-as-context, explicit transient panels, persistent hints.

Anvil keeps its own palette and layout. Reference behavior is limited to
hierarchy, density, focus, and interaction rhythm.

## Palettes

Built-in modern palettes now include:

| Theme | Reference |
|---|---|
| `tokyonight` | shared Yazi/Helix Tokyo Night palette |
| `omp-kde` | omp KDE CachyOS palette |
| `helix-base16` | Helix base16 default dark, adjusted for AA |

All built-ins pass the existing WCAG-AA contrast test.

## Tokens

| Token | Contract |
|---|---|
| canvas | deepest surface; editor text sits directly on it |
| panel | one step above canvas for tree, overlays, start page |
| elevated | selected/current surface; never used for decoration |
| border | quiet 1px rounded border; accent only on focus |
| text | primary reading color |
| muted | labels, secondary paths, hints |
| dim | section labels and disabled affordances |
| accent | focus, key hints, links, active mode |
| success/warning/error | semantic only; never general decoration |

No shadows, gradients, glow, or non-portable transparency.

## Layout

- Editor is the primary surface. Chrome must not compete with text.
- Sidebar width: `clamp(width / 5, 18, 40)`, only when the editor remains
  usable.
- Split panes share one gutter model; wheel routing follows the pane under the
  pointer.
- Overlays are centered, bounded, and use the same rounded panel primitive.
- Start page:
  - wide (`>= 68` columns): two columns, `Start` and `Recent`
  - narrow: one column, actions first, recent files second
  - recent files are limited and must be openable in the current trusted roots

## Typography in a terminal

- Uppercase is reserved for short section labels (`FILES`, `WORKSPACES`).
- Keys are bold accent text with fixed-width alignment; descriptions are
  normal text.
- Paths and metadata are muted; selected rows are the only inverted surface.
- Avoid ASCII art and decorative separators. Use whitespace for grouping.

## Interaction

- Keyboard first; every pointer action has a keyboard equivalent.
- Start page hints: `r` recent files, `Space f` find, `Space e` tree, `?`
  help, `:q` quit.
- Breadcrumbs are interactive: path reveals the tree; symbol opens symbols.
- Overlays show their key contract in the footer, not in a manual.
- `reduced_motion = true` freezes spinners and avoids continuous repaint.
- `high_contrast = true` raises text, muted, dim, and border contrast while
  preserving the selected palette.

## Feature pages

Current overlays are intentionally task-scoped:

```text
Start · Help · Settings · Problems · Tests · Debug
Symbols · References · Calls · Code actions · Completion
Terminal · Plugins · Packages
```

A page earns a full surface only if it has persistent navigation or multiple
related workflows. Otherwise it remains a bounded overlay.

## Future slices

1. Settings are grouped into Editor / Search / LSP / Accessibility.
2. Start page supports mouse selection for recent entries.
3. Settings and debug overlays use the focused border token; remaining overlays
   still need the same treatment.
4. Theme preview page using the same panel/selected primitives.
5. Optional animation only where it communicates transition, never for
   decoration.

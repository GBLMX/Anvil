# IDE Capability Plan

Status: P0 mostly landed; P1 is planned. Search/replace, problems, quick-open
preview, and icon themes are owned by `0.5.4`/`0.5.7`/`0.5.8`; this plan does
not re-schedule them.

Scope: editor-level capabilities that make Anvil usable for daily software work
without turning the kernel into a full IDE. Anvil remains Linux-first, with no
Tokio runtime, no in-process plugin UI, and no embedded WebView.

## 1. Boundaries

- `anvil-session` owns documents, commands, focus/overlay state, effects, and
  lifecycle.
- `anvil-editor` owns text, selection, undo/redo, and view projections.
- `anvil-tui` is the input adapter and renderer.
- LSP, Git, tasks, terminals, and debuggers are external capabilities reached
  through typed effects and the gateway/adapters.
- A feature that needs a second event loop, in-process plugin execution, or
  remote marketplace authority needs an ADR first.

## 2. P0: required before calling the editor complete

| Area | Current | Required |
|---|---|---|
| Completion | snippet mirrors, ranking, resolve docs, placeholders landed | — |
| Navigation | definition, hover, symbols, references, highlights, incoming/outgoing calls landed | — |
| Structure | diagnostics, code actions, format, rename, folding ranges, semantic tokens, code lens landed | — |
| Search | literal `/`, `n`/`N`, search/replace UI landed | multi-file/project search only if measured |
| Problems | bounded panel and jumps landed | — |
| Session | buffer/cursor/layout restore and crash backup landed | — |
| Git | diff, hunk staging, blame, conflict previews landed | conflict resolution operations |
| Settings | typed registry and overlay landed | validation, reset, changed markers, hot-reload boundary |

Remaining P0: project search, Git conflict resolution, and settings validation/
reset/hot-reload boundary. Each item needs a deterministic failure test.

## 3. P1: IDE-like capability

- Task runner with problem matchers and bounded output.
- Test explorer with jump-to-failure.
- Debug Adapter Protocol client in a separate process.
- Terminal panel with explicit workspace and trust boundaries.
- Multi-root workspaces with per-root gateway/trust policy.
- Code folding with session persistence.
- Auto-save policy remains; swap files and external-conflict recovery landed.

These are separate adapters, not `anvil-session` monoliths. Quick-open preview
and icon theme are not repeated here; they are `0.5.4`.

## 4. P2: ecosystem and collaboration

- Remote/SSH development.
- Collaboration and review integration.
- Signed plugin marketplace and installation UI.
- Profiling, coverage, and performance panels.
- Notebook/WebView surfaces.

Not required for v1.

## 5. Non-goals

- Embedded browser or WebView editor.
- In-process plugin UI or Lua on the keystroke path.
- Default animation, glow, shadow, or non-portable transparency.
- A second async runtime or event loop.
- A giant decorative dashboard as the main surface.
- Copying LazyVim/Yazi/Helix keybindings or visual skins.

## 6. Sequence

1. Finish `0.5.3`–`0.5.9`: palette/which-key, preview/icons, status/tabs,
   notifications/problems, search/replace, mouse/accessibility.
2. Remaining LSP depth: completion snippet mirrors/ranking, then code lens
   with above-line virtual rows.
3. Session restore, crash backup, persistent undo.
4. Git diff/hunk/blame/conflict.
5. Settings and remaining theme/keymap boundaries.
6. P1 adapters: tasks, terminal, debugger.
7. P2 only after package signing and the `0.6.x` distribution boundary.

## 7. Gates

- No new runtime dependency without an ADR and measured need.
- LSP results carry path, request id, revision, and generation; stale results
  never overwrite newer state.
- Read-only buffers never mutate on search/replace or LSP edits.
- Large-file and binary preview paths stay bounded.
- Session restore must never silently discard unsaved/modified buffers.
- Code lens rows are capped per source line and viewport, truncate with a
  visible marker, and never render on folded or hidden lines.
- Every P0 item needs tests for failure, cancellation, and narrow terminal
  layouts where applicable.

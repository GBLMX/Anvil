# IDE Capability Plan

Status: P0 and P1 landed. Task runner, test failure explorer, DAP client, PTY
terminal panel, multi-root routing/gateway lifecycle, and folding persistence
are implemented. Cross-root tree/search is tracked separately in
[multi-root-routing-plan.md](multi-root-routing-plan.md). Search/replace,
problems, quick-open preview, and icon themes are owned by
`0.5.4`/`0.5.7`/`0.5.8`; this plan does not re-schedule them.

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
| Search | literal `/`, `n`/`N`, search/replace, and bounded project search landed | — |
| Problems | bounded panel and jumps landed | — |
| Session | buffer/cursor/layout restore and crash backup landed | — |
| Git | diff, hunk staging, blame, conflict previews, and resolution operations landed | — |
| Settings | typed registry, overlay, validation, reset, changed markers, and explicit `:config-reload` landed | file watching is out of scope |

Remaining P0: none for the internal-polish phase. Cross-root file tree/search
remains the P1 follow-up tracked in
[multi-root-routing-plan.md](multi-root-routing-plan.md).

## 3. P1: IDE-like capability

- Task runner with problem matchers and bounded output: landed in `:task` and
  `:task-output`, with background timeout and problems-panel integration.
- Test explorer with jump-to-failure: `:tests <command>` runs through the task
  runner and opens located failures in a selectable overlay.
- Debug Adapter Protocol client in a separate process: `dap::DapClient` owns
  framed stdio, initialize/launch/breakpoints/stepping, responses, and events.
- Terminal panel with explicit workspace and trust boundaries: PTY session,
  Admin-gated `:terminal*` controls, interactive bottom panel with key/paste
  routing, and process-group cleanup landed.
- Multi-root workspaces with per-root gateway/trust policy: routing/trust core,
  trust-gated per-root gateways, `:workspace-*` commands, and owning-root
  open/save routing landed. Cross-root tree/search UX remains a follow-up.
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

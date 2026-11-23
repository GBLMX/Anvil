# ADR-0073: TUI Motion and Transition Boundary

- Status: Accepted; command palette enter fade implemented
- Version: 1.0.0
- Date: 2026-11-23
- Related: [ADR-0016](0016-render-style-layers.md),
  [ADR-0068](0068-tui-visual-language-and-markers.md),
  [ADR-0071](0071-tui-terminal-capability-and-frame-delivery.md),
  [0.5.x plan](../0.5.x-plan.md), [TUI design](../tui-design.md)
- Supersedes:
- Superseded by:

## Context

The TUI has one existing motion source: the busy spinner. It is already gated by
`ui.reduced_motion`, and the event loop only redraws on dirty state. The UI
style contract also says animation may communicate a transition, never decorate
content, and `reduced_motion` must freeze continuous repaint.

The deferred effects section of the 0.5.x plan requires an ADR before adding
animation: dependency category, duration/frame budget, reduced-motion,
`NO_COLOR`, tmux fallbacks, version compatibility, zero idle redraw, and no
impact on input, cursor, scrolling, or LSP paths.

External research points at post-render effects: widgets render a deterministic
frame, then an effect pass transforms cells using elapsed time. TachyonFX is a
mature implementation of that model, but it is a sizeable dependency with many
breaking releases. The first Anvil need is not an effect platform; it is a
small set of bounded transitions.

The performance baseline remains authoritative: editor first frame
`<= 10 ms`, 100 MB scroll frame p99 `<= 33 ms`, and no full-document layout per
frame. Animation work must not change those definitions.

## Decision

### 1. Motion is a post-render transition layer

`anvil-tui` owns motion state. Widgets and state updates remain deterministic;
an effect pass may transform the already-rendered buffer for a bounded target
rectangle.

Motion never reaches `anvil-core`, `anvil-editor`, `anvil-session`, or
`anvil-runtime`. Plugins cannot create, cancel, or render motion.

### 2. The first implementation is local and minimal

The initial implementation uses a small in-tree transition engine in
`anvil-tui`. It supports only the transitions Anvil actually needs:

- overlay enter/exit;
- notification enter/exit;
- progress completion.

The first delivery is an overlay transition. Text, cursor, gutter, diagnostic
underlines, scrolling, and LSP results are not animated.

TachyonFX is not added as a default dependency. If composition or effect
variety later justifies it, it is introduced only as a non-default `effects`
feature with a pinned version and an ADR revision covering dependency cost and
compatibility. A TachyonFX dependency must not be added merely because the
first fade is easier to write with it.

### 3. Motion is event-driven, not a second loop

There is no fixed 60 FPS loop. The existing event loop stays the only loop.

- An active transition sets a bounded next-poll deadline of at most 16 ms.
- When no transition remains, the existing event polling and zero-idle-redraw
  behavior resume.
- Effect completion requests one final deterministic repaint and removes the
  transition.
- Input remains responsive during motion; `Esc` and navigation are not queued
  behind animation completion.

Effect state has a stable target key, so a repeated transition replaces or
cancels the existing one rather than accumulating managers.

### 4. Bounded duration and area

| Transition | Budget |
|---|---:|
| Overlay enter/exit | 100–140 ms |
| Notification enter/exit | 120–160 ms |
| Progress completion | 150–200 ms |

- No transition exceeds 200 ms.
- At most two transitions are active at once.
- A transition is clipped to its target rectangle; no full-screen effect pass
  for a small popup.
- No infinite, ambient, or text-content effects.

### 5. Accessibility and terminal fallback

- `ui.reduced_motion = true` skips transition creation and applies the final
  visual state immediately.
- `NO_COLOR` disables color-only transitions. Positional transitions may remain
  only when reduced motion is off.
- `TERM=dumb` disables motion.
- When the `TerminalProfile` reports synchronized output unavailable, avoid
  color fades that can visibly flicker; keep the static final frame.
- No transition may be the only signal for loading, error, or completion.
- tmux or other multiplexers follow the same profile/fallback behavior; they do
  not get a separate animation path.

### 6. Performance and lifecycle verification

- First-frame and scroll gates in `docs/performance.md` remain unchanged.
- A motion tick processes only the target area and only while active.
- Completion leaves no tinted cells, no stale buffer, and no scheduled redraw.
- Active motion during input must not change key-to-screen behavior or editor
  state.
- Reduced-motion tests assert that no transition is created and no extra idle
  frame occurs.
- A PTY smoke covers one enter and one exit transition with synchronized output
  enabled and disabled.

## Consequences

Positive:

- The first animation is small enough to review and benchmark.
- No new dependency or effect DSL enters the default build.
- Existing input, scrolling, LSP, and first-frame budgets stay isolated from
  animation.
- A later TachyonFX adoption has an explicit feature gate and compatibility
  decision.

Negative:

- The local transition engine will initially cover fewer effects than
  TachyonFX.
- A second animation shape requires extending the local engine until the
  TachyonFX gate is met.
- Motion state adds one more owner to the existing TUI loop and needs lifecycle
  tests.

## Implementation status

2026-11-23: the local engine is in `anvil-tui/src/motion.rs`. The command
palette fades in over 120 ms behind synchronized output. It is disabled by
`reduced_motion`, `NO_COLOR`, `TERM=dumb`, or unavailable synchronized output.
Closing remains immediate; exit transitions need a buffered previous frame and
are not part of this delivery.


## Verification

- Overlay fade completes, requests one final repaint, and leaves zero active
  transitions.
- A second transition with the same target replaces, rather than duplicates,
  the first.
- `reduced_motion = true` applies the final state immediately and creates no
  animation tick.
- `NO_COLOR`, `TERM=dumb`, and synchronized-output-unavailable paths produce a
  static final frame.
- Input and `Esc` remain responsive during a transition.
- No editor text, cursor, scroll, gutter, diagnostic underline, or LSP result
  is transformed by motion.
- Frame timing and first-frame gates pass on the reference machine.
- `cargo test --workspace`, clippy, fmt, architecture checks, and
  `git diff --check` pass.

## References

- [TachyonFX README](https://github.com/ratatui/tachyonfx): post-render
  stateful effects, composition, spatial patterns, and dependency scope.
- [TachyonFX EffectManager](https://docs.rs/tachyonfx/0.25.2/tachyonfx/struct.EffectManager.html):
  effect lifecycle, replacement, cancellation, and per-frame processing.
- [Effect Workflow tutorial](https://github.com/pyratatui/pyratatui/blob/main/docs/tutorial/effect_workflow.md):
  create → add to manager → process per frame → remove after completion.
- [CodeWhale Motion Contract](https://github.com/codewhale-hq/codewhale/blob/main/docs/MOTION_CONTRACT.md):
  explicit full/reduced/off motion modes and separation of decorative motion
  from status feedback.
- [Anvil performance baseline](../performance.md): first-frame and scroll gates.
- [0.5.x deferred effects](../0.5.x-plan.md): existing feature-gate and fallback
  requirements.

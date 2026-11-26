# ADR-0071: TUI Terminal Capability and Frame Delivery Upgrade

- Status: Accepted; terminal profile, synchronized draw, keyboard normalization, and wide-character truncation implemented 2026-11-23
- Version: 1.3.0
- Date: 2026-11-23
- Related: [ADR-0019](0019-terminal-keyboard-enhancement.md),
  [ADR-0057](0057-tui-surface-model-and-focus-contract.md),
  [ADR-0068](0068-tui-visual-language-and-markers.md),
  [Performance baseline](../performance.md)
- Supersedes:
- Superseded by:

## Context

The TUI is already on the current terminal stack:

- `ratatui` 0.30.2 and `crossterm` 0.29 in `Cargo.lock`;
- `anvil-tui-kit::TerminalSession` owns raw mode, the alternate screen, mouse,
  focus, bracketed paste, cursor shape, and terminal cleanup;
- keyboard enhancement requests `DISAMBIGUATE_ESCAPE_CODES` and
  `REPORT_EVENT_TYPES`;
- the event loop redraws only when `model.file.dirty` is set;
- the TUI has no synchronized output wrapper, no single terminal-capability
  owner, and no terminal-protocol tests beyond the existing event/state tests.

The remaining gap is not a crate-version upgrade. It is the missing boundary
between terminal capabilities, frame delivery, and the render loop. Without
that boundary:

- a new terminal mode can be requested in one path and forgotten in cleanup;
- unsupported modes cannot degrade predictably;
- frame tearing and partial updates cannot be diagnosed separately from
  application rendering;
- adding a second backend or an image protocol later would spread capability
  checks across the terminal lifecycle and renderer.

The Ratatui 0.30 line also changes the available primitives. Border merging
arrived in 0.30.0, 0.30.1 added more precise buffer diff options, and 0.30.2
fixed wide-cell cleanup and added the Termina backend. The current dependency
already includes those fixes; adopting them is a UI decision, not a version
bump. Ratatui 0.30.0 documents border merging; 0.30.2 documents the wide-cell
cleanup and Termina backend in its release highlights.

## Decision

### 1. One terminal capability profile

`anvil-tui-kit` owns a `TerminalProfile` for each session. The profile records
the terminal features the session requested and successfully enabled:

| Field | Meaning |
|---|---|
| `keyboard_enhancement` | disambiguated keys and event kinds are active |
| `synchronized_output` | DECSET 2026 frame batching is active |
| `focus_events` | focus gained/lost reporting is active |
| `mouse_capture` | pointer events are active |
| `bracketed_paste` | paste events are active |
| `truecolor` | the session may use 24-bit color |

Capabilities are detected once at startup, not queried per frame. An explicit
environment/configuration override may disable a capability for a terminal or
multiplexer that accepts an escape sequence but renders it incorrectly.

No second capability registry is introduced in `anvil-tui`. Render code reads
the profile through `TerminalSession`.

### 2. One frame-delivery entry point

The event loop calls `TerminalSession::draw(|frame| view(&model, frame))`
instead of calling `terminal.draw` directly. The helper owns:

- optional synchronized-output begin/end;
- the Ratatui draw and flush;
- frame timing instrumentation;
- best-effort end/cleanup when a draw fails.

Synchronized output is emitted only when the profile says it is active.
`ANVIL_SYNCHRONIZED_OUTPUT=0` disables it before startup for terminals or
multiplexers that accept the sequence but render it incorrectly.

### 3. Synchronized output is the first frame upgrade

When enabled, each frame is wrapped in the terminal's synchronized update
sequence. Unsupported terminals retain the existing draw path. A test writer
must assert begin/draw/end ordering and the failure cleanup path. A PTY smoke
must verify startup, resize, exit, and panic/drop cleanup.

The existing frame budget remains the gate from
[the performance baseline](../performance.md): editor first frame `<= 10 ms`,
100 MB scroll frame p99 `<= 33 ms`, and no full-document layout per frame.
This ADR does not relax those gates or replace their measurement definitions.

### 4. Keyboard enhancement is extended only with evidence

The existing disambiguation and event-type flags remain. The TUI does not
enable `REPORT_ALL_KEYS_AS_ESCAPE_CODES`: that changes text delivery and risks
breaking IME and plain-text input. `REPORT_ALTERNATE_KEYS` may be enabled only
behind a profile flag with PTY tests for layout-dependent shortcuts.

Release events remain non-editing events unless a future ADR defines a
specific gesture. Repeat handling keeps its current per-mode semantics.

### 5. Use the existing Ratatui line; do not migrate the backend

The current `CrosstermBackend` remains. The TUI adopts only maintained,
user-visible primitives from the current Ratatui line:

- adjacent border merging for split panes where the shared panel primitive
  currently draws duplicate edges;
- wide-cell cleanup coverage for CJK, emoji, and combining-character
  replacement;
- precise buffer diff options only if a future cell contains escape-bearing
  content whose display width differs from its stored symbol.

Block shadows, animation, and decorative effects are not adopted. The Termina
backend is not selected by this ADR; it needs a separate evaluation against
Crossterm on input handling, terminal coverage, startup cost, and cleanup.

### 6. Native terminal integrations remain separate decisions

Image protocols, OSC 8 hyperlinks, and OSC 52 clipboard writes are not part of
this upgrade. Image protocols remain outside the default editor surface; OSC 8
and OSC 52 require their own security and fallback decision. Plugins still may
not emit arbitrary terminal escape sequences.

## Consequences

Positive:

- Terminal modes have one owner and one cleanup path.
- Synchronized output can remove visible tearing without changing the renderer.
- Unsupported terminals degrade predictably instead of depending on ad hoc
  escape-sequence behavior.
- Future backend or image work starts from a capability boundary rather than
  more terminal-global checks.

Negative:

- `TerminalOptions` and session startup grow one capability profile.
- Synchronized output adds an implementation path and tests that a headless
  `TestBackend` cannot fully validate.
- Border merging changes visual output in split-pane tests.
- The backend remains Crossterm even though Ratatui now offers Termina.

## Verification

- A test writer observes synchronized begin/end around one frame and no end
  command when begin fails.
- PTY smoke: startup, resize, quit, and drop/panic cleanup leave the terminal in
  the original mode.
- Capability override tests cover disabled synchronized output, keyboard
  enhancement, mouse, focus, and paste paths.
- Frame timing records first frame and p99 without changing the gates in
  `docs/src/performance.md`.
- Existing keyboard tests still reject release events as editing actions and
  preserve paste/focus behavior.
- Wide-character tests cover CJK and emoji replacement without stale cells.
- Split-pane rendering tests cover merged shared borders.
- `cargo test --workspace`, clippy, fmt, architecture checks, and
  `git diff --check` pass.

## References

- [Ratatui 0.30.0 release highlights](https://ratatui.rs/highlights/v030/):
  current modularization, execution API, widget/layout changes, and block
  border merging.
- [Ratatui 0.30.1 release highlights](https://ratatui.rs/highlights/v0301/):
  buffer diff options, buffer application API, and diff allocation changes.
- [Ratatui 0.30.2 release highlights](https://ratatui.rs/highlights/v0302/):
  Termina backend, wide-cell cleanup, and current patch-level behavior.
- [Kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/):
  disambiguation, event types, alternate keys, and the risk of reporting all
  keys as escape codes.
- [Synchronized output (DECSET 2026) terminal support](https://www.terminfo.dev/modes/decset-2026-synchronized-output):
  begin/end frame semantics and terminal support.
- [Crossterm synchronized update command](https://docs.rs/crossterm/0.29.0/crossterm/terminal/struct.BeginSynchronizedUpdate.html):
  the available lifecycle primitive.
- [Anvil performance baseline](../performance.md): definitions and gates.

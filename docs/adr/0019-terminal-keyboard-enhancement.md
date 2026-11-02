# ADR-0019: Terminal Keyboard Enhancement and Key Dispatch

- Status: Accepted
- Date: 2026-11-02
- Related: [ADR-0006](0006-upstream-dependencies.md), [ADR-0008](0008-selection-first-interaction.md), [ADR-0015](0015-command-line-completion.md)

## Context

The TUI receives keys through `crossterm` in the terminal's legacy mode. That
path cannot reliably distinguish `Alt+letter` from `Esc` followed by a letter,
`Ctrl+Shift+letter` from `Ctrl+letter`, or a held key from repeated presses.
Terminals also disagree on whether `Alt+C` is reported as `Char('C')` or
`Char('c')`, so per-binding case variants do not scale.

The terminal may not implement the kitty keyboard protocol at all. Sending
`PushKeyboardEnhancementFlags` unconditionally would leave unsupported terminals
with an unparsed `CSI >...u` sequence, and failing to pop an accepted push would
leak terminal state across editor runs.

`REPORT_EVENT_TYPES` changes the event stream from presses to press, repeat, and
release events. Ignoring release events is correct for actions, but ignoring
repeat events would break autorepeat when the protocol is enabled.

## Decision

### 1. Capability-gated keyboard enhancement

`TerminalSession` queries `supports_keyboard_enhancement()` once during startup.
Only a successful `true` result enables the protocol. `false` and query errors
both keep the legacy path.

When enabled, the session pushes:

- `KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES`
- `KeyboardEnhancementFlags::REPORT_EVENT_TYPES`

The session records whether the push itself succeeded, not merely whether the
terminal claimed support. A failed push keeps the legacy path.

### 2. Balanced terminal restoration

The pop is sent only when the push succeeded. It is sent:

- from `Drop` after normal operation;
- explicitly during startup failure if the push succeeded before
  `Terminal::new` failed.

The restoration order is: pop keyboard flags, restore cursor shape, leave the
alternate screen, disable raw mode, show the cursor.

### 3. Unified key handling

`KeyPress` wraps `KeyEvent` and exposes the modifier state used by dispatch:
`ctrl`, `alt`, `super_key`, and `command_char`.

`command_char` lowercases Alt-modified ASCII letters for command matching, so
future Alt bindings use one lowercase spelling. Text input continues to use the
original character.

Terminal normalization lives in `input.rs`; normal and operator-pending binding
tables live in `keymap.rs`; `keys.rs` only dispatches resolved commands into the
model.

`Press` and `Repeat` events dispatch normally. `Release` events are ignored and
do not count as syntax activity.

### 4. Reject modified characters in plain contexts

Insert, command-line, search, rename, create, transfer, fuzzy, and completion
input accept plain or shifted characters. Ctrl, Alt, and Super modified
characters do not become text.

Pending sequences, register selection, and normal-mode command bindings use the
same plain-input rule. A modifier during an operator-pending sequence cancels
the pending state instead of being interpreted as its unmodified key.

### 5. Bounded user keymap; plugin keymaps deferred

A TOML user keymap may override normal and operator-pending bindings through
stable command IDs. It is validated atomically and cannot define editing
semantics. ADR-0008 keeps the Rust default keymap as the interim adapter and
reserves Lua/TS plugin keymaps for a later phase.

## Consequences

- Kitty, foot, WezTerm, and other supporting terminals get unambiguous Alt,
  Ctrl+Shift, repeat, and release events.
- Unsupported terminals remain on the existing escape-prefix path and receive
  no keyboard-enhancement sequences.
- A capability query may block or time out once at startup; it never runs in the
  event loop.
- `REPORT_EVENT_TYPES` release events are intentionally ignored; repeat events
  are actions.
- The default keymap remains Rust; user TOML can override normal and
  operator-pending bindings, while plugin keymaps remain deferred.

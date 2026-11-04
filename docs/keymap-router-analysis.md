# Contextual Keymap Router Analysis

Status: analysis only. No router, leader timeout, or preset files are
implemented yet.

Source issue: `[Feature] Implement Contextual Keymap Router and Space-Leader
Ergonomics`, with the accepted ADR-0020 addendum on F-key compatibility.

## Summary

The existing keymap already has a data-driven TOML layer and a fixed,
context-like routing order, but it does not satisfy the proposed ADR:

- contexts are implicit `if` branches, not a `KeymapContext` type;
- configuration exposes only normal and operator-pending modes;
- the leader has no timeout and no configured leader actions;
- F-keys are absent from defaults but also cannot be configured;
- presets are not shipped;
- `anvil-editor` has no keymap or crossterm dependency.

The addendum changes the F-key requirement from "never use F-keys" to "never
use F-keys in defaults, but allow users to bind them." The current parser
rejects F-keys entirely, so this part is still a gap.

## Current implementation

| Area | Current behavior | Evidence |
|---|---|---|
| Key names | Characters, named keys, modifiers; no `f1`-`f12` parsing | `crates/anvil-tui/src/keymap.rs`, `parse_key_name` |
| Config format | `[keymap.normal]` and `[keymap.pending]`; parsed with `toml_edit` | `KeymapConfig::from_toml` |
| Config location | `ANVIL_KEYMAP`, XDG config, or `~/.config/anvil/keymap.toml` | `KeymapConfig::default_path` |
| Command contexts | Only `Normal` and `Pending` | `crates/anvil-session/src/command.rs` |
| Routing | Fixed `if` chain in `handle_key` | `crates/anvil-tui/src/keys.rs` |
| Popup precedence | Fuzzy, symbols, code actions, completion, hover, diagnostics, then tree, then input modes, then normal | `handle_key` |
| Space leader | `Space` becomes `Pending(' ')`; no timeout | `normal_action`, `PendingCommand::Count` handling |
| Default Space actions | `Space e` opens tree; `Space f` opens fuzzy picker | `pending_action` |
| F-key defaults | None | `normal_action`, `pending_action` |
| F-key user bindings | Unsupported | `parse_key_name` rejects multi-character names |
| Presets | No `anvil-default.toml`, `anvil-vim.toml`, or `anvil-ergo.toml` | repository |
| ADR file | No `docs/adr/0020-*` file exists | `docs/adr` |

## Requirement gaps

| Requirement | Status | Notes |
|---|---|---|
| `KeymapContext` enum in `anvil-editor` | Missing | `anvil-editor` currently depends only on `anvil-api` and `anvil-core`; adding crossterm/UI contexts there would cross the kernel boundary in ADR-0013. |
| Router by specificity | Partial | Behavior exists as an ordered `if` chain, but is not data-driven or testable as one priority list. |
| Popup consumes `j` first | Partial | Popup-specific branches run before editor normal mode, but pending/leader and macro states run before popups. |
| Space leader timeout parser | Missing | A pending key waits indefinitely; there is no `Instant` deadline or leader expiry. |
| Externalized config | Partial | TOML is externalized, but only normal/pending; uses `toml_edit`, not serde. |
| F-key compatibility | Missing | No F-key parser support and no normalization tests. |
| No F-key defaults | Already true | No F1-F12 default binding exists. |
| Ergo default preset | Missing | Current defaults are Vim-like and include `Space e`/`Space f`. |
| Vim preset | Missing | No shipped preset file. |
| ADR-0020 acceptance tests in `anvil-editor` | Blocked | The router lives in `anvil-tui`; `anvil-editor` has no key event type. |

## Conflicts and decisions needed

### 1. Where should `KeymapContext` live?

The issue says `anvil-editor`. The current architecture puts key events in
`anvil-tui` and stable command IDs in `anvil-session`. `anvil-editor` is the
kernel and has no UI or crossterm dependency.

Recommended split:

- `anvil-session`: stable `KeymapContext` identifiers, command IDs, and
  context metadata. No crossterm types.
- `anvil-tui`: crossterm `KeyEvent` normalization, chord parsing, leader
  timeout, and the actual router.
- `anvil-editor`: unchanged.

If `KeymapContext` must be in `anvil-editor`, keep it as a dependency-free enum
and keep crossterm out of that crate. The actual router should still be tested
in `anvil-tui`, not `anvil-editor`.

### 2. Space mapping conflict

The issue acceptance says:

- `Space f` opens the FileTree;
- `Space e` focuses the Editor.

Current behavior and documentation say:

- `Space e` opens the FileTree;
- `Space f` opens the fuzzy file picker.

The addendum instead describes `Space + 1/2/3` for FocusEditor and
ToggleFileTree. Recommended resolution:

- `Space 1` focus editor;
- `Space 2` toggle/focus file tree;
- `Space 3` toggle split/popup (or leave unbound until defined);
- keep `Space e`/`Space f` as compatibility aliases unless the issue explicitly
  approves a breaking change.

Acceptance criteria should be rewritten before implementation. Otherwise the
implementation will either break existing users or fail the stated acceptance.

### 3. F-key policy

The addendum supersedes the original "No F-Keys" wording:

- defaults must not use F1-F12;
- users may bind `F1`-`F12`;
- terminal escape variants must normalize to `KeyCode::F(n)`.

crossterm already normalizes most terminal F-key sequences into `KeyCode::F`.
Anvil should not write a second raw-escape parser. The required work is:

- extend `parse_key_name` to accept `f1`-`f12` case-insensitively;
- add config tests for F-key bindings;
- verify crossterm mapping in the terminal compatibility matrix;
- keep F-keys out of shipped default/ergo presets.

### 4. Leader timeout

The event loop already polls with a 50 ms timeout:

```rust
if event::poll(Duration::from_millis(50))? {
    handle_event(&mut model, event::read()?);
}
```

That is enough for a leader deadline without a new thread:

- store `leader_started: Option<Instant>` with the pending leader;
- expire it when the next loop tick sees `elapsed() >= LEADER_TIMEOUT`;
- clear pending state and status when it expires;
- choose one timeout (recommended 500-800 ms);
- ensure `Esc` cancels immediately.

The leader state must not run before popup routing. Recommended routing order:

```text
Overlay/Popup > CommandLine/Search/Rename > PendingLeader > FileTree > Editor > Global
```

If leader routing must be first, active overlays must explicitly cancel the
leader on entry, or `Space` will steal keys from an open popup.

### 5. Config schema

The current schema is nested by mode:

```toml
[keymap.normal]
x = "editor.undo"

[keymap.pending.d]
d = "editor.yank_lines"
```

A context-aware schema needs either nested context sections:

```toml
[keymap.contexts.editor]
"ctrl-w" = "view.split_vertical"

[keymap.contexts.file_tree]
j = "cursor.down"

[keymap.contexts.global]
"space 1" = "view.focus_editor"
```

or a sequence table:

```toml
[[keymap]]
context = "editor"
key = "ctrl-w"
action = "view.split_vertical"
```

Recommended: nested context sections plus a top-level `preset` key. Keep
`[keymap.normal]` and `[keymap.pending]` as compatibility aliases for one
release. Unknown contexts, duplicate chords within a context, and commands not
registered for that context must reject the file.

`serde` is already a dependency of `anvil-tui`; `anvil-session` only has
`serde_json`. Parsing can stay in `anvil-tui` unless the router is moved.

### 6. Presets

The issue names `anvil-vim.toml` and `anvil-ergo.toml`; the addendum names
`anvil-default.toml`. Recommended shipped set:

- `anvil-default.toml`: ergo defaults, Space leader, no F-keys;
- `anvil-vim.toml`: current Vim-like normal/pending bindings;
- `anvil-ergo.toml`: explicit alias or the same content as default.

Preset selection precedence:

```text
CLI flag > ANVIL_KEYMAP > user keymap.toml > bundled default
```

Do not silently rewrite a user's existing config.

## Proposed implementation slices

1. **Config and key parsing**
   - Add `f1`-`f12` parsing and tests.
   - Add serde structs for contexts and presets.
   - Keep legacy normal/pending tables.
   - Add clear errors for unknown contexts/actions.

2. **Contexts**
   - Define `KeymapContext` without crossterm in the chosen crate.
   - Add `active_context(model)` in the TUI.
   - Convert the popup/tree/editor `if` chain to an ordered context list.
   - Add a pure routing function tested with a fake handler.

3. **Leader**
   - Add pending leader deadline.
   - Expire on the 50 ms loop tick.
   - Add tests for timeout, `Esc`, and popup precedence.

4. **Defaults and presets**
   - Remove F-keys from all shipped defaults (already true).
   - Add Space+1/2/3 ergo bindings.
   - Preserve current Space+e/f aliases unless the breaking change is approved.
   - Add the Vim preset.

5. **Acceptance tests**
   - `Space 1` focuses editor; `Space 2` toggles/focuses file tree.
   - Popup `j` moves popup selection and does not move the editor cursor.
   - F1-F12 bind only when a user config asks for them.
   - No F1-F12 appear in bundled presets.
   - Leader timeout returns to normal mode.
   - Context priority order is asserted directly.
   - Config rejects wrong-context commands and duplicate chords.

## Risks

- Moving keymap logic into `anvil-editor` would add UI dependencies to the
  kernel and conflict with ADR-0013.
- Changing `Space e`/`Space f` breaks existing muscle memory and documented
  behavior, including the file-tree/fuzzy picker docs.
- F-key normalization varies by terminal; crossterm covers common sequences,
  but the compatibility matrix should record tested terminals.
- A leader timeout can feel laggy or trigger accidental normal-mode actions if
  too short.
- A context router can change which component consumes `Esc`, `j`, `k`, and
  `Enter`; this needs regression coverage for each overlay.
- `serde` in `anvil-session` or `anvil-editor` adds dependencies to crates that
  currently avoid them.

## Recommendation

Do not implement the issue as written yet. First resolve the location of
`KeymapContext`, rewrite the Space acceptance mappings to match the addendum,
and confirm whether current `Space e`/`Space f` aliases stay. The lowest-risk
path is a TUI-local router with context identifiers in `anvil-session`, F-key
parsing only, a deadline-based leader, and serde-backed presets.

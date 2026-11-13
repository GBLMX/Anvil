# Contextual Keymap Router Analysis

Status: partially implemented in `0.5.3`. The context vocabulary, projection,
leader timeout, F-key parsing, Space bindings, which-key, contextual TOML
schema, and bundled presets have landed. Tree/popup/modal context dispatch, the
full `if`-chain migration, the command palette, and operation-scoped modal
layers remain.

Source issue: `[Feature] Implement Contextual Keymap Router and Space-Leader
Ergonomics`, with the accepted ADR-0020 addendum on F-key compatibility.

## Summary

The keymap now has stable `KeymapContext` identifiers and a derived context
stack, but routing is still mostly an ordered `if` chain:

- `KeymapContext` lives in `anvil-session` without crossterm types;
- the TUI projects the active context stack from focus, input mode, and
  overlays;
- configuration accepts normal, pending, and contextual tables through serde;
- `editor` and `global` context bindings dispatch; the other context sections
  validate but are not consumed yet;
- the Space leader has a 600 ms deadline;
- F-keys are absent from defaults but can be bound by users;
- bundled ergo and Vim presets are shipped;
- `anvil-editor` still has no keymap or crossterm dependency.

The addendum changed the F-key requirement from "never use F-keys" to "never
use F-keys in defaults, but allow users to bind them." Both parts are now
implemented.

## Current implementation

| Area | Current behavior | Evidence |
|---|---|---|
| Key names | Characters, named keys, modifiers, and `f1`-`f12` | `crates/anvil-tui/src/keymap.rs`, `parse_key_name` |
| Config format | `[keymap.normal]`, `[keymap.pending]`, and serde `[keymap.contexts.<context>]`; optional top-level `preset` | `KeymapConfig::from_toml` |
| Config location | `ANVIL_KEYMAP`, XDG config, `~/.config/anvil/keymap.toml`, or `ANVIL_KEYMAP_PRESET` when no file exists | `KeymapConfig::default_path`, `Model::load_keymap_config` |
| Command contexts | Command registry still classifies `Normal` and `Pending`; routing contexts are separate | `crates/anvil-session/src/command.rs`, `keymap.rs` |
| Routing | Fixed `if` chain in `handle_key`, with editor/global context lookup in normal mode | `crates/anvil-tui/src/keys.rs` |
| Popup precedence | The explicit overlay stack top consumes first; configured popup bindings run before its built-in keys | `handle_key`, `overlay_stack.rs` |
| Space leader | `Space` becomes `Pending(' ')` with a 600 ms deadline; next key is not consumed on expiry | `normal_action`, `begin_pending`, `expire_leader` |
| Default Space actions | `Space 1` editor, `Space 2` tree, plus `Space e`/`Space f` compatibility aliases | `pending_action` |
| F-key defaults | None | `normal_action`, `pending_action` |
| F-key user bindings | Supported and schema-validated | `parse_key_name`, keymap tests |
| Presets | `anvil-ergo.toml` and `anvil-vim.toml` bundled under `crates/anvil-tui/presets` | `KeymapConfig::from_preset` |
| ADR file | No `docs/adr/0020-*` file exists | `docs/adr` |

## Requirement gaps

| Requirement | Status | Notes |
|---|---|---|
| `KeymapContext` identifiers | Landed in `anvil-session` | The issue named `anvil-editor`, but UI contexts stay out of the kernel per ADR-0013. |
| Router by specificity | Partial | Context priority is a tested list; tree/popup/modal branches still use explicit `if` checks. |
| Popup consumes `j` first | Landed | A regression test keeps popup navigation from moving the editor cursor. |
| Space leader timeout parser | Landed | 600 ms deadline with an expiry test. |
| Externalized config | Partial | serde contexts and presets landed; legacy normal/pending aliases remain for one release. |
| F-key compatibility | Landed | `f1`-`f12` parse only when a user config asks for them. |
| No F-key defaults | Landed | No F1-F12 default binding exists. |
| Ergo default preset | Landed as an additive preset | Built-in Vim-like fallbacks remain underneath. |
| Vim preset | Landed | `anvil-vim.toml` makes the Vim bindings explicit. |
| Full context dispatch | Landed | `file_tree`, `popup`, and `modal` bindings dispatch before built-in keys; covered by interaction tests. |
| ADR-0020 acceptance tests in `anvil-editor` | Blocked | The router lives in `anvil-tui`; `anvil-editor` has no key event type. |

## Conflicts and decisions needed

### 1. Where should `KeymapContext` live?

The issue says `anvil-editor`. The current architecture puts key events in
`anvil-tui` and stable command IDs in `anvil-session`. `anvil-editor` is the
kernel and has no UI or crossterm dependency.

Resolved split:

- `anvil-session`: stable `KeymapContext` identifiers, command IDs, and
  context metadata. No crossterm types.
- `anvil-tui`: crossterm `KeyEvent` normalization, chord parsing, leader
  timeout, and the actual router.
- `anvil-editor`: unchanged.

Do not move `KeymapContext` into `anvil-editor`; that would cross the kernel
boundary in ADR-0013.

### 2. Space mapping conflict

The issue acceptance says:

- `Space f` opens the FileTree;
- `Space e` focuses the Editor.

Current behavior and documentation say:

- `Space e` opens the FileTree;
- `Space f` opens the fuzzy file picker.

The addendum instead describes `Space + 1/2/3` for FocusEditor and
ToggleFileTree. Resolution implemented in `0.5.3`:

- `Space 1` focuses the editor;
- `Space 2` opens/focuses the file tree;
- `Space 3` focuses the right split when a split exists;
- `Space e` and `Space f` remain compatibility aliases;
- the bundled ergo preset adds the same bindings without F-keys.

### 3. F-key policy

The addendum supersedes the original "No F-Keys" wording:

- defaults must not use F1-F12;
- users may bind `F1`-`F12`;
- terminal escape variants must normalize to `KeyCode::F(n)`.

crossterm already normalizes most terminal F-key sequences into `KeyCode::F`.
Anvil does not write a second raw-escape parser. Implemented:

- `parse_key_name` accepts `f1`-`f12` case-insensitively;
- config tests cover F-key bindings;
- bundled presets are tested to contain no F-key bindings;
- terminal compatibility still needs a recorded matrix.

### 4. Leader timeout

The event loop already polls with a 50 ms timeout:

```rust
if event::poll(Duration::from_millis(50))? {
    handle_event(&mut model, event::read()?);
}
```

That was enough for a leader deadline without a new thread. Implemented:

- `leader_deadline: Option<Instant>` is stored with the pending leader;
- the next loop tick expires it after 600 ms;
- expiry clears pending state without consuming the next key;
- `Esc` cancels immediately.

The leader state must not run before popup routing. Target routing order:

Modal/Popup > CommandLine > Rename/Search > Editor > FileTree > Global

PendingLeader is not a context. It must not shadow an active overlay.

If leader routing must be first, active overlays must explicitly cancel the
leader on entry, or `Space` will steal keys from an open popup.

### 5. Config schema

Legacy mode tables remain supported:

```toml
[keymap.normal]
x = "editor.undo"

[keymap.pending.d]
d = "editor.yank_lines"
```

Nested context sections and a top-level preset are implemented:

```toml
preset = "ergo"

[keymap.contexts.editor]
"ctrl-w" = "view.split_vertical"

[keymap.contexts.file_tree]
j = "cursor.down"

[keymap.contexts.global]
"space 1" = "view.focus_editor"
```

The sequence-table alternative was rejected. Unknown contexts, duplicate
chords after normalization, unknown commands, and state-only commands that
cannot run directly reject the file. Context values may target normal commands
or direct operator-pending commands such as `view.*`; operator state commands
are rejected.

`serde` parsing stays in `anvil-tui` via `toml_edit`'s serde feature; the
command registry remains in `anvil-session`.

### 6. Presets

Two additive presets are shipped:

- `crates/anvil-tui/presets/anvil-ergo.toml`: ergonomic Space leader, no
  F-keys; also selected by `preset = "ergo"` or `"default"`.
- `crates/anvil-tui/presets/anvil-vim.toml`: explicit Vim-style normal
  bindings; selected by `preset = "vim"`.

The built-in fallback remains underneath, so presets cannot remove a default
binding yet. Preset selection precedence:

```text
ANVIL_KEYMAP file > user keymap.toml > ANVIL_KEYMAP_PRESET > built-in default
```

Do not silently rewrite a user's existing config.

## Implementation status

1. **Config and key parsing — landed**
   - `f1`-`f12` parsing and tests.
   - serde structs for contexts and presets.
   - Legacy normal/pending tables retained.
   - Clear errors for unknown contexts, commands, and duplicates.

2. **Contexts — landed**
   - `KeymapContext`, derived context list, editor/global binding dispatch,
     and direct pending-command context actions.
   - Popup, file-tree, and modal bindings dispatch through the ordered context
     list before their built-in fallback keys.
   - `route_context_key` is a pure resolver whose handler is exercised with a
     fake handler in unit tests.
   - An explicit LIFO overlay stack now owns the command palette, settings,
     buffer picker, fuzzy finder, problems panel, workspace/document symbols,
     references, call hierarchy, code actions, hover, diagnostic peek, theme
     browser, debug panel, test explorer, and completion: only the top entry
     consumes keys, and closing restores focus.
   - `active_keymap_contexts` no longer scans overlay booleans; the stack is the
     only source of popup/modal contexts.
   - Signature help and the location panel are passive render surfaces; the
     read-only pager remains an editor buffer rather than a modal layer.

3. **Leader — landed**
   - 600 ms deadline on the 50 ms loop tick.
   - Tests cover timeout, `Esc`, and popup precedence.

4. **Defaults and presets — landed**
   - No F-keys in bundled presets.
   - Space+1/2/3 plus `Space e`/`Space f` aliases.
   - Vim preset added.
   - Limitation: presets are additive over built-in fallbacks.

5. **Acceptance tests — mostly landed**
   - `Space 1` focuses editor; `Space 2` focuses file tree.
   - Popup `j` moves popup selection and does not move the editor cursor.
   - F1-F12 bind only when a user config asks for them.
   - No F1-F12 appear in bundled presets.
   - Leader timeout returns to normal mode.
   - Context priority order is asserted directly.
   - Config rejects unknown contexts, duplicate chords, and non-direct
     context commands.
   - Command palette over settings proves only the top overlay consumes and
     closing restores focus in LIFO order.
   - Remaining: LIFO coverage for every migrated overlay.

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

The TUI-local router with context identifiers in `anvil-session`, F-key parsing
only, a deadline-based leader, serde-backed contexts, and bundled presets is in
place. Do not move UI contexts into `anvil-editor`.

The command palette, operation-scoped `ModalLayer`, and shared operator/range
transaction path are landed. The ordered context stack, pure router, and
popup/tree/modal dispatch are landed; the next router work is full
modal-stack LIFO focus-restoration coverage.

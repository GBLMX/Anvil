# Keymap Configuration

Anvil loads an optional TOML keymap at startup. It can remap normal-mode and
operator-pending bindings without changing the built-in defaults.

The proposed contextual router, Space-leader timeout, F-key compatibility, and
preset work are analyzed in
[keymap-router-analysis.md](keymap-router-analysis.md).

## Location

Checked in order:

1. `ANVIL_KEYMAP`
2. `$XDG_CONFIG_HOME/anvil/keymap.toml`
3. `~/.config/anvil/keymap.toml`

A missing file keeps the default keymap. `ANVIL_KEYMAP` is intended for an
explicit path and for tests.

## Format

```toml
[keymap.normal]
x = "editor.undo"
alt-c = "selection.extend_up"
"ctrl-shift-p" = "command_line.open"

[keymap.pending.d]
d = "editor.yank_lines"
w = "editor.delete_words_next"

[keymap.pending." "]
f = "picker.fuzzy"
r = "editor.rename_symbol"
```

Each value is a command ID. Unknown IDs, invalid contexts, malformed keys,
duplicate chords, or an oversized file reject the entire file; defaults remain
active and the startup status line reports the error.

The configuration is parsed and validated before it replaces the active keymap.
It is not hot-reloaded.

## Key syntax

- A single character: `x`, `S`
- Named keys: `space`, `esc`, `tab`, `enter`, `backspace`, `delete`, `insert`,
  `left`, `right`, `up`, `down`, `home`, `end`, `pageup`, `pagedown`
- Modifiers: `ctrl-`, `alt-`, `super-`, `shift-`
- Examples: `ctrl-w`, `alt-c`, `shift-s`, `ctrl-shift-p`

`shift-` requires a letter. `alt-` letters are matched case-insensitively, so
`alt-c` and `alt-C` refer to the same binding.

## Normal commands

```text
editor.collapse_selections
lsp.hover
macro.record
macro.replay
selection.split_lines
selection.word_occurrences
selection.extend_up
selection.extend_down
editor.insert_here
command_line.open
search.open
editor.append
editor.append_line_end
editor.open_below
editor.open_above
search.next
search.previous
editor.delete_forward
editor.put
editor.undo
editor.redo
cursor.line_start
cursor.line_end
cursor.word_next
cursor.word_previous
cursor.word_end
cursor.left
cursor.right
cursor.down
cursor.up
```

## Operator-pending commands

```text
editor.change_inner_word
editor.change_inner_quote
lsp.definition
diagnostics.next
diagnostics.previous
view.split_vertical
view.focus_left
view.focus_right
view.close_split
tree.open
picker.fuzzy
editor.rename_symbol
lsp.code_action
lsp.document_symbols
lsp.workspace_symbols
lsp.format
editor.change_word_next
editor.change_word_end
editor.change_word_previous
editor.delete_lines
editor.yank_lines
editor.delete_words_next
editor.delete_words_end
editor.delete_words_previous
editor.yank_words_next
editor.yank_words_end
editor.yank_words_previous
```

## Limits

- Maximum file size: 64 KiB.
- No includes, environment expansion, or Lua execution.
- Only normal and operator-pending contexts are configurable in this version.
- Insert, command-line, search, rename, and overlay input keep their built-in
  behavior.

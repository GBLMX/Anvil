# Keymap Configuration

Anvil loads an optional TOML keymap at startup. It can remap normal-mode,
operator-pending, and editor/global context bindings without changing the
built-in defaults. Key syntax, canonical bindings, and the Space-leader timeout
are documented below; the historical 0.5.3 router design is in
[keymap-router-analysis.md](keymap-router-analysis.md).

## Location

Checked in order:

1. `ANVIL_KEYMAP`
2. `$XDG_CONFIG_HOME/anvil/keymap.toml`
3. `~/.config/anvil/keymap.toml`
4. `ANVIL_KEYMAP_PRESET` when no keymap file exists

A missing file keeps the default keymap. `ANVIL_KEYMAP` is intended for an
explicit path and for tests. `ANVIL_KEYMAP_PRESET` selects a bundled preset by
name (`ergo` or `vim`).

## Format

```toml
# Optional base preset; user tables override its bindings.
preset = "ergo"

[keymap.normal]
x = "editor.undo"
alt-C = "selection.extend_up"
"ctrl-shift-p" = "command_line.open"

[keymap.pending.d]
d = "editor.yank_lines"
w = "editor.delete_words_next"

[keymap.pending." "]
f = "picker.fuzzy"
e = "tree.open"
p = "command_palette.open"
r = "editor.rename_symbol"

[keymap.contexts.editor]
"ctrl-w" = "view.split_vertical"

[keymap.contexts.global]
"alt-u" = "editor.undo"
```

Each value is a command ID. Unknown IDs, invalid or unknown contexts, malformed
keys, duplicate chords within a context, or an oversized file reject the entire
file; defaults remain active and the startup status line reports the error.

Context bindings take precedence over `[keymap.normal]` and the built-in
defaults. Context values may use normal commands or direct operator-pending
commands such as `view.split_vertical`; commands that only make sense after an
operator prefix are rejected. All context sections are dispatched:
`editor`, `file_tree`, `popup`, `modal`, `command_line`, `search`, `rename`,
and `global`. Text-input contexts use only their own explicit bindings and do
not fall back to `global`. For editor and file-tree handling, `global` bindings
are applied after the active context's built-in keys are left unhandled;
popup/modal built-ins consume first and do not fall back to global bindings.

## Presets

Two bundled presets are available:

- `anvil-ergo.toml` (`ergo`, also `default`): the ergonomic Space leader.
- `anvil-vim.toml` (`vim`): explicit Vim-style normal bindings.

Bundled presets do not bind F1–F12. A keymap file selects a preset with the
top-level `preset` key and can override any of its bindings in the normal,
pending, or context tables. Without a keymap file, `ANVIL_KEYMAP_PRESET` may be
used; the built-in defaults remain active if neither is present.

The configuration is parsed and validated before it replaces the active keymap.
It is not hot-reloaded.

The Space leader expires after 1200 ms. `Space e` opens the file tree,
`Space d` opens the filterable diagnostics picker, `Space g` lists changed
files, `Space j` opens the jump list, `Space H` opens search history,
`Space :` opens command history, `Space K` opens the full keymap list
(`:keybinds` shows the keymapped-command subset), `Space p` opens the command
palette, and `Ctrl-w` handles window focus without numbered aliases.

## Canonical bindings

One action has one documented key. Hardware keys and terminal encodings may
remain as internal aliases, but they are not separate advertised shortcuts.

| Action | Key |
|---|---|
| Collapse selections | `Esc` |
| Select word occurrences | `s` |
| Split selection into lines | `S` |
| Add cursor below | `C` |
| Add cursor above | `Alt-C` |
| Jump back / forward | `Alt-o` / `Alt-i` |
| Toggle line comment | `Ctrl-/` |
| File picker | `Space f` |
| Buffer picker | `Space b` |
| File tree | `Space e` |
| Diagnostics picker | `Space d` |
| Changed files | `Space g` |
| Jump list | `Space j` |
| Search history | `Space H` |
| Command history | `Space :` |
| Keymap list | `Space K` |
| Document / workspace symbols | `Space s` / `Space S` |
| Rename / code action | `Space r` / `Space a` |
| Format | `Space F` |
| Command palette | `Space p` |
| Window prefix | `Ctrl-w` |

`Ctrl-_` remains accepted only because many terminals encode `Ctrl-/` as
`0x1f`; it is not documented as a second shortcut. `Home`/`End` and arrow keys
remain hardware aliases for `0`/`$` and `h/j/k/l`.

`Ctrl-/` toggles line comments only. It recognizes `//` for Rust, Go, C-family,
JVM, Swift, PHP, and JavaScript/TypeScript; `#` for shell, Python, TOML, YAML,
Rust-adjacent config, Ruby, and the listed dotfiles; `--` is used for SQL
and Lua. Supported dotfiles are `.bashrc`, `.bash_profile`, `.zshrc`,
`.zprofile`, `.profile`, `.env`, `.editorconfig`, `.gitignore`,
`.gitattributes`, `.dockerignore`, `.npmrc`, `.prettierignore`, and
`.eslintignore`. HTML, CSS, XML, and unknown extensions report an unsupported
line-comment type instead of inserting an invalid marker; block comments are
not implemented.

Visual `v`/`V` selections support `d`, `y`, and `c`. Press `s` to open the
operation-scoped surround layer, then `(`/`)`, `[`/`]`, `{`/`}`, `<`/`>`,
`"`, `'`, or `` ` `` to wrap the selection. `Esc` cancels the layer.

## Key syntax

- A single character: `x`, `S`
- Named keys: `space`, `esc`, `tab`, `enter`, `backspace`, `delete`, `insert`,
  `left`, `right`, `up`, `down`, `home`, `end`, `pageup`, `pagedown`,
  `f1`–`f12`
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
editor.toggle_comment
navigation.jump_back
navigation.jump_forward
navigation.match_bracket
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
lsp.hover
editor.toggle_comment
navigation.change_back
navigation.change_forward
diagnostics.next
diagnostics.previous
view.split_vertical
view.focus_left
view.focus_right
view.focus_editor
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
- Normal and operator-pending bindings remain configurable. Context tables are
  parsed and validated, but only `editor` and `global` dispatch today.
- Insert, command-line, search, rename, and overlay input keep their built-in
  behavior.
- Presets are additive over the built-in fallback; they cannot remove a default
  binding yet.

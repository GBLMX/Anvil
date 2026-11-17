# Getting started

Anvil is a modal editor with a selection-first core. This page is also available
inside the editor with `:help start`.

## First-run task list

1. Start in a real project:

   ```bash
   anvil --workspace ~/src/my-project
   ```

2. Confirm the install:

   ```text
   :help start
   anvil doctor --profile installed
   ```

3. Open a file with `Space f` (fuzzy finder) or `Space e` (file tree).
4. Enter insert mode with `i`, make a change, then `Esc`.
5. Save with `:w`; leave with `:q`.
6. Inspect settings with `:settings`; the selected row shows its source.
7. Review recovery rules with `:help recover` before relying on undo or swaps.

## Minimal keymap card

| Key | Action |
|---|---|
| `i` / `a` / `A` / `o` / `O` | insert modes |
| `Esc` | normal mode |
| `h` `j` `k` `l` | motions |
| `w` `b` `e` | word motions |
| `x` / `dd` / `yy` / `p` | edit and paste |
| `u` / `Ctrl+r` | undo / redo |
| `/` then `n` / `N` | search |
| `Space f` | fuzzy file finder |
| `Space e` | file tree |
| `Space d` | diagnostics picker |
| `Space g` | changed files |
| `Space j` | jump list |
| `Space H` | search history |
| `Space :` | command history |
| `Space K` | keymap list |
| `Space L` | language readiness |
| `Space b` | buffer picker |
| `Space p` | command palette |
| `Space s` | document symbols |
| `K` | hover |
| `gd` / `gr` | definition / references |
| `:` | Ex command |
| `?` | help |
| `:q` | quit |

## Everyday commands

```text
:w                  save
:wa                 save every modified buffer
:wq                 save and quit
:q!                 discard changes and quit
:settings           settings overlay with source labels
:config-reload      reload and report effective files
:watch-status       watcher scope and state
:help recover       save/undo/swap semantics
:git-diff           current-file diff
:git-blame          current-file blame
:search-project Q   bounded project search
:task <command>     run a bounded workspace task
```

Language servers are not bundled. `anvil doctor` reports which supported
servers are on `PATH`; install only the ones you use.

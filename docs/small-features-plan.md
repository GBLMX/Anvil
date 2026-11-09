# Small Features Plan

Status: proposed. This is the small, Vim-like ergonomics lane under the frozen
`0.6.0` internal-polish phase. It does not replace the P0 work in
[internal-polish-plan.md](internal-polish-plan.md) or the capability gates in
[ide-capability-plan.md](ide-capability-plan.md).

Scope: missing editor motions, operators, registers, visual-mode actions, and
small display/Ex commands that are cheap to understand, bounded in behavior,
and pay off during dogfooding. No new event loop, no Vimscript, no plugin
keymap host, and no second editing model.

## 1. Current baseline

Already present and should be reused:

- normal/insert/visual modes, multi-cursor `s`/`Alt-s`/`C`, visual `v`/`V`;
- `h`/`j`/`k`/`l`, `w`/`b`/`e`, `0`, `Home`/`End`;
- `i`/`a`/`A`/`o`/`O`, `x`, `p`, `u`, `Ctrl+r`, `.`;
- `dd`/`yy`, `dw`/`db`/`de`, `yw`/`yb`/`ye`, text objects, paragraphs, tags;
- `G` last line and `count G` line jump;
- `/`/`n`/`N`, literal and regex search, in-buffer replace;
- folds `za`/`zM`/`zR`, windows `Ctrl+w`, LSP `gd`/`gr`/`gh`/`gi`/`go`/`K`;
- command palette, which-key, fuzzy open, settings overlay, themes.

The gap is therefore not “the editor has no Vim keys”. The gap is the long tail
that users reach for without thinking.

## 2. Known conflicts

These must be respected before adding “Vim” bindings:

| Key | Current meaning | Decision |
|---|---|---|
| `C` | extend selection down / add cursor | Keep. Do not silently reinterpret as Vim `C`. Add `c$` as change-to-end alias first; a modal remap needs a separate decision. |
| `s` | select word occurrences | Keep. Vim substitute-character is not added under the same key. |
| `v` / `V` | visual mode | Keep. |
| `Ctrl+v` | multi-cursor adapter | Keep. |
| `g`, `z`, `[`, `]` | pending prefixes | Extend through `PendingCommand`, not per-key special cases. |
| `r` | plain `r` is free; `Ctrl+r` is redo | `r` may become replace-character. |
| `Space` | leader | Keep. Do not bind small actions directly to Space. |

## 3. Gap list

Priority order is daily-value first, not Vim completeness.

| Pri | Feature | Key | Missing behavior | Size |
|---|---|---|---|---|
| S1 | file start | `gg` | first line | S |
| S1 | line end | `$` | line end, count-aware | S |
| S1 | first non-blank | `^` | first non-blank column | S |
| S1 | insert first non-blank | `I` | insert at first non-blank column | S |
| S1 | inline find | `f`/`t`/`F`/`T` | find/till character, `;`/`,` repeat | M |
| S1 | delete to end | `D` | delete cursor to line end | S |
| S1 | yank line | `Y` | yank current line (or selection) | S |
| S1 | paste before | `P` | paste before cursor/selection | S |
| S1 | join lines | `J` / `gJ` | join with space / without space | S |
| S1 | replace character | `r` | replace one grapheme, count-aware | M |
| S1 | toggle case | `~` | toggle grapheme/selection case | S |
| S2 | half/full page | `Ctrl+d`/`Ctrl+u`/`Ctrl+f`/`Ctrl+b` | viewport-aware scrolling | M |
| S2 | scroll cursor line | `zz`/`zt`/`zb` | center/top/bottom cursor line | S |
| S2 | matching bracket | `%` | jump to matching `()[]{}` | M |
| S2 | search word | `*`/`#` | search word under cursor, whole-word | M |
| S2 | operator + motion | `d$`, `d0`, `dj`, `dk`, `dG`, `dgg` | compose `d`/`c`/`y` with non-word motions | M |
| S2 | indent/dedent | `>>`/`<<` | block indent with visible count | M |
| S2 | change to end alias | `c$` | safe alternative to conflicting `C` | S |
| S3 | named registers | `"a` … `"z`, `"A` | read/write/append | M |
| S3 | special registers | `"0`, `"_` | last yank, black hole | S |
| S3 | register browser | `:registers` | bounded read-only list | S |
| S3 | clear search highlight | `:noh` | remove transient match highlight | S |
| S4 | visual swap ends | `o` | move active end of selection | S |
| S4 | reselect | `gv` | restore last visual selection | S |
| S4 | visual indent/case/join | `>`/`<`/`~`/`J` | selection-aware actions | M |
| S5 | column marker | `colorcolumn` | one or more vertical markers | M |
| S5 | cursor line | `cursorline` | highlight current line | S |
| S5 | line-number modes | `number`, `relativenumber` | runtime toggle | S |
| S5 | whitespace display | `list`, `listchars` subset | tabs/trailing space markers | M |
| S5 | redraw | `Ctrl+l` | clear and repaint | S |
| S5 | buffer list/switch | `:ls`, `:b N`, `Ctrl+^` | list and switch by number/alternate | M |
| S5 | force reload/quit | `:e!`, `:qa` | explicit discard/reload paths | S |
| S6 | jump history | `Ctrl+o`/`Ctrl+i` | back/forward after jumps | M |
| S6 | LSP declaration family | `gD`/`gy`/`gI` | declaration/type/implementation | M |
| S6 | LSP status/restart | `:LspInfo`, `:LspRestart` | visible lifecycle control | M |

Do not start S6 until S1–S3 are dogfooded. Marks (`m`/`'`/`` ` ``) are useful
but interact with persistence; keep them out of the first pass.

## 4. Slices

### S1: Motion and edit essentials

Deliver:

- `gg`, `$`, `^`, `I`.
- `f`/`t`/`F`/`T` with `;`/`,` repeat, count support, current-line boundary.
- `D`, `Y`, `P`, `J`/`gJ`, `r`, `~`.
- `c$` as the non-conflicting change-to-end alias.

Rules:

- One command maps to one undo transaction.
- Motions must respect folded lines and must not place the cursor on a hidden
  line.
- `r` replaces one grapheme, not one byte; wide and combining characters are
  covered by tests.
- Counts are consumed exactly once; a failed motion still clears the count.
- Multi-cursor actions apply to every selection or explicitly reject the
  command with a status message.

Acceptance:

- Unit tests for start/end of file, trailing newline, empty file, count, fold
  boundary, multibyte/wide grapheme, read-only buffer, and undo grouping.
- TUI smoke for `gg`, `G`, `D`, `J`, `r`, and `~`.

### S2: Viewport, search, and operator composition

Deliver:

- `Ctrl+d`/`Ctrl+u` half-page, `Ctrl+f`/`Ctrl+b` full-page.
- `zz`/`zt`/`zb`.
- `%` matching bracket with a bounded scan.
- `*`/`#` word search and `:noh`.
- `>>`/`<<` for current line and visual selection.
- Operator composition for `d`/`c`/`y` with `$`, `0`, `G`, `gg`, `j`/`k`.

Rules:

- Scroll commands change the viewport, not the buffer.
- `Ctrl+d`/`Ctrl+u` preserve the existing scroll margin.
- Bracket matching never scans the whole document for an unbalanced opener;
  cap at the visible/parseable window and report failure without moving.
- `*` uses the same word boundaries as `s`/`w`, not a separate regex.
- Indent width comes from the existing display/tab policy; do not hardcode a
  second tab width.
- Operator composition reuses one motion-range implementation; `D` is not a
  separate range engine from `d$`.

Acceptance:

- Tests for 0-height/1-line viewports, narrow terminals, folded regions,
  no-match bracket, count on `*`, and one-step undo for indentation.

### S3: Registers and search hygiene

Deliver:

- Append form `"A`–`"Z`.
- `"0` last yank.
- `"_` black hole.
- `:registers` with bounded output.
- `:noh`.

Rules:

- Existing unnamed register behavior must stay compatible.
- Clipboard registers `"+` and `"*` keep their current failure behavior; a
  missing clipboard never loses buffer text.
- Register text is stored as text, not as byte offsets that can become stale.
- `:registers` is read-only and never prints unbounded contents.

Acceptance:

- Tests for yank/delete/paste across registers, append, black hole, clipboard
  failure, and `:noh` after search.

### S4: Visual-mode polish

Deliver:

- `o` to swap the active selection end.
- `gv` to reselect.
- `>`/`<`, `~`, `J`, `p` in visual mode.

Rules:

- Visual commands end visual mode where Vim does; do not leave a stale anchor.
- Linewise and charwise selections keep their current distinction.
- Multi-cursor visual state is either applied to all selections or rejected
  before mutation.

Acceptance:

- Tests for forward/backward selections, linewise whitespace, mixed-width
  characters, reselect after edit, and Esc cleanup.

### S5: Small display and Ex polish

Deliver:

- `editor.colorcolumn` (integer or comma-separated small list).
- `editor.cursorline`.
- `number`/`relativenumber` runtime toggles.
- `list`/`listchars` subset for tabs and trailing spaces.
- `Ctrl+l`.
- `:ls`, `:b N`, `Ctrl+^`, `:e!`, `:qa`.
- `:messages` if the existing status buffer can be bounded safely.

Rules:

- Settings stay typed and go through the existing registry.
- Rendering consumes display columns, not byte columns.
- `colorcolumn` is clamped to terminal width at render time; it does not
  allocate per frame.
- `:e!` is the only explicit discard path; normal `:e` keeps the dirty guard.
- `:ls` and messages are bounded and read-only.

Acceptance:

- Render tests at 40/80/200 columns with wide characters, tabs, and horizontal
  scroll.
- Settings tests for parse/validate/save and invalid values.
- Buffer-switch tests for dirty state and missing files.

### S6: Navigation polish

Only after S1–S3:

- `Ctrl+o`/`Ctrl+i` jump history, bounded and deduplicated.
- `gD` declaration, `gy` type definition, `gI` implementation.
- `:LspInfo`/`:LspRestart` with the existing worker lifecycle.
- Marks only if a persistence schema is accepted; otherwise leave them out.

## 5. Proposed order

1. S1: `gg`, `$`, `^`, `D`, `Y`, `P`, `J`, `r`, `~`, `f`/`t`.
2. S2: scrolling, bracket matching, `*`/`#`, indent.
3. S3: registers, `:noh`.
4. S4: visual-mode polish.
5. S5: display settings and Ex polish.
6. S6: navigation only after dogfooding.

Each slice ships with tests, docs, and a TUI smoke. Do not batch multiple
slices into one unreviewable change.

## 6. Non-goals

- Vimscript or Lua keymaps.
- A second operator/editing model.
- Full Vim register model (numbered registers, expression registers, macros
  beyond the current macro support).
- Persistent marks before session storage exists.
- Git, sessions, multi-file search, tasks, terminal, debugger, or code lens.
- Any default binding that removes the current multi-cursor keys without an
  explicit migration decision.

## 7. Verification gates

Every slice:

```sh
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Changed TUI surfaces also require a real PTY smoke test. Tests must cover
counts, trailing newlines, empty buffers, read-only buffers, folded lines,
multi-cursor state, Unicode graphemes, and undo transaction boundaries.

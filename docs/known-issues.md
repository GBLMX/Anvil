# Known Issues

Status: mixed. BUG-001, BUG-004, BUG-005, BUG-006, BUG-007, and BUG-009
are fixed; the remaining entries are open.

## BUG-001: File tree and fuzzy picker do not scroll with the selection

- Area: TUI, `Space e` file tree and `Space f` fuzzy file picker
- Status: Fixed 2026-11-01; tree and fuzzy viewports follow the selected row.
- Severity: usability
- Repro:
  1. Open a workspace with more tree entries or fuzzy results than fit in the
     panel.
  2. Open the file tree with `Space e` or the fuzzy picker with `Space f`.
  3. Move the selection down past the visible rows.
- Actual: the selection moves, but the visible window stays anchored at the
  start of the list. The selected row can disappear below the panel.
- Expected: the list should scroll to keep the selected row visible, including
  when moving up from the bottom and when the list is filtered.
- Original evidence (fixed):
  - Tree rendering iterates the full `model.tree` and renders a `Paragraph`
    without a scroll offset (`crates/anvil-tui/src/render.rs`, tree block).
  - Fuzzy rendering uses `filtered.iter().take(height - 3)` from index zero and
    has no selected-item offset (`crates/anvil-tui/src/render.rs`, fuzzy block).
  - Selection movement only changes `tree_selected` / `fuzzy.selected`
    (`crates/anvil-tui/src/file_tree_ui.rs`).
- Verification:
  - Both panels keep the selected row visible at the top and bottom edges.
  - Regression tests select the last row in a 20-entry tree and fuzzy list.
  - Existing selection and filtering behavior remains unchanged.

## BUG-002: Syntax highlighting does not cover Anvil project file formats

- Area: TUI syntax highlighting
- Status: Open; not fixed
- Severity: feature coverage / usability
- Current coverage: only Rust and JSON are registered in
  `crates/anvil-tui/src/syntax.rs` (`language_for`).
- Required baseline: at least the formats used by the Anvil repository:
  - Rust (`.rs`)
  - TOML (`.toml`)
  - Go (`.go`)
  - Markdown (`.md`)
  - JSON / JSON with comments (`.json`, `.jsonc`)
  - YAML (`.yml`, `.yaml`)
  - Shell (`.sh`, `.bash`)
  - Python (`.py`) for repository scripts
  - Extensionless project files such as `go.mod`, `Makefile`, and
    `Dockerfile`, by filename detection
- Evidence:
  - `crates/anvil-tui/Cargo.toml` only includes `tree-sitter-json` and
    `tree-sitter-rust`.
  - `language_for` switches on the file extension and has no fallback for
    extensionless files.
  - Unsupported files currently clear the syntax cache and render as plain text
    (`SyntaxState::poll`).
- Acceptance for a future fix:
  - Every baseline format is recognized by extension or filename.
  - Unsupported formats still render safely as plain text.
  - Highlighting remains bounded by `MAX_HIGHLIGHT_BYTES`.

## BUG-003: Markdown highlighting needs a deliberate grammar and query

- Area: TUI syntax highlighting, Markdown
- Status: Open; not fixed
- Severity: feature quality
- Current behavior: `.md` is not registered at all.
- Required attention:
  - headings and heading levels
  - fenced and inline code, including fence language tags
  - emphasis and strong emphasis
  - links, images, and reference links
  - lists, task markers, and block quotes
  - tables
  - HTML blocks and frontmatter
  - preserve plain-text fallback for malformed Markdown
- Notes:
  - Do not treat Markdown punctuation as code tokens; fenced code should use
    the nested language grammar where available.
  - Large Markdown files should still obey the existing 256 KiB highlight
    limit and remain editable without highlighting.
- Acceptance for a future fix:
  - Headings, links, inline code, fenced code, emphasis, lists, quotes, and
    tables have visibly distinct classes.
  - Malformed input never panics or shifts text; the editor remains usable.
  - Markdown tests cover fenced code, nested lists, frontmatter, and a broken
    fence.

## Urgent foundational gaps

These are missing capabilities, not regressions. They block normal IDE-like
workspace management and should be addressed before adding more UI features.

### BUG-004: File tree cannot create files or directories

- Area: TUI `Space e` file tree; gateway filesystem API
- Priority: Urgent
- Status: Fixed 2026-11-01; `a`/`A` create entries through confined gateway
  operations.
- Original behavior:
  - Tree navigation, expand/collapse, and open only.
  - The gateway had no directory creation API.
  - `:w path` could create a file only when its parent existed.
- Implemented resolution:
  - `a` creates a file and `A` creates a directory in the selected directory,
    or in the selected file's parent.
  - Inline prompt; `Enter` creates, `Esc` cancels.
  - A created file opens; a created directory is selected after refresh.
  - Names are single components; empty, `.`/`..`, separators, control
    characters, and existing targets are rejected without overwrite.
  - `sys.create_dir`, `sys.rename`, and `sys.remove` use the workspace root
    dirfd and require `fs.write`; empty files use `sys.write_file`.
  - Tree refresh keeps the new entry selected.
- Verification:
  - TUI tests cover create/rename/delete and open-buffer delete guards.
  - Go fsroot tests cover confined create/rename/delete and no-replace.
- Acceptance:
  - File and directory creation work in the tree without direct `std::fs`
    access.
  - Outside-workspace and symlink escapes are denied.
  - Existing entries are never silently overwritten.

### BUG-005: File tree cannot rename, delete, or move entries

- Area: TUI file tree; gateway filesystem API
- Priority: Urgent
- Status: Fixed 2026-11-01; move, copy, recursive delete, and open-path
  remapping are implemented.
- Resolution:
  - `r` renames the selected entry in place.
  - `m` moves and `c` copies the selected file into a destination directory.
  - `d` recursively deletes a file, symlink, or directory tree.
  - Operations run through the Go gateway in the background `FileWorker`; the
    status bar shows a spinner while a transfer or delete is in flight.
  - Open buffer paths are remapped after a move.
  - Gateway additions: `sys.rename` (move), `sys.copy`, and `sys.remove_all`.
  - `sys.copy` streams with `io.Copy`; recursive delete walks with
    descriptor-relative no-follow opens; source and destination symlinks are
    rejected or removed without following.
  - TUI tests cover move/copy/recursive delete and outside-workspace rejection;
    Go tests cover copy, no-overwrite, symlink escape, and recursive delete.
- Acceptance:
  - A file can move to another workspace directory; open paths follow it.
  - A regular file can copy without replacing an existing target.
  - A non-empty directory can be deleted recursively while a symlink target
    outside the workspace survives.
  - `..`, absolute paths, symlink-parent escapes, and outside-workspace targets
    are denied.
- Verification:
  - `cargo test --workspace --no-fail-fast`: 230 passed, 1 ignored.
  - `go test ./go/...` and `go vet ./go/...` pass.
  - Workspace clippy, fmt, and `git diff --check` pass.

### BUG-006: No new-buffer or close-buffer lifecycle

- Area: TUI buffer management
- Priority: High
- Status: Fixed 2026-11-01; new, indexed close, dirty-save prompt, force
  discard, split cleanup, and last-buffer reset are implemented.
- Resolution:
  - `:new` creates an unnamed empty buffer.
  - `:bd` closes the active buffer; `:bd N` closes a 1-based index; `:bd!`
    skips the dirty guard.
  - Dirty close enters `Save before closing? [Y/n/c]`; `y` saves then closes,
    `n` discards, `c`/Esc cancels.
  - Closing a buffer clears a split pane showing the same path only when that
    pane is clean; a dirty split is refused rather than discarded.
  - Closing the last buffer resets to one clean unnamed buffer instead of
    leaving a stale editor state.
  - The removed `Buffer` and its `EditorState`/`LineIndex`/`PieceTable`
    handles are dropped after removal.
- Acceptance:
  - New, close, indexed close, save-before-close, and force-discard work with
    modified and unmodified buffers.
  - No stale split pane points at a removed buffer.
  - The active buffer and buffer count remain valid after every close.

- Verification:
  - `cargo test --workspace --no-fail-fast`: 230 passed, 1 ignored.
  - `cargo test -p anvil-tui buffer`: 13 passed.
  - Workspace clippy, fmt, and `git diff --check` pass.

### BUG-007: LSP does not auto-start for supported files

- Area: LSP lifecycle
- Priority: High (policy/UX gap)
- Status: Fixed 2026-11-01; supported files start their language server on
  open, and each `(tool, language_id)` keeps its own worker.
- Original behavior:
  - Opening a supported file only synced an already-running worker.
  - A single worker was replaced when switching languages, so Rust ↔ Go
    switching restarted the server and dropped the previous session.
- Resolution:
  - `sync_lsp_active` starts a missing worker for the current file.
  - Workers are keyed by `(tool, language_id)` and reused across buffers.
  - Switching Rust ↔ Go keeps both workers alive; diagnostics stay keyed by
    path.
  - Crash restart is bounded per language.
- Acceptance:
  - Buffer switches do not spawn duplicate workers for the same language.
  - Switching languages does not stop the other language server.
  - A crashed server follows the existing bounded restart policy.

### BUG-008: External file changes are not watched

- Area: Workspace/file lifecycle
- Priority: High
- Status: Partially fixed 2026-11-01; open-buffer external changes are in
  0.4.0 scope. Tree create/delete/rename refresh is scheduled for 0.5.0 in
  [#9](https://github.com/GBLMX/Anvil/issues/9).
- Resolution:
  - `sys.stat` now returns regular-file size and `modified_ns`.
  - `FileWorker::Watch` polls registered open-buffer paths through the gateway
    every 100 ms and emits only after 500 ms of unchanged metadata; the
    baseline is then updated, so one burst produces one notification.
  - Clean buffers reload from disk; dirty buffers enter
    `File changed on disk: <path> [y/N]` and never reload unless confirmed.
  - Watch paths are refreshed after open, save, new/close, split changes, and
    buffer moves.
  - Worker tests cover metadata-change debounce; TUI tests cover clean reload,
    dirty refusal, and confirmed reload.
- Remaining:
  - Directories are not yet watched, so external create/delete/rename does not
    refresh the workspace tree.
  - Tree refresh preservation of expanded/selected state is part of that
    remaining work.
  - Tracking issue:
    [#9: [0.5.0] Workspace tree watcher](https://github.com/GBLMX/Anvil/issues/9).
- Acceptance:
  - External change to a clean open buffer is reloaded.
  - External change to a dirty buffer prompts and never discards local edits.
  - External create/delete/rename tree refresh is explicitly deferred to
    [#9](https://github.com/GBLMX/Anvil/issues/9) for 0.5.0.
- Verification:
  - `cargo test --workspace --no-fail-fast`: 230 passed, 1 ignored.
  - `cargo test -p anvil-tui`: 124 passed.
  - `go test ./go/...` and `go vet ./go/...` pass.
  - Workspace clippy, fmt, and `git diff --check` pass.

### BUG-009: Search mode drops Tree-sitter foreground colors

- Area: TUI search rendering
- Priority: High
- Status: Fixed 2026-11-01; search overlays preserve syntax foregrounds.
- Reproduction:
  1. Open a Rust file with active Tree-sitter highlighting.
  2. Press `/` and type a character, for example `e`.
  3. Observe the viewport.
- Actual:
  - Search matches show a background highlight.
  - All syntax foreground colors disappear; the rest of the viewport renders as
    plain white/gray.
- Expected:
  - Syntax foreground colors remain intact.
  - Search only adds a background color/underline to matched ranges.
  - The active match can use a brighter background while retaining syntax fg.
- Original evidence:
  - `crates/anvil-tui/src/lib.rs` `visible_lines` only obtains the syntax cache
    when `needle.is_empty()`.
  - When a search query is non-empty, the branch emits `Span::raw` for
    non-matches and search-only styles for matches, bypassing
    `push_syntax_spans`.
  - Search styles are built from fresh `Style::default()` layers rather than
    patching the existing syntax span style.
- Resolution:
  - `visible_lines` always builds syntax spans, regardless of input mode.
  - `apply_search_highlights` splits existing spans at match boundaries and
    patches only background/modifier styles.
  - Normal and active matches use dedicated background styles; active matches
    are selected by start offset.
  - Search rendering reuses the chunk-safe `find_matches_in` matcher.
  - `search_preserves_syntax_and_adds_background` covers keyword fg + active bg.
  - The layer contract is recorded in
    `docs/adr/0016-render-style-layers.md`.
- Proposed layer order:
  1. syntax foreground
  2. search match background
  3. active search match background + bold
  4. selection background
  5. cursorline background
  6. LSP diagnostic underline/modifier
- Acceptance:
  - A search over a Rust keyword keeps its syntax fg and gains search bg.
  - Non-matching spans retain syntax colors.
  - Active match is distinguishable without replacing fg.
  - Diagnostics underlines still compose with search backgrounds.
  - Add a `TestBackend` regression test for fg+bg composition without requiring
    snapshot infrastructure.
- Verification:
  - Focused TUI test passes.
  - `cargo test -p anvil-tui`: 124 passed.
  - `cargo test --workspace --no-fail-fast`: 230 passed, 1 ignored.
  - Workspace clippy, fmt, and `git diff --check` pass.

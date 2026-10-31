# Known Issues

Status: open bugs recorded for a later fix. Nothing in this document is fixed
by the current change.

## BUG-001: File tree and fuzzy picker do not scroll with the selection

- Area: TUI, `Space e` file tree and `Space f` fuzzy file picker
- Status: Open; not fixed
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
- Evidence:
  - Tree rendering iterates the full `model.tree` and renders a `Paragraph`
    without a scroll offset (`crates/anvil-tui/src/render.rs`, tree block).
  - Fuzzy rendering uses `filtered.iter().take(height - 3)` from index zero and
    has no selected-item offset (`crates/anvil-tui/src/render.rs`, fuzzy block).
  - Selection movement only changes `tree_selected` / `fuzzy.selected`
    (`crates/anvil-tui/src/file_tree_ui.rs`).
- Acceptance for a future fix:
  - Both panels keep the selected row visible at the top and bottom edges.
  - Filtering resets or clamps the scroll position without hiding the selection.
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
- Status: Open; not fixed
- Current behavior:
  - `Space e` supports movement, expand/collapse, open, and hidden-file toggle
    only.
  - The gateway has `sys.write_file` but no directory creation API.
  - `:w path` can create a file only when its parent directory already exists.
  - There is no way to create a folder from Anvil.
- Required interaction:
  - `a` creates a file and `A` creates a directory in the selected directory,
    or in the selected file's parent.
  - Inline prompt; `Enter` creates, `Esc` cancels.
  - A created file opens; a created directory expands and selects.
  - Reject empty names, absolute paths, `..`, `/`, control characters, and
    existing targets without overwrite.
- Required plumbing:
  - Add a workspace-confined `sys.create_dir` gateway method using the root
    dirfd and `mkdirat`; require `fs.write`.
  - Use `sys.write_file` for the empty new file.
  - Refresh the tree through `FileWorker` and keep the new entry visible.
- Acceptance:
  - File and directory creation work in the tree without direct `std::fs`
    access.
  - Outside-workspace and symlink escapes are denied.
  - Existing entries are never silently overwritten.

### BUG-005: File tree cannot rename, delete, or move entries

- Area: TUI file tree; gateway filesystem API
- Priority: Urgent
- Status: Open; not fixed
- Current behavior: no tree keys or gateway methods exist for rename, unlink,
  move, or copy.
- Required interaction:
  - Rename current entry; delete current entry with explicit confirmation.
  - Never delete a modified open buffer without a clear guard.
  - Update open buffer paths and close affected split panes safely.
  - Keep operations workspace-confined and preserve revision/conflict checks.
- Required plumbing:
  - Add confined gateway methods for rename and unlink; do not shell out.
  - Add a confirmation state so destructive operations cannot run from a stray
    key press.
- Acceptance:
  - Rename/delete work for files and directories, including open buffers.
  - `..`, symlink, and outside-workspace attempts are denied.
  - Failed operations leave the tree and buffers unchanged.

### BUG-006: No new-buffer or close-buffer lifecycle

- Area: TUI buffer management
- Priority: High
- Status: Open; not fixed
- Current behavior:
  - Buffers are created only by opening a file.
  - Commands provide `:bn`/`:bp` but no new empty buffer, close buffer, or
    delete buffer.
  - `:q` quits the editor; `Ctrl-w q` closes only a split pane.
- Required behavior:
  - Create an empty buffer without a path.
  - Close the current buffer, or a named/indexed buffer, with an unsaved-change
    guard.
  - Remove the buffer from split-pane references and keep one valid active
    buffer.
- Acceptance:
  - `new`, close, and switch work with modified and unmodified buffers.
  - Closing a modified buffer requires explicit confirmation or force.
  - No stale split pane points at a removed buffer.

### BUG-007: LSP does not auto-start for supported files

- Area: LSP lifecycle
- Priority: High (policy/UX gap)
- Status: Open; not fixed
- Current behavior:
  - Opening a supported file calls `sync_lsp_active`, but that function only
    syncs an already-running worker.
  - A language server starts only through `:lsp-start` (or crash restart).
  - Users opening a Rust/Go/TypeScript file see no diagnostics until they run
    the command manually.
- Required decision:
  - Choose explicit-start versus auto-start as the default policy.
  - If auto-start is enabled, start once per supported file/language and avoid
    duplicate servers; `:lsp-stop` remains explicit.
  - If explicit-start is intentional, document it prominently.
- Acceptance:
  - The lifecycle is deterministic and does not spawn duplicate servers on
    buffer switches.
  - A crashed server follows the existing bounded restart policy.
  - The status/help text matches the selected policy.

### BUG-008: External file changes are not watched

- Area: Workspace/file lifecycle
- Priority: High
- Status: Open; not fixed
- Current behavior:
  - `FileWorker` only lists directories/files on explicit requests.
  - No filesystem watcher refreshes the tree or open buffers after an external
    edit, create, rename, or delete.
  - A buffer can display stale contents while the disk has changed.
- Required behavior:
  - Detect external changes and mark or reload affected buffers safely.
  - Refresh the tree without dropping the selected entry or expanded state.
  - Never overwrite dirty in-memory edits silently.
- Acceptance:
  - External create/delete/rename updates the tree.
  - External change to a clean open buffer is surfaced or reloaded.
  - External change to a dirty buffer prompts or marks a conflict; it never
    discards local edits.

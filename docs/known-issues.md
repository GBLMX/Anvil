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

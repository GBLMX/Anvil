# ADR-0054: File Picker Query, Ranking, and Index Lifecycle

- Status: Accepted; implemented 2026-11-19
- Version: 1.0.0
- Implementation: `FileQuery` matching/ranking, cached per-root indexing,
  generation-safe previews, syntax preview, and picker actions landed;
  `Ctrl-s` horizontal split remains deferred by the vertical-only split model.
- Date: 2026-11-18
- Related: [ADR-0040](0040-helix-keymap-compatibility.md),
  [ADR-0046](0046-full-workspace-watch.md),
  [ADR-0047](0047-file-tree-navigation-and-refresh.md),
  [ADR-0050](0050-discovery-first-language-packs.md),
  [ADR-0051](0051-external-editor-reference-models.md)
- Supersedes:
- Superseded by:

## Context

`Space f` is Anvil's workspace file picker. It already has the right high-level
shape:

- it is separate from the file tree;
- it enumerates trusted workspace roots through gateway file workers;
- it supports multiple roots with root-qualified display paths;
- it loads previews asynchronously;
- it records frecency when a file is opened.

Its matching semantics are weaker than its product name suggests. The current
filter is a case-folded substring test:

```text
entry.to_lowercase().contains(query.to_lowercase())
```

That has concrete consequences:

- `abc` does not match `a/b/c.rs`;
- spaces are literal, not fzf-style AND terms;
- there is no exact/prefix/suffix/negation syntax;
- matching and sorting scan every entry synchronously on each keystroke;
- results have no explicit cap or truncation state;
- preview is plain text with no syntax or scroll state;
- the only action is open in the current pane.

The file list is also requested again on every `Space f` open. The existing
watcher infrastructure can invalidate an index, but the picker does not yet own
an indexed file-list lifetime.

External editors show the useful boundary:

- fzf's extended search supports space-separated terms, exact/wild/negation
  syntax, smart-case matching, path-specific scoring, and explicit tiebreaks.
- Helix pickers use an fzf-compatible query subset and keep picker navigation
  separate from matching. Its global search picker intentionally uses regex,
  while file pickers use fuzzy matching.
- Telescope separates pickers, sorters, and previewers, and recommends a
  native sorter for large result sets.
- fzf-lua provides resume, combined pickers, and multiple previewers, showing
  the value of persistent picker state and reusable primitives.
- VS Code Quick Open is a file-name-first navigation surface; symbols and line
  navigation are separate commands.

Anvil should borrow the query and lifecycle patterns without importing fzf,
Telescope, or a new runtime picker framework.

## Decision

### 1. Keep `Space f` a file picker

`Space f` remains the workspace file picker. It is not the file tree, live
grep, workspace symbol search, or a general command palette. Those surfaces
keep their existing commands and algorithms.

The default keybinding and existing successful-open behavior remain unchanged.

### 2. Introduce a bounded `FileQuery` syntax

Extract query parsing and matching from the overlay state into a
frontend-neutral `FileQuery`:

```text
FileQuery {
  raw
  terms
  smart_case
}
```

The first implementation supports:

| Syntax | Meaning |
|---|---|
| `foo bar` | AND of both terms |
| `Foo` | smart-case: contains an uppercase character, so case-sensitive |
| `'foo` | exact substring |
| `^foo` | prefix match |
| `foo$` | suffix match |
| `!foo` | exclusion |

Matching is performed across path segments, so `abc` can match `a/b/c.rs`.
Match ranges for every matched term are returned to the renderer; highlighting
must not be limited to one contiguous substring.

Not implemented in this ADR:

- OR expressions;
- regular expressions;
- `@symbol`, `:line`, or language-server queries;
- column-specific filters such as `%path`;
- a user-configurable query grammar.

Those require separate product decisions and tests.

### 3. Use path-aware deterministic ranking

The matcher returns a score and a stable tiebreak. Ranking order:

```text
exact basename
  > basename prefix
  > basename token
  > path segment token
  > contiguous path match
  > subsequence path match
  > frecency
  > shorter path / stable path order
```

Frecency is a tie-break, not a substitute for a good path match. The empty
query shows recent files first, then stable path order. Ranking is pure and
must not depend on the currently selected row or render state.

The initial result limit is `FILE_PICKER_RESULT_LIMIT = 1000`. The picker
reports `truncated` when more matches exist so the user can narrow the query.
If measurements show a different limit is necessary, the constant and the
matching budget change together in one review.

### 4. Own a per-root file index

Each trusted root gets a file-list cache with a generation:

```text
root path -> entries, generation, last refresh
```

Rules:

- opening `Space f` shows the cached list immediately when present;
- a background refresh starts for stale roots;
- a watcher event invalidates only the affected root;
- untrusted or removed roots are deleted from the picker index;
- a stale response with an older generation is discarded;
- results from multiple roots are merged, root-qualified, sorted, and
  deduplicated after matching.

The existing gateway/file-worker boundary remains the only file enumeration
path. The picker does not read the filesystem directly.

Query matching is extracted so it can move to a worker if the large-workspace
benchmark exceeds the interactive budget. The ADR requires the observable
behavior; it does not require a new thread implementation immediately.

### 5. Coalesce preview work

Preview remains asynchronous and generation-checked. Changes:

- selection changes request a preview for the current row, not every transient
  intermediate row during rapid movement;
- stale preview results are ignored by generation;
- only the latest pending preview should be allowed to occupy the worker queue;
- preview has its own loading, truncated, and error state and never blocks the
  list.

Preview gains:

- syntax highlighting from the existing syntax cache;
- line numbers;
- vertical scrolling;
- file size and modification metadata when available;
- a clear binary/large-file/truncated message.

No external previewer process is introduced.

### 6. Add picker-local actions without changing other pickers

The fuzzy picker gets its own key hints and actions:

| Key | Action |
|---|---|
| `Enter` | open in the current pane |
| `Ctrl-v` | open in a vertical split |
| `Ctrl-s` | horizontal split (deferred; the split model is vertical-only) |
| `Ctrl-t` | open in a new buffer/tab |
| `Ctrl-h` | toggle hidden files |
| `Ctrl-r` | recent-files mode |
| `Ctrl-y` | copy selected path |
| `Esc` | cancel |

Actions are implemented in the fuzzy picker's key path; they must not silently
change `BufferPicker`, diagnostics, command palette, or other overlays.

Result count, truncated state, active root, loading state, and key hints are
visible in the picker chrome. A query can be resumed for the same workspace,
but cross-workspace query resume is deferred.

Multi-select and bulk actions are deferred until there is a concrete workflow
that cannot use single-file opening.

### 7. Preserve the existing compatibility boundary

- `Space f` remains the keybinding; existing keymap aliases continue to work.
- Opening a file still records frecency and jump history.
- Root display labels remain stable.
- Hidden files follow the existing tree visibility setting at open time and
  can be toggled from the picker.
- Untrusted roots never enter the index.
- No shell command, ripgrep process, fzf process, or plugin is added to the
  default path.
- No new fuzzy-matching dependency is added until a benchmark demonstrates that
  the pure implementation is insufficient.

## Consequences

Positive:

- `Space f` becomes genuinely useful for non-contiguous paths;
- query behavior is explicit and testable;
- large workspaces avoid repeated full enumeration and unbounded result sets;
- preview and matching can be made responsive without redesigning the overlay;
- file opening actions no longer require leaving the picker;
- the query matcher can be reused by other pickers later without forcing the
  same policy on global search or symbols.

Negative:

- a query parser and ranking implementation must be maintained;
- per-root cache invalidation adds lifecycle state;
- preview coalescing adds generation and queue semantics;
- split/new-buffer actions expand the picker keymap;
- a dynamic result limit may need tuning after benchmarks.

## Verification

Unit and integration tests:

- `abc` matches `a/b/c.rs`;
- `foo bar` requires both terms;
- `'foo`, `^src/`, `foo.rs$`, and `!test` behave as specified;
- smart-case behavior;
- multiple match ranges are rendered, including Unicode paths;
- multiple roots merge, deduplicate, and preserve root labels;
- cache reuse, watcher invalidation, and stale-generation rejection;
- untrusted/removed roots never appear;
- rapid query changes and rapid selection movement do not let an old preview
  overwrite the current row;
- hidden toggle invalidates or refreshes the right root cache;
- empty query shows recent files first;
- result limits report truncation;
- open current, split, and new-buffer actions each open the selected path;
- no shell/exec path is used for enumeration or preview.

Performance checks:

- synthetic 100k-file workspace;
- query keystroke latency and result-limit behavior;
- rapid navigation with preview disabled and enabled;
- memory use of the per-root index and filtered results.

Manual checks:

- open a multi-root workspace and confirm root badges and results;
- invalidate the index by creating/deleting a file and reopening the picker;
- open a huge/binary/unreadable file in preview and confirm the list remains
  usable;
- verify `Space f` continues to work when no gateway is available and reports
  the existing fallback behavior clearly.

## References

- [fzf manual](https://manpages.debian.org/testing/fzf/fzf.1.en.html)
  — extended search, smart-case, path scoring, tiebreak, and algorithm
  behavior used to define the initial query subset.
- [fzf search syntax](https://github.com/junegunn/fzf#search-syntax)
  — upstream description of exact, prefix, suffix, negation, and space-term
  matching.
- [Helix pickers](https://docs.helix-editor.com/master/pickers.html)
  — fzf-style picker filtering, picker-specific keymaps, and the distinction
  between file pickers and regex/global-search or symbol pickers.
- [Telescope README](https://github.com/nvim-telescope/telescope.nvim)
  — separation of pickers, sorters, and previewers, plus native sorter
  performance guidance.
- [fzf-lua README](https://github.com/ibhagwan/fzf-lua)
  — picker resume, combined pickers, and previewer/action patterns.
- [VS Code Code Navigation](https://code.visualstudio.com/docs/src/editing/editingevolved)
  — Quick Open as a file-name-first surface separate from symbol navigation.
- [ADR-0040](0040-helix-keymap-compatibility.md)
  — `Space f` is the Helix-compatible file picker binding.
- [ADR-0046](0046-full-workspace-watch.md)
  — watcher coverage used for file-index invalidation.
- [ADR-0047](0047-file-tree-navigation-and-refresh.md)
  — keeps the file picker separate from tree-local find and tree navigation.
- [ADR-0051](0051-external-editor-reference-models.md)
  — reference-model and anti-copy boundary for picker UX.

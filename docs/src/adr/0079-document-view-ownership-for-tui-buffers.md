# ADR-0079: Document and View Ownership for TUI Buffers

- Status: Accepted
- Version: 1.0.0
- Date: 2026-11-26
- Related: [ADR-0010](0010-cursor-and-snapshot-ownership.md),
  [ADR-0024](0024-tui-state-and-domain-boundaries.md),
  [ADR-0026](0026-persistent-undo-decision.md),
  [ADR-0041](0041-tui-primitive-extraction.md),
  [ADR-0042](0042-selection-first-session-frontend.md),
  [ADR-0057](0057-tui-surface-model-and-focus-contract.md),
  [ADR-0063](0063-command-architecture-and-execution-boundary.md),
  [ADR-0074](0074-anvil-tui-state-decomposition-and-crate-boundary.md)
- Supersedes:
- Superseded by:

## Context

`anvil-tui` currently uses one `Buffer` value for two different contracts:

- the document: `DocumentSession`, source path, unsaved state, missing state,
  and syntax state;
- the view: cursor, scroll offsets, visual selection anchor, and search query.

`BufferState` stores those values in `entries`, `split`, and `background`
buffers. `BufferState::active_id` identifies the active document, while
`BufferState::active` identifies the active entry. `Pane` is only a wrapper
around another `Buffer`.

`DocumentSession` itself currently wraps `EditorState`; `EditorState` owns the
text, line index, revision, undo/redo, and also viewport and selections
(`crates/anvil-session/src/lib.rs:72-75`,
`crates/anvil-editor/src/lib.rs:149-160`). The source path and
`modified`/`missing` flags live outside that session, in `Buffer` and
`FileState` (`crates/anvil-tui/src/state.rs:26-44`,
`crates/anvil-tui/src/state.rs:887-909`).

The split implementation exposes the failure mode directly. Opening a split
clones the current `Buffer` into a second pane
(`crates/anvil-tui/src/splits.rs:21-23`), and changing focus swaps the two
snapshots (`:68-73`). If both panes are intended to show the same file, editing
one pane advances only that pane's document state. Saving either pane can then
overwrite the other pane's changes.

The duplicate metadata has already produced compensating logic:

- `path_is_dirty` checks the active buffer, every entry, the background map, and
  the split separately (`crates/anvil-tui/src/buffer.rs:737-752`);
- `any_modified` repeats the same special cases for quit checks
  (`crates/anvil-tui/src/buffer.rs:754-772`);
- `FileState::modified` is written alongside per-buffer `modified` fields
  throughout editing and file operations.

`FileState::dirty` is a different flag: it only means "the next loop iteration
should render" (`crates/anvil-tui/src/state.rs:895-896`). It must not become a
second unsaved-changes source.

Any extraction that moves `DocumentSession` out of `Buffer` but leaves `path`,
`modified`, or `missing` behind recreates the same split-brain ownership. The
document boundary must move those fields with the text and undo stack.

## Decision

### 1. A document owns document state

Introduce a document record with one stable identity:

```text
Document
  id: DocumentId
  session: DocumentSession
  path: Option<PathBuf>
  modified: bool
  missing: bool
```

The `DocumentSession` owns text, line index, revision, undo, and redo. `path`,
`modified`, and `missing` move to the document. No view owns a second copy of
those fields.

`modified` is the only source of truth for unsaved changes. `FileState::modified`
may remain temporarily as a projection of the active document, but command
logic must not write it independently. `path_is_dirty` and `any_modified`
query the document store, not active/entry/background/split copies.

`FileState::dirty` remains render invalidation. It is not renamed in this
decision, but call sites must not use it to decide whether a document is
modified. A later mechanical rename to `render_dirty` is allowed.

Document-level operations include:

- save, save-as, reload, and external-change detection;
- undo and redo;
- LSP `didOpen`, `didChange`, and `didSave`;
- close, quit, and conflict checks;
- background documents used by workspace edits.

A document stays alive while at least one owner references it: one or more
views, a `background_documents` entry, or an in-flight save/conflict
operation. Closing the last view unloads the document only when none of those
other owners remain; otherwise the document survives as a background document.
Background entries are first-class owners, not unreferenced caches.

### 2. A view owns view state

Introduce a view record that references a document:

```text
View
  id: ViewId
  document: DocumentId
  cursor: usize
  selections: ...
  scroll: usize
  hscroll: usize
  visual_anchor: Option<usize>
  visual_line: bool
  search_query: String
```

Cursor, selection, scroll, visual mode, and search state are view-local. Two
views of the same document can therefore have independent cursors and
selections while sharing text, revision, undo, path, and modified state.

`EditorState`'s `viewport` and `selections` are execution state for the active
view. When focus changes, the outgoing view saves its cursor/selection state and
the incoming view restores its own before a command runs. They are not a second
document-wide selection store. `DocumentSession` remains the frontend-neutral
command surface and does not gain ratatui/crossterm state.

The syntax/highlight cache is keyed by document identity and revision. It may
live on the document or in a document-indexed cache; it must not be duplicated
as independent per-view document text or undo state.

### 3. Buffer state becomes document and view stores

Replace the current `Buffer`/`Pane` shape with:

```text
DocumentStore: DocumentId -> Document
ViewStore:     ViewId     -> View
BufferState:
  documents
  views
  active_view
  split_view
  background_documents
```

`BufferId` is renamed to `DocumentId`. It must not remain a second document
identity alongside the new type. `Pane` holds a `View`, not a
cloned document snapshot. Opening a split creates a new view with the same
`document: DocumentId`; it never clones `DocumentSession`, `path`, or
`modified`.

This is the smallest change that gives split, tab, and future multi-pane
surfaces the same semantics as one file open in multiple views. It does not
require a new crate and does not change the text model.

### 4. Preserve the command and frontend boundaries

The document/view split stays inside `anvil-tui` and the existing
`DocumentSession` contract. It does not:

- change `anvil-session` into a TUI state owner;
- add a new `impl Model` for document behavior (ADR-0074);
- change the command IDs or `EffectQueue` (ADR-0063);
- change the text backend selected by ADR-0009 and ADR-0077;
- change persistent-undo policy (ADR-0026).

The active document supplies the command execution context. A command that
edits text applies one document revision and one undo record regardless of how
many views display it. A command that moves a view only mutates that view.

### 5. Migration guardrails

- Move `path`, `modified`, and `missing` with `DocumentSession` in the same
  slice that introduces `Document`. Do not create a document record that leaves
  them in `Buffer` or `FileState`.
- Do not keep a compatibility `Buffer` containing `DocumentSession` plus a
  `Document` containing the same session. Migrate callers to one owner.
- Do not change the text model, keymap, or event loop in this refactor.
- Do not make session restore serialize two copies of document metadata. The
  restored document is the source; views refer to its identity.
- A split PR must include a live shared-document test, not only session-restore
  coverage.

## Consequences

Positive:

- Split panes remain synchronized on text, revision, save state, and undo.
- Save, quit, reload, and LSP callbacks run once per document, not once per
  duplicated buffer snapshot.
- Path/modified/missing checks no longer need active/split/background special
  cases.
- Multiple views can preserve independent cursors and selections.
- The ownership rule can be tested without a terminal.

Negative:

- The first slice is a structural refactor across `Model`, buffer loading,
  saving, session restore, and tests.
- Active-view selection synchronization adds an explicit focus boundary.
- Session persistence and workspace-edit background documents must be migrated
  together or they will temporarily have two identities.
- Until the migration completes, `FileState::modified` remains a projection and
  must be audited for direct writes.
- More explicit identity types increase type churn in a crate that already has
  many `impl Model` methods.

## Verification

- Focused tests:
  - open one file in a split, edit the left view, and observe the right view's
    text and revision;
  - move and select independently in both views, then edit from each;
  - undo and redo from either view and observe one shared history;
  - save from either view and clear `modified` for every view of that document;
  - close one view, then edit and save the remaining view;
  - close the last view and unload the document.
- Failure/cancellation tests:
  - a failed save preserves `modified`;
  - closing a dirty view is refused while the document has unsaved changes;
  - external file changes and missed files are reported per document;
  - session restore preserves the split, active view, cursor, and dirty
    document without creating duplicate documents.
- Real surface smoke:
  - open the TUI, split a real file, edit in one pane, save from the other, and
    quit with both views open.
- Checks:
  - `cargo test -p anvil-tui --tests`
  - `cargo test --workspace --no-fail-fast`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo fmt --all -- --check`
  - `scripts/architecture-check.sh`
  - `python3 scripts/docs-check.py`

## Alternatives rejected

- **Keep the current split snapshot behavior.** Rejected: it is not a view of
  the same document, and save/focus paths already compensate for duplicated
  documents.
- **Move every field, including cursors, into `Document`.** Rejected: that
  prevents independent views and preserves the current coupling in a different
  struct.
- **Make `FileState` authoritative for path and modified state.** Rejected: it
  describes the active session, not each document; background and inactive
  documents would still need another owner.
- **Introduce a new crate for documents and views.** Rejected by ADR-0074:
  no second consumer or stable non-`Model` boundary exists yet.
- **Change the text model or add persistent snapshots at the same time.**
  Rejected: the split failure is an ownership bug, not evidence that the text
  backend is wrong.

## References

- dte `Buffer`: <https://github.com/craigbarnes/dte/blob/master/src/buffer.h>
  — one buffer owns its views; blocks and change history are document state.
- dte `View`: <https://github.com/craigbarnes/dte/blob/master/src/view.h>
  — cursor, selection, and viewport are view state over a buffer.
- dte `Window`: <https://github.com/craigbarnes/dte/blob/master/src/window.h>
  — multiple views share a window and remain distinct tabs.
- Flow Control `Buffer`: <https://github.com/neurocyte/flow/blob/master/src/buffer/Buffer.zig>
  — the text/undo model is separate from cursor and selection helpers.
- Flow Control `Editor`: <https://github.com/neurocyte/flow/blob/master/src/tui/editor.zig>
  — an editor surface combines a buffer model with view-local cursor state.

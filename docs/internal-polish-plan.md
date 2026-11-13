# Internal Polish Plan

Status: active, P0 and P1 complete. Version is frozen at **0.6.0**. This is a long
internal-polish phase, not a public release line.

The project keeps landing code and docs, but does not bump the version, create
release tags, publish GitHub releases, or open a marketplace during this phase.
Public distribution work resumes only after the exit gates below are accepted.

## 1. Rules

- `Cargo.toml` workspace version remains `0.6.0`. The freeze check in CI fails
  if it changes accidentally.
- `CHANGELOG.md` keeps accumulating under `[Unreleased]`. Do not add a new
  version heading for internal work.
- Do not create `v0.6.x` tags or run the release workflow. Release secrets stay
  outside the repository.
- The plugin API, gateway protocol, plugin index schema, and settings schema
  remain internal contracts. They may change without a public migration.
- Security fixes still land on `main`; they do not imply a public release.
- Every phase still follows the normal verification gates: tests, clippy, fmt,
  Go tests/vet, shell/YAML checks, and a real smoke test for changed surfaces.

## 2. Current baseline

Already landed:

- selection-first editing, multi-cursor, undo/redo, registers, operators, text
  objects, splits, tabs, and buffers;
- command palette, which-key, completion, fuzzy open, previews, search/replace,
  notifications, problems, themes, and persisted theme choice;
- LSP diagnostics, completion, definition, hover, symbols, code actions,
  format, rename, signature help, and inlay hints;
- sandboxed plugin host, signed packages, content-addressed store, installer
  role, installed-plugin manager, official-only release layout, installer, and
  signed release workflow.

Frozen version: `0.6.0`.

## 3. P0: daily-use completeness

These are the only internal-polish priorities until the phase exit gate.

### 3.1 Settings

Status: typed registry and minimal `:settings` overlay landed for
`editor.scroll_margin`, `editor.max_buffers`, `editor.auto_save`,
`search.case_sensitive`, and `lsp.inlay_hints`.
External-file hot reload remains out of scope.

Implemented: typed registry, `:settings` overlay, changed markers, per-row
reset, and save/cancel validation.

Remaining boundary:
- `:config-reload` reloads settings, keymap, and theme atomically at runtime;
  file watching is not supported;
- no new domains without a concrete consumer.

Difficulty: M. Main risk is scope creep; do not build a ten-tab settings clone.

### 3.2 Session safety

Status: buffer/cursor/active-tab restore, tree/ratio layout state, open split
panes with focus, and dirty buffer swap recovery landed. Conflict swaps are
retained until resolved; save/save-as is blocked until resolution. Persistent
undo history is explicitly rejected for this phase.
Landed:
- restore buffers, cursors, active tabs, tree visibility, split ratio, and open
  split panes with focus;
- preserve dirty swaps and refuse to silently discard dirty state.

Rejected for this phase: persistent undo history. Offset-based edit records are
valid only against one exact text state; restore keeps the exact buffer or swap
text and starts a fresh undo stack. See
[ADR-0026](adr/0026-persistent-undo-decision.md).

Difficulty: L–XL. Main risk is data loss and schema migration.

### 3.3 LSP depth

Status: completion ranking and snippet mirror grouping/editing landed; code
lens request/parse, virtual-row rendering, and command execution landed.
Landed:
- completion snippet mirrors and `sortText`/`filterText`/`preselect` ranking;
- code lens request/parse, virtual-row rendering, and command execution;
- references, document highlights, folding ranges, and semantic tokens;
- multi-file workspace edits with stale-revision rejection.

Implemented bounds:

- `MAX_CODE_LENS = 2048` parsed lenses per document.
- `MAX_CODE_LENS_PER_LINE = 3` visible rows per source line, including the
  `… +N lenses` truncation row.
- `MAX_CODE_LENS_ROWS_PER_VIEWPORT = 32`, including any truncation row, and
  never more than half the editor viewport height.
- Folded or hidden source lines never contribute lens rows.
- Truncation is visible; stale or malformed lens responses are ignored.

Code lens is not an inline suffix or popup-only list. Viewport, gutter, cursor
placement, mouse hit-testing, folding, and horizontal scroll must consume the
same source-line-to-display-row map.

Bounds are mandatory before implementation:

- `MAX_CODE_LENS = 2048` parsed lenses per document.
- `MAX_CODE_LENS_PER_LINE = 3` visible rows per source line, including the
  `… +N lenses` truncation row.
- `MAX_CODE_LENS_ROWS_PER_VIEWPORT = 32`, including any truncation row, and
  never more than half the editor viewport height; overflow uses one visible
  `… +N lenses` row.
- Folded or hidden source lines never contribute lens rows.
- Truncation is visible in the lens area, not silent; stale or malformed lens
  responses are ignored.

Difficulty: M–L. Completion mirrors/ranking are medium; code lens is
medium-to-large because above-line rendering requires that display map.

### 3.4 Git

Status: read-only `:git-diff` and `:git-diff --staged`, gutter marks, hunk
navigation, single-hunk stage/unstage, `:git-blame`, `:git-conflicts`, and
read-only base/ours/theirs previews landed. Whole-file
`:git-resolve-ours`/`:git-resolve-theirs` (and `!` variants) land the chosen
stage and `git add` it; worker tests cover stage validation, write, and staging.
Both-side and hunk-level conflict resolution are not supported.
- Keep gutter marks as a derived view, not the only Git surface.

Difficulty: M–L for whole-file resolution. Git index mutations remain a
data-loss surface.

### 3.5 File lifecycle

Status: swap/crash recovery landed. External-change conflicts preserve the
swap and expose `:conflict-recover` / `:conflict-discard`; saving an
unmodified conflicted buffer is refused until resolved.
Auto-save status: the focus-change policy is closed. It covers buffer
switch/close, split focus/close changes, opening another file, terminal focus
loss (including Insert mode), and `:q`; read-only, help, missing, stdin, and
conflicted buffers are skipped.

Landed: swap files, crash recovery, external-change conflict recovery, and
focus-change auto-save.

Auto-save policy:

- Default: `off`.
- First supported mode: `on_focus_change` only (buffer switch, split focus
  change, or leaving the TUI). Do not add an idle-delay mode until the
  focus-change path is dogfooded.
- Never auto-save unnamed/stdin buffers, read-only buffers, help views, missing
  files, or buffers with an unresolved external-change conflict.
- A failed auto-save keeps the buffer dirty, reports `E: auto-save: ...`, and
  does not retry in a loop. The next focus change, edit, or explicit `:w` may
  retry; a failure never discards text or marks the buffer clean.
- Auto-save uses the existing atomic save path, so atomic replacement semantics
  and the owner/ACL/xattr caveat are unchanged.
- Do not auto-save while a modal or blocking prompt owns the event loop.

Difficulty: M–L. Main risk is write races and incorrect merge resolution.

### 3.6 Search

Status: landed. Project search has cancellation, result caps, and stale-result
handling; replace across files is a reviewed transaction.

Difficulty: M. Reuse the gateway/tree layers; do not add a second index until
measurements justify it.

### 3.7 LSP lifecycle

Status: landed. Server discovery, version reporting, restart/backoff, and
visible failure are implemented.

Difficulty: M. Main risk is hidden fallback behavior.

## 4. P1: IDE-like adapters

Status: landed.

Implemented:

- task runner with problem matchers;
- test explorer with jump-to-failure;
- DAP debugger in a separate process;
- terminal panel with explicit workspace/trust boundaries;
- multi-root roots, trust-gated per-root gateways, and open/save routing;
- folding state persistence;
- external control/MCP adapter over `anvil-control`.

Remaining follow-up: cross-root file tree/search/mutations, tracked in
[multi-root-routing-plan.md](multi-root-routing-plan.md).

## 5. Explicitly deferred

- Signed plugin index/anti-rollback runtime.
- Marketplace browse/detail/install UI.
- Third-party publisher onboarding.
- Public module contract and third-party distribution fixture.
- Public release signing/provenance ceremony.
- Windows/macOS support.

These remain documented, but they do not block internal polish.

## 6. Suggested internal order

1. Settings validation/reset/hot-reload boundary.
2. Auto-save and large/binary/read-only file safety.
3. Multi-file search and transactional replace.
4. LSP lifecycle discovery/restart/backoff.
5. Command registry/effect-queue completion.
6. P1 adapters only after P0 dogfood.

## 7. Exit gates

Internal polish ends only when all are true:

- P0 features are complete and dogfooded.
- No known P0 data-loss, corruption, or security issue is open.
- Session restore/crash recovery has destructive-failure tests.
- Settings, LSP, Git, and file lifecycle have failure/cancellation tests.
- Performance baseline is re-run on the reference machine.
- Release layout, installer, plugin bootstrap, and release-signing dry run are
  verified with production-like keys.
- The next public version number is chosen explicitly; the frozen-version check
  is then updated in the same commit as the version bump.

## 8. Verification

Every slice must run:

```sh
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
go test ./go/...
go vet ./go/...
```

Changed UI surfaces require a real TUI smoke test; changed distribution
surfaces require an install-layout smoke test.

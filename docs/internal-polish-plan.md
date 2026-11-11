# Internal Polish Plan

Status: active. Version is frozen at **0.6.0**. This is a long internal-polish
phase, not a public release line.

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
`editor.scroll_margin`, `search.case_sensitive`, and `lsp.inlay_hints`.
External-file hot reload remains out of scope.

- Add a typed settings registry with explicit domains, defaults, path, and
  precedence.
- Extend `settings.toml` beyond `[theme]`.
- Add a built-in Settings overlay only after at least three independent domains
  exist.
- Add validation errors, changed markers, reset, and a documented hot-reload
  boundary.

Difficulty: M. Main risk is scope creep; do not build a ten-tab settings clone.

### 3.2 Session safety

Status: buffer/cursor/active-tab restore, tree/ratio layout state, open split
panes with focus, and dirty buffer swap recovery landed. Conflict swaps are
retained until resolved; save/save-as is blocked until resolution. Persistent
undo history is explicitly rejected for this phase.
- Restore buffers, cursors, active tabs, tree visibility, split ratio, and open
  split panes with focus.
- Persistent undo history is rejected: offset-based edit records are valid only
  against one exact text state. Restore keeps the exact buffer or swap text and
  starts a fresh undo stack; a second validated replay format and unbounded
  session JSON are not justified.
- Never silently discard dirty state.

Difficulty: L–XL. Main risk is data loss and schema migration.

### 3.3 LSP depth

Status: completion ranking and snippet mirror grouping/editing landed; code
lens request/parse, virtual-row rendering, and command execution landed.
- Completion snippet mirrors: repeated tabstop indices survive parsing, group
  as mirrors, navigate as one unique stop, and receive typed insertions and
  deletions together.
- `sortText`/`filterText`/`preselect` ranking landed.
- Code lens: request/parse bounded to 2048 total and 3 visible rows per line;
  per-line and viewport truncation are visible; rows participate in viewport,
  gutter, cursor, mouse, and folding mapping. Clicking executes
  `workspace/executeCommand`.
- References, document highlights, folding ranges, and semantic tokens are
  landed.
- Multi-file workspace edits with stale-revision rejection are landed.

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
read-only base/ours/theirs previews landed. Conflict resolution operations remain.
- Keep gutter marks as a derived view, not the only Git surface.

Difficulty: XL. Git index mutations and conflict resolution are data-loss
surfaces.

### 3.5 File lifecycle

Status: swap/crash recovery landed. External-change conflicts preserve the
swap and expose `:conflict-recover` / `:conflict-discard`; saving an
unmodified conflicted buffer is refused until resolved.
- Auto-save with an explicit policy.
- Swap files and crash recovery integration.
- External-change conflict resolution.
- Large/binary/read-only safe behavior.

Difficulty: M–L. Main risk is write races and incorrect merge resolution.

### 3.6 Search

- Multi-file/project search with cancellation, result caps, and stale-result
  handling.
- Replace across files as one reviewable transaction.

Difficulty: M. Reuse the gateway/tree layers; do not add a second index until
measurements justify it.

### 3.7 LSP lifecycle

- Server discovery, version reporting, restart/backoff, and visible failure.
- Optional bundled servers remain a separate distribution decision.

Difficulty: M. Main risk is hidden fallback behavior.

## 4. P1: IDE-like adapters

Only start after P0 is complete and dogfooded:

- task runner with problem matchers;
- test explorer with jump-to-failure;
- DAP debugger in a separate process;
- terminal panel with explicit workspace/trust boundaries;
- multi-root workspaces with per-root gateway and trust policy;
- folding state persistence.
- external control/MCP adapter over `anvil-control`; no second transport and no
  direct workspace file access (see
  [external-integration-plan.md](external-integration-plan.md)).

Difficulty: L–XL. DAP, terminal, and multi-root are architecture-level work and
must not be folded into `anvil-session`.

## 5. Explicitly deferred

- Signed plugin index/anti-rollback runtime.
- Marketplace browse/detail/install UI.
- Third-party publisher onboarding.
- Public module contract and third-party distribution fixture.
- Public release signing/provenance ceremony.
- Windows/macOS support.

These remain documented, but they do not block internal polish.

## 6. Suggested internal order

1. LSP completion snippet mirrors and ranking.
2. LSP code lens with above-line virtual rows.
3. Session restore and crash backup.
4. Persistent undo decision: fresh stack after restore.
5. Git diff/hunk/blame/conflict.
6. Auto-save, swap, and external-conflict resolution.
7. P1 adapters only after P0 dogfood.

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

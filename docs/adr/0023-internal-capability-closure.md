# ADR-0023: Internal Capability Closure Before Public Distribution

- Status: Proposed
- Date: 2026-11-12
- Related: [ADR-0001](0001-plugin-isolation.md), [ADR-0012](0012-plugin-host-isolation.md), [ADR-0013](0013-kernel-distribution-boundary.md), [ADR-0021](0021-signed-plugin-index.md), [ADR-0022](0022-core-editing-completion.md)

## Context

The internal-polish plan says P0 and P1 are complete, but several user-visible
loops are still half-wired. They are not missing modules; they are missing
routing, policy, failure behavior, or a final UI step.

Examples:

- **Multi-root files**: `Model` owns `root_file_workers`, but the main poll
  loop only polls the primary `file_worker`. Quick-open and project search
  still send requests through the primary worker. The file tree still assumes
  one relative namespace. Cross-root rename/copy is rejected. The multi-root
  plan is still marked proposed even though root registration and open/save
  routing exist.
- **File lifecycle**: auto-save has a documented policy but no implementation.
  External settings hot reload is explicitly unsupported. Persistent undo is
  explicitly rejected for this phase.
- **Plugin distribution**: manifest `onCommand` dispatch works, but dynamic
  `editor.register_command` is pending. The signed index/revocation verifier
  and cache exist, but marketplace browse/detail, indexed install, consent,
  update, and revocation/provenance UI do not. Index-to-download integration is
  pending. Production signing keys are external.
- **Plugin sandbox**: `LANDLOCK_SCOPE_SIGNAL` is enabled on Landlock ABI >= 6;
  older kernels still rely on seccomp as the primary signal control. The threat
  model tracks this as T9/T10.
- **Verification**: the full performance gate has not run on the reference
  machine, and release signing/provenance has not been exercised with
  production-like keys.

Calling these "landed" because the underlying module exists makes the
documentation overstate the product. This ADR defines what "closed" means and
what must happen before marketplace or public-distribution claims.

## Decision

### 1. Define closure as a user-visible, tested loop

A capability is closed only when all of the following exist:

- one owner and one routing rule for every input path;
- an explicit failure/degraded state;
- bounded resource use;
- tests for success, cancellation, and failure;
- documentation that matches the implemented behavior.

A created worker, parsed document, or cached index is not a closed capability.

### 2. Close multi-root file operations first

- Use one `FileWorker` per trusted root. Every live worker must be polled.
- All paths crossing the model boundary are absolute; events carry their
  owning root.
- File tree list/expand/watch, quick-open enumeration and preview, project
  search/cancellation, and create/rename/remove/copy route through the owning
  root.
- Merge fan-out results with global caps; deduplicate absolute paths; label
  non-primary roots as `<root-name>/<relative>`.
- Untrusted roots have no worker, no watch, and no hidden fallback. The UI
  shows the trust action.
- Cross-root rename/copy remains rejected. Remote workspaces remain out of
  scope.
- LSP remains primary-root-only for this decision. The UI and docs must say so;
  per-root LSP requires a later ADR and measurements. Do not claim full
  multi-root IDE support before then.

### 3. Close file lifecycle explicitly

**Auto-save**

- Default `off`.
- First mode is `on_focus_change` only: buffer switch, split focus change, or
  leaving the TUI. No idle-timer mode in this phase.
- Never auto-save unnamed/stdin buffers, read-only buffers, help views, missing
  files, or buffers with an unresolved external-change conflict.
- Use the existing atomic save path. A failure keeps the buffer dirty, reports
  `E: auto-save: ...`, and does not retry in a loop.
- Do not auto-save while a modal or blocking prompt owns the event loop.
- Permission/ownership caveats of atomic replacement remain unchanged.

**Settings reload**

- Add an explicit `:config-reload` for settings, keymap, and theme.
- Keep the last good configuration when parsing or validation fails; show the
  error and do not partially apply a broken file.
- Do not restart LSP, plugins, or gateways as part of config reload.
- External file watching is optional and must not be the only way to reload.
- Invalid values fail the whole reload, not just the invalid row.

**Persistent undo**

- Remains rejected for this phase. Restore keeps the exact buffer or swap text
  and starts a fresh undo stack. Persisting undo requires a separate ADR.

### 4. Close plugin/distribution in dependency order

1. Implement dynamic `editor.register_command` and prove one plugin command
   works without a manifest `onCommand` entry.
2. Implement index-to-download integration over the signed cached index.
3. Implement marketplace browse/detail and indexed install only after dynamic
   commands, signed index, consent, and revocation checks exist.
4. Consent must show added/removed file roots, hosts, executables, API
   compatibility, and signature/source class.
5. Update/install is atomic. No automatic activation after update. A failed
   verification or rename leaves the prior version usable.
6. Revocation checks must disable revoked installed packages and block revoked
   installs, including offline cached revocations.
7. No marketplace package enters the index until release signing/provenance
   passes with production-like keys.
8. Local signed install remains available without marketplace or network.

Marketplace work stays deferred until the internal-polish exit gates pass. The
existing signed index verifier/cache is a building block, not a shipped
marketplace.

### 5. Make the security gap explicit

`LANDLOCK_SCOPE_SIGNAL` is enabled on Landlock ABI >= 6. The closure decision is:

- enable it where the kernel supports it;
- if the kernel lacks support, expose a visible degraded mode and keep the
  seccomp signal controls;
- do not silently claim full signal isolation on older kernels;
- update the threat model and tests to distinguish full and degraded modes.

Seccomp remains required. Landlock signal scoping is not a replacement for the
existing seccomp denylist.

### 6. No public release before the exit gates

Do not create a release tag, publish a marketplace index, or claim a stable
distribution until:

- two trusted roots pass file tree, quick-open, project search, and mutation
  routing tests;
- auto-save failure, settings reload failure, and destructive session-restore
  tests pass;
- dynamic plugin commands, consent, revocation, and provenance paths pass;
- the performance baseline is re-run on the reference machine;
- release layout, installer, plugin bootstrap, and signing dry run pass with
  production-like keys;
- known-issues and plan statuses match the code.

## Consequences

Positive:

- "Landed" means usable, not merely present in the tree.
- Multi-root stops being a half-wired worker map.
- Auto-save and config reload have explicit, testable failure behavior.
- Marketplace cannot ship before its trust and consent prerequisites.
- The Landlock signal limitation is visible instead of being buried in a
  threat model.

Negative:

- More work is required before marketplace and public-release claims.
- Multi-root remains primary-root-only for LSP in this phase.
- Auto-save supports fewer modes than Helix.
- Config reload is explicit rather than a full file-watcher experience.
- Closing the loop will expose stale status tables and may require
  documentation churn.

## Alternatives considered

### Keep the partial multi-root implementation (rejected)

Starting workers that are never polled creates false confidence, dead state,
and untestable routing. Either close the loop or remove the claim.

### Per-root LSP now (deferred)

Per-root language servers multiply process count, workspace-folder semantics,
and capability routing. This ADR keeps primary-root LSP explicit, not silent.
A later ADR can add per-root sessions after measurement.

### Auto-save on idle and focus change (rejected for this phase)

The focus-change path has fewer write races and is easier to dogfood. Add
idle-delay only after focus-change failure behavior is proven.

### File-watcher-only settings reload (rejected)

A watcher alone gives no explicit retry, no last-good guarantee, and no
consistent error surface. Start with explicit reload; add watching later.

### Ship marketplace before dynamic commands and signing (rejected)

It would create a marketplace that cannot reliably execute plugin commands,
cannot prove publisher identity, and cannot enforce revocation or consent.

### Silently accept the Landlock signal gap (rejected)

The sandbox claim must match the kernel mode. A visible degraded mode is
acceptable; an invisible one is not.

## Verification expectations

- Two-root E2E: both roots render, quick-open labels both, project search
  returns both, create/rename/remove route correctly, untrusted root cannot
  expand, cross-root copy is rejected with an error.
- Auto-save: focus change saves dirty file; stdin/read-only/help/conflicted
  buffers are skipped; failed save stays dirty; no retry loop; atomic save
  semantics hold.
- Config reload: valid settings/keymap/theme apply; invalid values keep the
  last good configuration; LSP/plugin/gateway processes are untouched.
- Plugin: dynamic command registration and replay; consent diff; failed host
  stop; failed update; revoked install; offline revocation cache.
- Security: Landlock signal scope enabled test, or a tested degraded-mode
  report when unavailable.
- Performance and release: reference-machine baseline, release-layout smoke,
  plugin bootstrap, and production-like signing/provenance dry run.
- `cargo test --workspace --no-fail-fast`, clippy, fmt, Go tests/vet, shell/YAML
  checks, and the relevant real TUI/install smoke tests pass.

## Implementation status (2026-11-12)

- Multi-root: trusted-root `FileWorker`s are polled; quick-open enumerates,
  labels, and previews files from every trusted root; project search fans out,
  merges/deduplicates absolute results, and opens secondary-root locations.
  File tree and mutation fan-out remain primary-root-only.
- File lifecycle: auto-save and atomic `:config-reload` are implemented.
  External settings file watching remains explicit; persistent undo is still
  rejected for this phase.
- Plugin distribution: manifest `onCommand` dispatch works; dynamic
  `editor.register_command` remains pending. Marketplace browse/detail,
  indexed install, and revocation/provenance UI remain pending.
- Plugin sandbox: `LANDLOCK_SCOPE_SIGNAL` is enabled on ABI >= 6; older
  kernels keep seccomp as the primary signal control and report no synthetic
  scope success.

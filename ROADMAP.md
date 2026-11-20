# Roadmap

This is a directional roadmap, not a release commitment. The detailed gates stay
in `PROJECT_CHARTER.md`, `docs/phase-0-7-plan.md`, and the ADRs.
Canonical layer/order: [docs/README.md § Delivery hierarchy](docs/README.md#delivery-hierarchy).

## Internal polish mode

Version is frozen at **0.6.0** for long-term internal feature polish.

- Do not bump the version, create release tags, or publish releases.
- Do not add signed-index/marketplace surfaces during this phase.
- Accumulate work under `CHANGELOG.md` `[Unreleased]`.
- Follow [docs/internal-polish-plan.md](docs/internal-polish-plan.md) for P0/P1
  priorities and exit gates.
- `scripts/check-frozen-version.sh` fails CI on an accidental version bump.


## Current

- `0.6.0` internal-polish baseline: the selection-first kernel, session
  extraction, P0/P1 feature sets, multi-root routing, and optional `0.7.x`
  official modules are landed.
- The latest hardening pass closed the open editor, plugin-host, gateway,
  TUI, theme, and icon issues; the workspace gates pass with 1,318 Rust tests
  and 16 explicit ignored/manual tests.
- The full release layout installs and passes `anvil doctor --profile
  installed`; the remaining exit gates are tracked below.

## Remaining internal polish

P0/P1 items are complete. Cross-root file tree/search UX and focus-change
auto-save policy are closed; root labels, secondary-root refresh, untrusted
roots, root-removal cleanup, and auto-save safety guards are landed.

Persistent undo is deferred by
[ADR-0026](docs/adr/0026-persistent-undo-decision.md); revisiting it requires
a new ADR that supersedes that decision.

Completion snippet mirrors/ranking and code lens are landed; see
[docs/internal-polish-plan.md](docs/internal-polish-plan.md).

Detailed difficulty and exit gates are in
[docs/internal-polish-plan.md](docs/internal-polish-plan.md). Marketplace and
signed-index work stay deferred until the polish phase exits.

## Later

- [0.7.x] Official optional Rust modules: source-level only, feature-gated,
  trusted in-process code; see [docs/0.7.x-plan.md](docs/0.7.x-plan.md).
  Composition and two-consumer gate are landed (ADR-0027); the module contract
  and registry are landed (ADR-0028); Git extension points are defined
  (ADR-0029); pure Git helpers and `FileWorker` service injection now live in
  `anvil-mod-git`; TUI startup selection and the `official-git` Cargo feature
  are landed; `:git-*` dispatch and completion are gated without the service.
  `official.git` is registered through `ModuleRegistry` and the installed-binary
  Git parity smoke passes. Workspace index extension points are defined
  (ADR-0030); the bounded index, gateway-backed build/search, cancellation/stale
  handling, module registration, quick-open adapter, opt-in feature
  composition, and installed-binary workspace-index smoke are landed. The
  minimal second consumer, module boundary check, compatibility matrix,
  deprecation policy, and contract v1 freeze close 0.7.3. The 0.7.4 slices for
  opt-in `official.desktop`, `official.language-latex`, and
  `official.language-typst` are landed; optional grammars are feature-gated.
  The remaining candidate pool is optional and unscoped.
- Optional TachyonFX-style UI effects: `anvil-tui` only, non-default feature,
  ADR and performance gate required; see the deferred section in
  [docs/0.5.x-plan.md](docs/0.5.x-plan.md). Not before the `0.5.10` baseline.
- Broader language/tooling support after the kernel boundary is stable.
- Release signing, SBOM, and provenance after release automation exists.
- Signed plugin packages, a bounded download manager, installed-plugin
  management, and a marketplace index/UI; see
  [docs/0.6.x-plan.md](docs/0.6.x-plan.md). Blocked on plugin lifecycle, the
  plugin API v1 freeze, release signing/provenance, and the `0.5.x` feedback UI.
- Optional PGO/BOLT for the official binary only after representative workload
  measurements.
- Windows/macOS remain out of scope for v1.

## Explicit non-goals

- A stable Rust ABI or dynamic Rust plugin loading.
- Vimscript compatibility.
- A plugin runtime inside the editor process.
- A global gateway shared by multiple workspaces.
- Claiming production support before a stable release exists.

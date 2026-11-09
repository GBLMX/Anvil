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

- `0.5.x` feature line after the `0.5.0` session extraction release.
- Stabilize the selection-first kernel and its existing tests.
- Complete the kernel/distribution boundary decision
  ([ADR-0013](docs/adr/0013-kernel-distribution-boundary.md)).
- Keep the interim TUI keymap clearly separated from kernel semantics.
- `0.4.6c` plugin host supervision: single-flight startup, bounded start queue,
  start timeout, crash backoff, explicit disable/re-enable, and drain-before-kill.
- `0.4.7a` cgroup-before-exec resource limits, `pidfd`/`cgroup.kill` stop,
  core dump suppression, and global running/queued-start caps.
- `0.4.7b` idle host reclaim: 60-second idle timeout with drain, token revoke,
  socket/cgroup cleanup, and queued-start capacity release.
- `0.4.7c-a` preconnected editor Unix socket before Landlock/seccomp; the
  post-sandbox transport no longer opens sockets.
- `0.4.7c-b` seccomp denylist for signals, SIGIO, exec/memfd, mounts,
  namespaces, introspection, `clone3`, and post-sandbox sockets.
- `0.4.8a` method catalog and document snapshot/apply-edit DTOs, generated
  JSON, TypeScript, and Lua method bindings.
- `0.4.8b` plugin host gateway preconnect and authenticated `sys.*` forwarding.
- `0.4.8c` editor-side snapshot and revision-checked transaction contract with
  status-bar rejection reporting.
- `0.4.9` rescoped: API-version hello/manifest validation, document session,
  plugin host lifecycle, command registry, focus state, overlay/input state,
  and the bounded effect queue landed.
- `0.5.0` session extraction is complete: command-visible effects route through
  `anvil-session`; the TUI is the input adapter and renderer.

## Next (internal polish)

1. LSP completion snippet mirrors and ranking.
2. LSP code lens, rendered as standard virtual rows above source lines.
3. Session restore, crash backup, and persistent undo decision.
4. Git diff/hunk/blame/conflict surfaces.
5. Auto-save, swap files, and external-conflict resolution.
6. Multi-file/project search and replace.
7. P1 adapters: tasks, tests, DAP, terminal, multi-root, folding persistence.

Detailed difficulty and exit gates are in
[docs/internal-polish-plan.md](docs/internal-polish-plan.md). Marketplace and
signed-index work stay deferred until the polish phase exits.

## Later

- A standard-edition composition crate and a sample third-party distribution.
- [0.7.x] Official optional Rust modules: source-level only, feature-gated,
  trusted in-process code; see [docs/0.7.x-plan.md](docs/0.7.x-plan.md).
  Blocked on `0.4.9`, `0.6.0`, the standard-edition composition crate, and the
  module two-consumer gate.
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

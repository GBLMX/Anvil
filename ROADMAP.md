# Roadmap

This is a directional roadmap, not a release commitment. The detailed gates stay
in `PROJECT_CHARTER.md`, `docs/phase-0-7-plan.md`, and the ADRs.
Canonical layer/order: [docs/README.md § Delivery hierarchy](docs/README.md#delivery-hierarchy).

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

## Next

1. [0.5.2] Generalize `d/y/c` over motions and text objects, including
   inner/outer word, quote, bracket, paragraph, and tag objects.
2. Version the gateway protocol and add compatibility tests.
3. Establish a reproducible kernel performance baseline.
4. Add user keymap/config boundaries without freezing the plugin keymap format.
5. [0.4.4–0.4.8] Split the plugin runtime into gateway capability control,
   manifest/host loading, activation supervisor, resource isolation, and
   plugin I/O/document transactions; 0.4.6c supervision, 0.4.7a resource
   placement, idle reclaim, and seccomp closure are landed, with plugin I/O
   remaining. User-visible plugin commands/notifications additionally require
   the `0.5.x` command-palette and notification/status surface. Design is in
   [ADR-0017](docs/adr/0017-plugin-activation-and-lifecycle.md).
6. [0.4.1] Close the remaining workspace tree watcher validation in
   [#9](https://github.com/GBLMX/Anvil/issues/9); implementation is described
   in [ADR-0018](docs/adr/0018-workspace-tree-watch.md).
7. [0.5.x] Core interaction and modern TUI UI: last-change repeat, operator
   grammar, command palette, quick-open preview, tabs/status, notification and
   problems panels, search/replace, and focus/accessibility; see
   [docs/0.5.x-plan.md](docs/0.5.x-plan.md).
8. [0.4.7–0.6.1] Linux substrate hardening: cgroup attach before exec, pidfd
   lifecycle, atomic install/rollback, `statx` identity, and measured watcher
   backends; see [docs/linux-platform-plan.md](docs/linux-platform-plan.md).
9. [post-0.5] IDE capabilities: completion resolve/snippets, references,
   folding/semantic tokens, search/replace, problems, session restore, Git
   diff/hunks, and separate task/terminal/debug adapters; see
   [docs/ide-capability-plan.md](docs/ide-capability-plan.md).

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

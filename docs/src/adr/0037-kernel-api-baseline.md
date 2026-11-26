# ADR-0037: Kernel API Baseline During Internal Polish

- Status: Accepted 2026-11-13
- Date: 2026-11-13
- Related: [ADR-0013](0013-kernel-distribution-boundary.md),
  [ADR-0024](0024-tui-state-and-domain-boundaries.md),
  [ADR-0028](0028-module-contract.md)

## Context

ADR-0013 defined the kernel/distribution direction but left the kernel API
unfrozen. Since then:

- the module contract v1 is frozen for `0.7.3`;
- `anvil-distro-minimal` is an independent second consumer;
- official modules exist for Git, workspace index, desktop, LaTeX, and Typst;
- optional grammars are feature-gated;
- `anvil-tui` remains the trusted frontend and is not a kernel crate.

The remaining gap is not another extraction. It is deciding which public
surfaces distributions may build against, and which crates remain internal
implementation details.

## Decision

### 1. Kernel baseline

The kernel baseline is source-level and consists of:

| Crate | Baseline surface |
|---|---|
| `anvil-api` | cross-process DTOs, capability and view types |
| `anvil-core` | `TextBuffer`, `TextDocument`, text-model contracts |
| `anvil-editor` | `EditorState`, selection, snapshots, editing commands |
| `anvil-ipc` | JSON-RPC codec and `GatewayClient` wire contract |
| `anvil-session` | frontend-neutral documents, commands, effects, keymap, and plugin lifecycle |
| `anvil-module` | frozen module contract v1 and service traits |

These crates use semver. A source-breaking change to a baseline surface
requires either a new major version or an explicit contract bump. Internal
items remain unstable and may change without a contract bump.

### 2. Non-kernel crates

`anvil-tui` is the UI adapter and renderer. It may depend on the kernel; the
kernel must never depend on it.

`anvil-plugin-host` is an implementation of the runtime-isolation boundary, not
a public plugin ABI. The session may use it internally, but module APIs must not
expose plugin-host implementation types.

`anvil-distro-standard` and `anvil-distro-minimal` compose the kernel. They may
select modules and services; they do not become part of the kernel API.

### 3. Dependency direction

Allowed:

```text
distribution -> module -> ((anvil-session | anvil-api | anvil-core | anvil-editor | anvil-ipc) as needed)
distribution -> anvil-tui
anvil-tui -> kernel
```

Forbidden:

```text
kernel -> anvil-tui
module -> anvil-tui
module -> plugin-host implementation types
```

`anvil-module` may depend on `anvil-session` and `anvil-ipc` for the frozen
contract. A future change should reduce that dependency only if it stays
source-compatible with contract v1 or explicitly bumps the contract.

### 4. Freeze boundary

Frozen:

- `anvil-module` contract v1;
- public service traits registered by official modules;
- `anvil-api` DTOs consumed by external clients;
- `anvil-ipc` request/response and error envelope;
- documented `anvil-core` / `anvil-editor` / `anvil-session` APIs listed as
  baseline.

Not frozen:

- `anvil-tui` modules, state layout, rendering, and key handling;
- private fields and implementation details of kernel crates;
- `anvil-plugin-host` internals;
- Lua/TS plugin runtime ABI;
- Go gateway internals beyond the versioned protocol.

### 5. Change process

A source-breaking kernel baseline change requires:

1. an ADR or amendment naming the affected baseline surface;
2. a contract or major-version change;
3. an update to the official modules and both consumers;
4. a feature-matrix build and dependency-boundary check.

Internal changes that do not touch baseline items do not need this process.

### 6. CI boundary guard

CI must verify:

- kernel crates do not depend on `anvil-tui`;
- official modules do not depend on `anvil-tui`;
- the minimal fixture composes without official modules;
- feature-free, per-module, and all-module builds succeed;
- `anvil-module` contract v1 remains buildable by the official modules.

## Consequences

Positive:

- Modules and distributions have a named surface instead of an implicit one.
- Kernel/TUI direction is enforceable in CI.
- Contract v1 can evolve without freezing the entire monorepo.

Negative:

- Baseline changes require coordination across modules and consumers.
- Some `anvil-session` internals remain visible until a later narrowing.
- The freeze is source-level, not a stable Rust ABI.

## Non-goals

- A stable Rust ABI.
- Freezing `anvil-tui` internals.
- Freezing the Go gateway internals.
- Making trusted Rust modules sandboxed.
- Publishing a marketplace or runtime module ABI.

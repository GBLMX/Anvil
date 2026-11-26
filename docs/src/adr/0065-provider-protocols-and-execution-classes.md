# ADR-0065: Provider Protocols and Execution Classes

- Status: Accepted; provider protocols and execution classes implemented
- Version: 1.0.0
- Date: 2026-11-20
- Related: [ADR-0024](0024-tui-state-and-domain-boundaries.md),
  [ADR-0025](0025-lsp-and-language-breadth.md),
  [ADR-0032](0032-language-tooling-service.md),
  [ADR-0037](0037-kernel-api-baseline.md),
  [ADR-0058](0058-sandbox-capability-profiles-and-trust-boundaries.md)
- Supersedes:
- Superseded by:

## Context

Picker sources, completion sources, formatters, test adapters, and debug
adapters previously had single, TUI-coupled implementations. Adding each one
directly to `anvil-tui::Model` would deepen the UI dependency and make runtime
plugin providers diverge from native providers.

The execution requirements differ: data-only providers need no process,
formatters and tests need bounded one-shot execution, LSP needs a long-lived
gateway-managed connection, and DAP needs a long-lived bidirectional stdio
process with stronger lifecycle ownership.

## Decision

### 1. UI-neutral provider boundary

Provider identities and protocols live in `anvil-session`, not in
`anvil-tui`. Providers do not depend on `ratatui` or `crossterm` and never
receive `&mut Model`.

A provider returns data or a spec. The kernel/core owns selection, insertion,
undo, ranking, cancellation, and lifecycle decisions.

### 2. Multiple providers per protocol

Each protocol supports multiple providers from its introduction. Do not add a
single global provider slot that forces another refactor later.

### 3. Four execution classes

| Class | Used by | Execution |
|---|---|---|
| Data-only | picker, completion | provider returns data; no process |
| One-shot tool | formatter, linter, test runner | bounded `sys.exec.v2` via gateway |
| Long-lived LSP | language servers | existing gateway `lsp.start` |
| Long-lived DAP | debug adapters | trusted direct spawn owned by core |

One-shot execution carries cwd, stdin, timeout, and bounded stdout/stderr.
`cwd` is workspace-relative and root-confined. There is no shell parsing and no
arbitrary environment map in this decision.

### 4. DAP is a bounded exception

DAP adapters are trusted external tools, like language servers. The core DAP
manager owns process creation, process groups, cwd, frame limits, restart, and
shutdown. Providers return adapter specs and never a child process or stdio
handle. Runtime Lua plugins may not provide DAP adapters.

### 5. Failure is explicit

Provider absence, timeout, overflow, cancellation, and malformed output are
reported to the requesting surface. A provider cannot silently substitute
another provider's result or bypass the gateway policy.

## Consequences

Positive:

- Native, module, and runtime plugin providers share UI-neutral contracts.
- Process execution stays bounded and capability-checked.
- DAP has one clear process owner without introducing a generic process broker.

Negative:

- There are multiple execution classes to test and reason about.
- DAP remains a trusted direct-spawn exception, not a sandboxed gateway path.
- Adding an environment-map contract would require a separate policy decision.

## Verification

- Provider protocol tests in `anvil-session` cover registration, removal,
  duplicate identities, cancellation, and malformed results.
- One-shot execution tests cover cwd confinement, bounded stdin/stdout/stderr,
  timeout, and non-shell argv handling.
- LSP routing remains root-scoped under ADR-0048.
- DAP tests cover process-group shutdown, frame caps, and unresponsive adapter
  cleanup.
- Runtime plugin provider tests cover data-only capability checks.

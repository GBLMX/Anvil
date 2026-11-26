# ADR-0070: IDE Extension Contributions and Execution Trust

- Status: Accepted; descriptor and grant contract implemented
- Version: 1.0.0
- Date: 2026-11-23
- Related: [ADR-0001](0001-plugin-isolation.md),
  [ADR-0005](0005-exec-policy.md),
  [ADR-0012](0012-plugin-host-isolation.md),
  [ADR-0014](0014-plugin-api-v1-freeze.md),
  [ADR-0017](0017-plugin-activation-and-lifecycle.md),
  [ADR-0021](0021-signed-plugin-index.md),
  [ADR-0028](0028-module-contract.md),
  [ADR-0034](0034-release-signing-authority.md),
  [ADR-0037](0037-kernel-api-baseline.md),
  [ADR-0038](0038-project-settings-trust.md),
  [ADR-0048](0048-per-root-language-tooling-and-lsp-workspace.md),
  [ADR-0058](0058-sandbox-capability-profiles-and-trust-boundaries.md),
  [ADR-0060](0060-release-checksum-signing-authority.md),
  [ADR-0063](0063-command-architecture-and-execution-boundary.md),
  [ADR-0065](0065-provider-protocols-and-execution-classes.md),
  [ADR-0067](0067-settings-file-hot-reload.md)
- Supersedes:
- Superseded by:

## Context

Anvil already has two extension mechanisms with different trust models:

- runtime Lua/TS plugins run in the sandboxed plugin host;
- source-level Rust modules are compile-time, trusted, and feature-gated.

The runtime plugin API already exposes capability-gated `sys.exec` and
`sys.exec.v2`. This ADR does not remove those existing bounded one-shot
capabilities; it narrows what runtime plugins may contribute as IDE providers
and descriptors.

ADR-0065 allows runtime plugins to provide data-only picker and completion
results, but rejects runtime formatter and DAP providers. Formatters and test
runners use bounded one-shot gateway execution; language servers use the
gateway LSP path; DAP adapters are a trusted direct-spawn exception owned by
the core.

The [IDE platform plan](../plans/active/ide-platform-plan.md) needs more integrations:
language servers, formatters,
debug adapters, build targets, and task definitions. A manifest that merely
declares one of those commands must not become an execution grant. Without an
explicit boundary, a plugin manifest becomes an arbitrary-command launcher and
defeats the existing plugin host, workspace trust, and gateway policy.

The current repository has the pieces but not one extension-level contract:

- the plugin manifest has commands and activation events, but no closed
  capability vocabulary or execution grant;
- the gateway enforces argv/cwd policy for one-shot execution, but not
  extension identity, manifest revision, or per-root grants;
- workspace settings are trust-gated, but there is no user-facing permission
  record for external tools;
- runtime plugins cannot provide formatter/DAP providers, but there is no
  explicit declaration-time rejection of those capabilities.

The failure mode is concrete: a plugin or copied workspace config declares
`formatter = "sh -c ..."`, the core launches it, and the user has granted
execution without seeing or approving the command.

## Decision

### 1. Three extension classes

The following classes are distinct and must not be collapsed:

| Class | Trust | May contribute |
|---|---|---|
| Runtime plugin | Sandboxed, untrusted | Commands, picker/completion sources, and existing capability-gated `sys.exec` / `sys.exec.v2` |
| Native module | Official source, compile-time | Typed in-process services, commands, and declarative tool descriptors |
| Trusted tool descriptor | Declarative configuration, explicit user grant | LSP, formatter, DAP, build, and task descriptors |

A runtime plugin manifest may not contain formatter, DAP, LSP, or build
execution descriptors. Those descriptors are not a plugin capability.

### 2. Declarative descriptors are not authority

A tool descriptor is metadata:

```toml
id = "rust-analyzer"
kind = "lsp"
program = "/usr/bin/rust-analyzer"
args = []
cwd = "root"
env = ["PATH", "RUSTUP_HOME", "CARGO_HOME"]
languages = ["rust"]
```

The descriptor does not execute anything. It becomes executable only after the
trust chain in §3 completes and the gateway validates the resolved command in
§4.

For the first implementation, descriptors may come only from:

- a native official module;
- trusted user settings;
- trusted workspace settings.

A future third-party descriptor pack requires a separate decision. It must not
be introduced as a runtime plugin feature by implication.

### 3. Trust chain

Execution requires all of the following, in order:

1. the integration is installed;
2. it is enabled for the current session;
3. the workspace root is trusted;
4. the user grants the specific capability for that integration;
5. the user approves the first resolved launch;
6. the gateway accepts the resolved command and environment.

Installation, enabling, and a manifest declaration grant nothing by
themselves. The default is deny.

Capabilities are closed and non-wildcard:

```text
exec:lsp
exec:formatter
exec:dap
exec:build
exec:task
```

No `exec:*`, no implicit capability from `kind`, and no capability inheritance
between roots.

A grant is bound to:

- integration id and version;
- manifest or descriptor hash;
- workspace root;
- capability;
- resolved executable path.

Changing any bound value invalidates the grant. A profile-wide grant must not
silently cover a different workspace.

### 4. Gateway execution policy

Every external tool, including descriptors contributed by native modules, runs
through the gateway:

- `program` is an absolute path or a path resolved from an explicit allowlist;
- arguments are an argv array; no shell string, pipeline, or redirection;
- `cwd` is workspace-relative and root-confined;
- `env` is an explicit allowlist; secret-bearing variables are not inherited;
- stdout/stderr are bounded; timeout and process-group cleanup are mandatory;
- formatters receive stdin and return stdout only; in-place and temp-file
  modes remain separate policy work;
- LSP and DAP clients remain core implementations; descriptors provide only
  adapter launch metadata;
- DAP keeps the ADR-0065 trusted direct-spawn exception, but only after the
  same capability grant.

A native module may call the gateway, but it may not bypass it for a
descriptor-launched external tool.

### 5. Untrusted workspaces

In an untrusted workspace root:

- integrations are not auto-enabled;
- LSP, DAP, formatter, build, and task descriptors do not start automatically;
- no grant from another root is reused;
- the user may run a one-off command only after an explicit confirmation.

Trust is per root. A multi-root session must show the owning root for every
descriptor and grant.

### 6. Distribution and marketplace

- The signed index and publisher key rules remain ADR-0021.
- A dev extension is explicitly marked and receives no execution capability by
  default.
- Unsigned or unverified descriptors may be inspected, but not granted
  `exec:*` capabilities.
- Revocation removes the descriptor and any outstanding grant at the next
  refresh.
- Marketplace UI is deferred until signing, provenance, and revocation are
  complete.

### 7. Runtime plugin boundary stays narrow

Runtime plugins:

- may register commands, picker sources, and completion sources;
- may call core-mediated commands subject to the command policy;
- may request bounded one-shot `sys.exec` / `sys.exec.v2` through the
  gateway when the host advertises and the user grants that existing plugin
  capability;
- may not spawn processes directly or bypass the gateway;
- may not declare or implement formatter, DAP, LSP, build, or task
  descriptors/providers;
- may not modify text, selection, undo, rendering, focus, or sandbox policy.

This preserves ADR-0065 and ADR-0014. Removing `sys.exec` / `sys.exec.v2`
would be a separate breaking plugin-API decision. Adding a runtime
formatter/DAP provider would also require a new ADR that supersedes those
boundaries.

## Consequences

Positive:

- A manifest cannot become an arbitrary-command launcher.
- Extensions can contribute IDE integrations without being trusted with
  process execution.
- Per-root grants align with multi-root routing and workspace trust.
- The existing plugin host, gateway, and sandbox boundaries remain intact.

Negative:

- Users must approve external tools per root, which adds first-run friction.
- Native modules and trusted settings are the only descriptor sources at
  first, so third-party integration distribution is slower.
- Manifest/descriptor hashing and grant invalidation add state and tests.

## Verification

- Focused tests:
  - runtime plugin manifest with `formatter`, `dap`, or `lsp` is rejected;
  - descriptor without a grant cannot start;
  - runtime plugin `sys.exec` remains capability-gated and bounded; this ADR
    does not widen or remove it;
  - `exec:*` and unknown capabilities are rejected;
  - grant invalidates on version, manifest hash, root, capability, or command
    change.
- Failure/cancellation tests:
  - shell metacharacters are not executed;
  - env allowlist denies secrets;
  - cwd outside the root is rejected;
  - untrusted root and untrusted workspace are denied;
  - gateway timeout and process-group cleanup terminate the child.
- Real surface smoke:
  - a trusted root starts one formatter, one LSP, and one DAP adapter;
  - a second root with a different grant cannot reuse the first root's grant;
  - an untrusted root starts none automatically;
  - the integration appears in the permission/status UI with the exact
    resolved command.

## References

- VS Code Extension Host: <https://code.visualstudio.com/api/advanced-topics/extension-host>
  — extensions run in a separate host; the host is kept from slowing UI or
  modifying the UI.
- LSP 3.17 specification: <https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/>
  — language features are a client/server protocol.
- Debug Adapter Protocol overview: <https://microsoft.github.io/debug-adapter-protocol/overview>
  — DAP standardizes debug UI while adapters remain external tools.
- Build Server Protocol specification: <https://build-server-protocol.github.io/docs/src/specification>
  — build targets and capabilities are exchanged with a build server.
- IntelliJ Platform overview: <https://plugins.jetbrains.com/docs/src/intellij/about.html>
  — project model and PSI are separate platform layers.
- IntelliJ indexing and PSI stubs: <https://plugins.jetbrains.com/docs/src/intellij/indexing-and-psi-stubs.html>
  — indexing is background work with an explicit unindexed mode.
- Zed extensions: <https://zed.dev/docs/src/extensions/developing-extensions>
  — extensions declare languages, debuggers, themes, and snippets through a
  manifest.
- Sourcegraph SCIP indexers: <https://sourcegraph.com/docs/src/code-navigation/writing-an-indexer>
  — symbols and occurrences can be imported from an external index format.

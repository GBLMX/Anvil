# API Surface and Compatibility

Status: current. Canonical inventory for the kernel baseline and the
cross-process contracts. Detailed decisions live in
[ADR-0037](adr/0037-kernel-api-baseline.md), [ADR-0014](adr/0014-plugin-api-v1-freeze.md),
[ADR-0007](adr/0007-jsonrpc-envelope-and-capability-token.md), and
[ADR-0028](adr/0028-module-contract.md).

## Audiences

| Audience | Crate | Stability |
|---|---|---|
| Plugin authors | `anvil-api`, `anvil-ipc` | Plugin API v1 and JSON-RPC envelope are frozen |
| Control/MCP clients | `anvil-control` | Protocol v3; typed Rust DTOs plus JSON wire |
| Module authors | `anvil-module` | Contract v1 is frozen |
| Text model consumers | `anvil-core` | Kernel baseline; semver-versioned source surface |
| Headless frontends | `anvil-session` | Kernel baseline; semver-versioned source surface |
| Editor core consumers | `anvil-editor` | Kernel baseline; semver-versioned source surface |
| Internal TUI | `anvil-tui` | Not a kernel baseline; no stability promise |

## Version lines

```text
Plugin API:       anvil_api::API_VERSION = 1.0.0
JSON-RPC:         anvil_ipc::JSONRPC_VERSION = 2.0
Control protocol: anvil_control::PROTOCOL_VERSION = 3
Module contract:  anvil_module::CONTRACT_VERSION = 1
Kernel baseline:  ADR-0037 (source-level, not a Rust ABI)
Workspace version: Cargo.toml workspace.version
```

The plugin API, control protocol, and module contract are independent
version lines. A change in one does not silently change another.

## Frozen surface

- `anvil-api` request/response DTOs, method catalog, API version, and generated
  JSON Schema / TypeScript / Lua bindings.
- `anvil-ipc` JSON-RPC envelope, framing, error codes, and gateway client wire
  contract.
- `anvil-control` control methods, capability policy, and typed result DTOs.
- `anvil-module` contract v1, module traits, registry rules, and service traits.
- The `anvil-core` / `anvil-editor` / `anvil-session` items listed as the kernel
  baseline in ADR-0037.

Not frozen: `anvil-tui` internals, private fields, plugin-host internals, and Go
gateway internals beyond the versioned protocol.

## Compatibility rules

- Kernel and contract crates use semver. A source-breaking change requires a
  major or contract bump plus an ADR/amendment.
- Adding a public item is normally minor; removing or renaming it is major.
- Adding a public enum variant or public struct field is major unless the type
  was already `#[non_exhaustive]`.
- Adding `#[non_exhaustive]` to an existing public type is itself a major
  change. Frozen v1 types cannot be retrofitted silently.
- A contract bump must update official modules, both composition consumers, and
  the compatibility matrix.

## Automated gates

- Generated bindings: `cargo run -p anvil-api --example export_bindings --features bindings`, then
  `git diff --exit-code -- crates/anvil-api/bindings`.
- Public surface snapshot: `bash scripts/api-surface-check.sh` hashes every
  normalized `pub` declaration, including signatures, fields, and variants.
  The committed hash is `docs/src/api-surface.sha256`; a deliberate API change
  must update it in the same change.
- Dependency direction: `scripts/architecture-check.sh`.
- Semver: PR CI runs `cargo-semver-checks`. Locally, install the pinned
  checker and run `cargo semver-checks check-release --baseline-rev origin/main`.

## Migration notes
### Editor text deltas

`anvil-editor::EditDelta` and `EditorState::text_edits_since` are additive
kernel APIs. They expose applied text mutations for view remapping; existing
editor and session signatures are unchanged.


### ADR-0077: benchmark-only text backends

`RopeBuffer` and `GapBuffer` are no longer exported by `anvil-core`, and
`anvil-core` no longer depends on `ropey`. Production consumers should keep
using `TextDocument`/`PieceTable`; comparison or benchmark consumers should
depend on the unpublished `anvil-text-bench` crate. The source-breaking change
is recorded in the changelog for the 0.8.0 major line.

## Known pending refactors

- `anvil-editor` exposes typed `try_*` methods; legacy `bool` wrappers on
  `EditorState` and `DocumentSession` are deprecated for one compatibility
  cycle.
- `anvil-control::CommandSpec` keeps its wire name for string dispatch and
  exposes a typed `ControlCommandId` for manifest checks.
- `anvil-ipc::FrameError::Io` preserves the I/O kind; the full `io::Error`
  source and message are not retained.
- Plugin API 1.1 and module contract v2 are separate ADR tracks. They must not
  change v1 behavior in place.
- Module contract v1 keeps one service per kind. Multi-provider composition is
  currently distribution-level (`LanguageToolingProviders`); contract v2 may
  promote a provider-list registration after a second consumer exists.

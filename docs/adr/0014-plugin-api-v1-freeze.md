# ADR-0014: Plugin API v1 Freeze

- Status: Accepted
- Date: 2026-11-06
- Related: ADR-0003, ADR-0007, ADR-0013

## Decision

The plugin wire API freezes at `1.0.0`.

- Rust types in `anvil-api` are the single source of truth.
- `anvil_api::API_VERSION` is `1.0.0`.
- `anvil_api::METHODS` has no pending DTO entries.
- JSON Schema, TypeScript, LuaLS stubs, and `api-version.ts` are generated and
  committed.
- The manifest path is `plugin.json`. ADR-0003's `Anvil.toml` reference is
  superseded; keeping the existing path avoids a second discovery format.
- A manifest declares its API line in `api`; accepted v1 values are `1`, `1.0`,
  and `1.0.0`.
- `session.hello` uses the same `1.0.0` version on the editor, plugin host, and
  gateway sides.

## Method catalog

The frozen catalog includes the ADR-0003 ten methods plus the implemented
snapshot, atomic apply-edit, and workspace mutation helpers:

`editor.get_view`, `editor.get_snapshot`, `editor.set_mode`,
`editor.apply_edit`, `editor.register_command`, `ui.notify`, `ui.status`,
`event.subscribe`, `sys.read_file`, `sys.create_dir`, `sys.rename`,
`sys.remove`, `sys.copy`, `sys.remove_all`, `sys.fetch`, and `sys.exec`.

## Consumers

The freeze gate is satisfied by two consumers:

1. The official editor/session path uses the Rust DTO catalog for view,
   snapshot, mode, and atomic edits.
2. `crates/anvil-api/tests/contract_consumer.rs` is an independent consumer that
   round-trips every request DTO and checks the catalog for empty contracts.

## Marketplace boundary

This ADR freezes the API contract only. Release signing, SBOM, provenance, and
the `0.6.1` package/index ADR remain prerequisites for marketplace publishing.

# Module inventory

`anvil modules [--json]` is a read-only view of the trusted modules compiled
into the running distribution. It does not contact a marketplace, read
installed plugin state, or mutate anything.

## Human output

```text
anvil 0.6.0 modules (distribution anvil)
official.git                 Official Git                   contract 1-1  commands 0   feature=official-git enabled
```

## JSON schema

Schema version `1` has stable top-level fields and stable module ids:

```json
{
  "schema_version": 1,
  "distribution": "anvil",
  "version": "0.6.0",
  "modules": [
    {
      "id": "official.git",
      "name": "Official Git",
      "contract": "1..1",
      "version": "0.6.0",
      "source": "compiled-in",
      "enabled": true,
      "feature": "official-git",
      "commands": 0
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `schema_version` | inventory schema, currently `1` |
| `distribution` | process name from the distribution |
| `version` | workspace/distribution version |
| `modules[].id` | stable module id; may not be inferred from display names |
| `modules[].name` | human-readable module name |
| `modules[].contract` | inclusive module-contract range as `min..max` |
| `modules[].version` | distribution version providing the compiled module |
| `modules[].source` | always `compiled-in` for this inventory |
| `modules[].enabled` | whether the distribution enables the module |
| `modules[].feature` | Cargo feature that compiles the module |
| `modules[].commands` | number of commands the module declares |

Only modules compiled into the binary appear. A default build enables
`official-git`; a `full` build adds workspace index, desktop, LaTeX, and Typst.
This is a feature-gated inventory, not a claim that all possible official
modules are present.

## Why feature gating matters

A module missing from the output is either not compiled into this binary or not
enabled by its distribution. It is not silently downloaded. Use
`anvil doctor` for installation and language-server checks.

## Installed plugins

Compiled distribution modules are separate from runtime plugins. Use
`:plugin-inventory` for a read-only page listing installed plugin IDs,
versions, enabled state, source, and digest. `:plugin-list` opens the mutable
manager with enable/disable, uninstall, and rollback actions.

For a plugin without a package digest, the inventory shows `digest=bundled`
only when its source is `official`; local plugins show `digest=none`.

## Compatibility and deprecation policy


Status: contract v1 frozen in `0.7.3`. The standard edition and the independent
`anvil-distro-minimal` consumer build against the same `anvil-module`
contract.

## Compatibility matrix

| Component | Contract | Cargo feature | Standard | Minimal | Registered service |
|---|---:|---|---|---|---|
| `anvil-module` | 1 | n/a | compiled | compiled | contract and registry |
| `official.git` | 1 | `official-git` | default | absent | `GitService` |
| `official.workspace-index` | 1 | `official-workspace-index` | opt-in | absent | `WorkspaceIndexService` |
| `official.desktop` | 1 | `official-desktop` | opt-in | absent | `DesktopService` |
| `official.language-latex` | 1 | `official-language-latex` | opt-in | absent | `LanguageToolingService` |
| `official.language-typst` | 1 | `official-language-typst` | opt-in | absent | `LanguageToolingService` |

The matrix records the contract line, not a Rust ABI. Modules are compiled from
source into the distribution and are rebuilt with the kernel.

## Contract v1 freeze

Contract v1 covers:

- `Module`, `ModuleMetadata`, `ModuleId`, and `ContractRange`;
- `ModuleRegistry` registration, command ownership, and service slots;
- the `GitService`, `WorkspaceIndexService`, `DesktopService`, and
  `LanguageToolingService` extension points.

The freeze is source-level. Adding another official module does not change the
contract; it adds a Cargo feature, a module crate, and a composition call.

## Deprecation policy

1. **Additive changes stay in v1 only when source-compatible.** New defaulted
   trait methods and new registry service slots do not bump the contract.
   Adding a field to `ModuleMetadata`, adding a `ModuleError` variant, or
   changing any other public struct/enum shape requires a contract bump because
   downstream modules construct or match those types.
2. **Breaking changes require a contract bump.** Removing or changing a public
   signature, required trait method, service trait, registration rule, or
   failure semantic increments `CONTRACT_VERSION`.
3. **Deprecation precedes removal.** Public APIs are marked `#[deprecated]` and
   documented for at least one release series before removal. Removal happens
   only with the next contract version.
4. **Old ranges fail closed.** A module whose `ContractRange` does not include
   the compiled contract is rejected with `ContractMismatch`; the registry does
   not partially register it.
5. **Official modules move with the contract.** The distribution updates its
   official modules in the same change that bumps the contract. Third-party
   modules are updated and rebuilt by their distributions; there is no runtime
   binary compatibility promise.
6. **Ids and features are stable within a contract line.** Renaming an official
   module id or Cargo feature is a compatibility change; use a new id or defer
   the rename to the next contract version.
7. **No silent behavior changes.** A module may not replace kernel commands,
   bypass gateway policy, or widen plugin capabilities as a compatibility
   workaround.

## Required checks

- Feature matrix: no modules, each official module, and all modules.
- Contract version mismatch and registration rollback tests.
- Independent `anvil-distro-minimal` composition.
- Module dependency trees must not contain `anvil-tui`.
- Installed-binary smoke with the opt-in module enabled.
- Desktop URL validation and fixed-opener argument-boundary tests.

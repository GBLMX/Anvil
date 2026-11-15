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

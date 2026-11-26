# Settings sources

Anvil resolves typed settings from these sources, in order:

1. built-in defaults;
2. the user settings file (`ANVIL_SETTINGS`, or
   `$XDG_CONFIG_HOME/anvil/settings.toml` / `$HOME/.config/anvil/settings.toml`);
3. a trusted workspace `.anvil.toml`;
4. environment variables.

Environment variables are a current compatibility source, but ADR-0075
proposes deprecating per-field environment overrides. The
[environment inventory](#environment-inventory) separates migration targets
from bootstrap and operational variables.

This page documents the current implementation. Sparse user saves,
reset-to-inherited, the metadata registry, and the registry-driven overlay are
implemented; the generated schema and user-patch export default remain proposed
in [ADR-0045](adr/0045-settings-patch-layers.md).

CLI flags currently control process behavior (`--workspace`, `--editor`,
`--read-only`, `--truecolor`), not typed settings fields. They are therefore not
shown as a settings source. If a typed CLI override is added later, it must get
its own stable `cli` origin.

## Effective and ignored sources

`:settings` shows the source of the selected field:

- `default`
- `environment`
- `startup override` for `ANVIL_THEME`
- the user settings path
- the trusted project settings path

If a project `.anvil.toml` exists but cannot be used, the same line also shows
the ignored path and reason, for example:

```text
project file ignored: /repo/.anvil.toml (workspace root is untrusted)
```

An explicit `ANVIL_SETTINGS` file replaces the project layer; an existing
project file is then reported as ignored with that reason.

`:config-reload` reports the settings files that actually loaded and appends
ignored project files with their reason. A parse failure is reported as an
error containing the file path and parser location; it does not silently
replace the failure with defaults.

## Saving and reset

The `:settings` overlay saves only explicit user overrides. Resetting a row
removes its user key and re-resolves the value from the trusted project,
environment, or built-in default. `ANVIL_*` environment values are never
copied into the user file by a save. Comments and unknown keys in the existing
file are preserved, while unknown keys remain load errors on the next reload.

## Export and import

`:settings-export <path>` writes the sparse user override patch to a portable
TOML file. Paths are resolved relative to the workspace unless absolute.
Untouched defaults, inherited project values, and environment overrides are
not exported.

`:settings-export --effective <path>` writes the resolved effective values,
including environment overrides, for diagnostics or machine migration.

`:settings-import <path>` validates the file, writes it to the user settings
path (`ANVIL_SETTINGS` or `$XDG_CONFIG_HOME/anvil/settings.toml`), and runs the
same reload as `:config-reload`. Environment overrides still apply after
import, and `:settings` shows the resulting origin for each field. The imported
file is normalized: comments and unknown keys are preserved only when the
target file already contains them, matching `:settings` saves.

## Schema

`anvil settings schema` prints the JSON Schema generated from the settings
registry. Release layouts install it as
`share/anvil/settings.schema.json`; bind it to a settings file with:

```toml
#:schema /absolute/path/to/share/anvil/settings.schema.json
```

Taplo uses the schema for validation, completion, and descriptions.
The schema includes `[lsp.commands]` and per-root `[lsp.roots]` profiles.

Language-scoped editor values live in `$XDG_CONFIG_HOME/anvil/languages.toml`
and, for trusted primary roots, `<root>/.anvil/languages.toml`:

```toml
[languages.rust]
soft_wrap = true
format_on_save = true
```

`anvil settings languages-schema` prints the generated schema; release layouts
install it as `share/anvil/languages.schema.json`. Only registry entries marked
language-scoped are accepted. Global-only keys, unknown keys, and executable
settings are rejected. Language overrides apply to the active buffer only and
do not mutate process-wide settings.

The precedence is global resolved value, user language layer, trusted project
language layer, then `ANVIL_*` environment. `ANVIL_SETTINGS` remains a complete
override and ignores both language files.

`anvil settings check [--json]` is a read-only report of the global and
language layers. It lists each candidate path, whether it exists, why a layer
was ignored, every field's effective and explicit value with its source, and
language overlay values. JSON output carries `schema_version: 1`. It does not
apply settings or replace `anvil doctor`.

The `:settings` overlay supports `/` search over key, label, description, and
group. Query filters include `@modified`, `@explicit`, `@language`, and
`@source:<text>`. Rows mark explicit overrides and the footer reports the
selected field's source. Saving still writes only the user global layer.

## Environment inventory

Inventory date: 2026-11-27. The filter matters: a raw repository scan finds
**88 unique `ANVIL_*` tokens**, including sandbox, signing, gateway, CI, test,
and documentation references. That is not a settings-field count.

`ANVIL_SETTINGS` is a complete-file override, not a per-field settings layer.
It must be tracked and tested separately.

| Class | Count | Variables |
|---|---:|---|
| Per-field editor/search/LSP/completion behavior | 10 | `ANVIL_EDITOR_SCROLL_MARGIN`, `ANVIL_EDITOR_AUTO_SAVE`, `ANVIL_EDITOR_SOFT_WRAP`, `ANVIL_EDITOR_FORMAT_ON_SAVE`, `ANVIL_EDITOR_FILE_WATCHING`, `ANVIL_EDITOR_MAX_BUFFERS`, `ANVIL_EDITOR_SIDEBAR_PERCENT`, `ANVIL_SEARCH_CASE_SENSITIVE`, `ANVIL_LSP_INLAY_HINTS`, `ANVIL_COMPLETION_AUTO_BRACKETS` |
| Complete-file override | 1 | `ANVIL_SETTINGS` |
| Presentation/bootstrap | 3 | `ANVIL_THEME`, `ANVIL_HIGH_CONTRAST`, `ANVIL_REDUCED_MOTION` |
| LSP language command override | 15 | `ANVIL_LSP_RUST`, `ANVIL_LSP_GO`, `ANVIL_LSP_PYTHON`, `ANVIL_LSP_TYPESCRIPT`, `ANVIL_LSP_CLANGD`, `ANVIL_LSP_JAVA`, `ANVIL_LSP_LUA`, `ANVIL_LSP_JSON`, `ANVIL_LSP_YAML`, `ANVIL_LSP_SHELL`, `ANVIL_LSP_MARKDOWN`, `ANVIL_LSP_TOML`, `ANVIL_LSP_HTML`, `ANVIL_LSP_CSS`, `ANVIL_LSP_ZIG` |

`anvil-tui/src/settings.rs` therefore contains 14 unique `ANVIL_*` names:
10 per-field variables, `ANVIL_SETTINGS`, and 3 presentation variables. The
historical ADR-0075 phrase "10 settings registry variables" referred to the
then-current 9 per-field variables plus the complete-file override.

The LSP names are configured by `anvil-lsp-client/src/settings.rs`; they are
language command overrides, not ordinary settings fields. A settings migration
must not count them as per-field editor settings.

The older ADR-0075 snapshot (87 raw tokens, 10 registry variables, 17 LSP
variables) is historical. The numbers above are the current inventory; any
further migration must regenerate this table with the same filters.

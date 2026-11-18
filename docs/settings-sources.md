# Settings sources

Anvil resolves typed settings from these sources, in order:

1. built-in defaults;
2. the user settings file (`ANVIL_SETTINGS`, or
   `$XDG_CONFIG_HOME/anvil/settings.toml` / `$HOME/.config/anvil/settings.toml`);
3. a trusted workspace `.anvil.toml`;
4. environment variables.

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

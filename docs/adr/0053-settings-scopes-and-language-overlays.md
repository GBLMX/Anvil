# ADR-0053: Explicit Settings Scopes, Language Overlays, and Registry Completeness

- Status: Proposed
- Version: 1.0.0
- Date: 2026-11-18
- Related: [ADR-0024](0024-tui-state-and-domain-boundaries.md),
  [ADR-0038](0038-project-settings-trust.md),
  [ADR-0045](0045-settings-patch-layers.md),
  [ADR-0048](0048-per-root-language-tooling-and-lsp-workspace.md),
  [ADR-0052](0052-language-tool-setup-and-source-resolution.md)
- Supersedes:
- Superseded by:

## Context

ADR-0045 replaced full-file settings saves with sparse patches, field-level
provenance, inherited reset, and a generated schema. Those decisions remain
correct. The next settings problem is not another global field: it is scope
and metadata drift.

The current implementation still has these limits:

- `Settings` is process-wide. LSP workers are root-aware, but editor behavior
  such as soft wrapping and format-on-save has no language scope.
- `settings.rs` contains the typed effective settings, a separately written
  `SettingsPatch`, a `SettingsKey` match table, and a registry. The registry is
  not yet the single source for every nested schema shape.
- `settings_schema()` generates flat fields from the registry but still
  special-cases theme, watch, LSP roots, and icons. Those sections can drift
  independently.
- The settings overlay has rows, tabs, adjust, reset, save, and source labels,
  but no search/filter or modified-marker state. ADR-0045 described a
  registry-driven searchable overlay; the current implementation does not yet
  provide that surface.
- There is no shared `settings check` command that reports all layer,
  provenance, deprecation, and validation errors at once.
- Renames have no metadata path. The current design says aliases should be
  used instead of a schema version, but `SettingDef` has no alias or
  deprecation fields.

External editors converge on a small set of useful patterns:

- VS Code separates default, user, remote, workspace, folder, and
  language-specific scopes; its settings editor has search, `@modified`,
  per-setting reset, and schema-backed IntelliSense. Language-specific user
  settings can override non-language workspace settings, so precedence must be
  explicit rather than inferred.
- Helix merges a global `config.toml` with a project `.helix/config.toml`,
  provides explicit config open/reload commands, and keeps language and
  language-server configuration in a separate `languages.toml`.
- Zed provides a settings editor plus JSON files, documents defaults and
  accepted values per setting, and supports language/platform/release-channel
  overrides.
- JSON Schema 2020-12 is intended for validation, documentation, and
  interaction; Taplo can bind an Anvil schema to a TOML file with `#:schema`.

Anvil should borrow the scope and discovery patterns without importing VS Code's
profile/policy system or a plugin settings center.

## Decision

### 1. Make scope an explicit registry property

Add a `SettingScope` model to the existing registry:

```text
SettingScope:
  Global
  Language
```

A setting declares whether it is global-only or eligible for language override.
Process-wide values such as `theme`, `ui.reduced_motion`, `ui.high_contrast`,
`watch.*`, and `editor.file_watching` remain global-only. Language-eligible
editor values start with `editor.soft_wrap` and `editor.format_on_save`; future
tab/indent fields can opt in when they exist.

Language values must not be merged into the process-wide `Settings` struct. The
runtime keeps:

```text
Settings         process/workspace-wide effective values
BufferSettings   language overrides for the active buffer
```

`BufferSettings` is resolved when the active buffer or language changes, not on
the render path. It contains only fields explicitly marked `Language` in the
registry. A buffer with no language or no language overlay uses the global
value.

This preserves the selection-first model and prevents switching a Rust buffer
to a Go buffer from changing the process-wide UI or another buffer's behavior.

### 2. Define one precedence matrix

For global values:

```text
defaults
  < user settings.toml
  < trusted primary-root .anvil.toml
  < ANVIL_* environment
```

For language-eligible values:

```text
global resolved value
  < user languages.toml
  < trusted primary-root .anvil/languages.toml
  < ANVIL_* environment
```

The matrix is explicit and tested. The project language layer is allowed only
when the primary workspace root is trusted, matching ADR-0038. Secondary roots
do not contribute process settings or language overlays.

`ANVIL_SETTINGS` keeps its existing complete-override behavior: when it is set,
the `ANVIL_SETTINGS` file replaces every other file-backed layer. That includes
the user and project `settings.toml` layers and the user and trusted-project
`languages.toml` layers. `ANVIL_*` environment variables still apply after it.
No `ANVIL_LANGUAGES` override is added in this ADR.

### 3. Put language overrides in a separate `languages.toml`

The user file is:

```text
$XDG_CONFIG_HOME/anvil/languages.toml
```

and the trusted project file is:

```text
<primary-root>/.anvil/languages.toml
```

The file contains language-keyed sparse patches, for example:

```toml
[languages.rust]
soft_wrap = true
format_on_save = true

[languages.markdown]
soft_wrap = true
```

Only registry entries marked `Language` are accepted there. Unknown keys,
global-only keys, and executable or command-bearing keys are rejected. LSP
root profiles remain in the existing `[lsp.roots]` section for now; this ADR
does not migrate them.

A separate file keeps `settings.toml` about process defaults and avoids turning
one file into a catalog of every language-specific behavior. It also gives
language overlays their own schema, trust check, and test surface.

### 4. Complete the registry as the shared fact source

`SettingDef` gains:

```text
scope
project_allowed
language_allowed
env name
kind/range/enum
aliases
deprecated
introduced
schema fragment
```

The registry drives:

- typed defaults and environment parsing;
- sparse patch validation;
- generated `settings.schema.json` and `languages.schema.json`;
- settings overlay rows, labels, descriptions, groups, tabs, and reset values;
- `settings check` provenance and validation output.

Nested schemas for theme, watch, LSP roots, and icons must be registry-owned
schema fragments rather than handwritten exceptions in `settings_schema()`.
The hand-written `const` registry remains until field count or measured
maintenance cost justifies generated Rust.

`SettingsPatch` may remain a manually written serde shape initially, but a
completeness test must fail when a registry key has no patch field, schema
entry, or UI row. This is a required invariant, not a documentation convention.

### 5. Add a first-class settings check

Add a read-only command:

```text
anvil settings check [--json]
```

It reports, for both global and language settings:

- the path of every candidate layer and whether it exists;
- trust state and why a project layer was ignored;
- every field's effective value, explicit value, and source;
- parse and schema errors with file and line/column;
- unknown keys, deprecated aliases, and disallowed project/language keys;
- environment overrides and startup-only overrides.

`anvil doctor` remains installation/runtime diagnostics. `:config-reload`
remains the explicit apply operation; this ADR does not add file watching or
automatic hot reload.

### 6. Make the overlay explain, not just edit

The three-tab settings overlay remains. Add:

- search over key, label, description, and group;
- `@modified`, `@source`, and `@language` filters;
- an explicit "effective" versus "explicit override" view;
- modified markers and inherited reset;
- source labels that distinguish user, trusted project, language user,
  language project, environment, and startup override;
- actions to open the user/project settings file and copy a setting key.

The overlay does not become a VS Code-style settings center, profile manager, or
plugin settings registry.

### 7. Keep compatibility and security bounded

- Renames use registry aliases and deprecation warnings; a `schema_version` is
  added only for a genuinely breaking migration.
- Saving remains sparse, atomic, comment-preserving, and writes only the user
  layer.
- Project and language project layers cannot contribute `[lsp.commands]`,
  executable paths, environment values, or other execution-bearing keys.
- No secrets are accepted in settings or committed project files.
- Profiles, a system-wide `/etc/anvil` layer, per-secondary-root project
  settings, plugin-contributed settings, and automatic hot reload remain
  deferred and require a separate ADR with a concrete consumer.

## Consequences

Positive:

- language-specific editor behavior has a defined owner and does not mutate
  process-wide settings;
- scope precedence is documented and testable;
- adding a setting has one registry metadata path for schema, UI, env, and
  validation;
- users can explain both value and origin instead of guessing;
- the settings overlay becomes usable as the field count grows;
- project and language trust boundaries remain explicit.

Negative:

- a new `languages.toml`, schema, and resolver path must be maintained;
- `BufferSettings` adds resolution and cache invalidation to buffer switches;
- the registry gains metadata and completeness tests before it can justify
  code generation;
- the settings overlay grows a query/filter state;
- language project settings introduce a second project file to reason about.

## Verification

Focused tests and checks:

- precedence matrix for default, user, trusted project, language user, language
  project, and environment values;
- two buffers with different languages resolve different `BufferSettings`
  without changing global `Settings`;
- untrusted or secondary-root project language files are ignored with a path
  and reason;
- global-only keys are rejected in `languages.toml`;
- command-bearing and executable keys are rejected from project language files;
- every registry key reaches its schema, patch field, provenance, and UI row;
- generated `settings.schema.json` and `languages.schema.json` cover all nested
  registry paths;
- `anvil settings check --json` reports each layer and every validation error
  with a stable schema version;
- search and `@modified` operate on the registry, not hardcoded row indexes;
- sparse save preserves comments and unknown keys, and reload still rejects the
  unknown key;
- `ANVIL_SETTINGS` ignores both user/project `settings.toml` and
  user/project `languages.toml` layers while `ANVIL_*` environment variables
  still apply;
- no language override is written into the user settings or project files by
  reset or save;
- language overlay resolution does not run on the render path.

Manual checks:

- edit a Rust and a Markdown buffer with different `soft_wrap` values and
  confirm each buffer resolves its own value;
- add project language settings in a trusted root and verify the source label;
- add the same file in an untrusted root and verify it is reported as ignored;
- bind both generated schemas in Taplo and verify completion/validation.

## References

- [VS Code: User and workspace settings](https://code.visualstudio.com/docs/configure/settings)
  — default/user/workspace/folder/language precedence, settings search,
  `@modified`, reset, and schema-backed editing.
- [Helix: Configuration](https://docs.helix-editor.com/master/configuration.html)
  — global plus project config merging, `:config-open`, and explicit
  `:config-reload`.
- [Helix: Languages](https://docs.helix-editor.com/master/languages.html)
  — separate `languages.toml`, project-local language configuration, and
  language-server settings.
- [Zed: All Settings](https://zed.dev/docs/reference/all-settings)
  — settings editor, per-setting defaults/options, and language/platform
  overrides.
- [JSON Schema 2020-12](https://json-schema.org/draft/2020-12)
  — validation, documentation, and interaction semantics for the generated
  schemas.
- [Taplo schema directives](https://github.com/tamasfe/taplo/blob/08f343be02ce1b20296470396a42f0fa47820449/site/site/configuration/directives.md)
  — binding a schema to a TOML document using `#:schema`.
- [ADR-0038](0038-project-settings-trust.md)
  — project settings are trust-gated and command configuration is user-only.
- [ADR-0045](0045-settings-patch-layers.md)
  — sparse patches, provenance, inherited reset, and the registry baseline.
- [ADR-0048](0048-per-root-language-tooling-and-lsp-workspace.md)
  — per-root language tooling and project-root LSP configuration.

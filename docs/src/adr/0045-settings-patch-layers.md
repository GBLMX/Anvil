# ADR-0045: Sparse Settings Layers, Provenance, and a Metadata Registry

- Status: Accepted; implemented 2026-11-16
- Version: 1.2.1
- Implementation: sparse saves, inherited reset, provenance, metadata registry,
  registry-driven overlay, generated settings schema, and sparse export default
  landed
- Date: 2026-11-16
- Related: [ADR-0006](0006-upstream-dependencies.md),
  [ADR-0023](0023-internal-capability-closure.md),
  [ADR-0024](0024-tui-state-and-domain-boundaries.md),
  [ADR-0037](0037-kernel-api-baseline.md),
  [ADR-0038](0038-project-settings-trust.md)
- Supersedes:
- Superseded by: [ADR-0067](0067-settings-file-hot-reload.md) for the settings hot-reload clause only

## Context

Anvil's typed settings model has 12 leaf settings across theme, editor, search,
LSP, and UI groups. The implementation is concentrated in
`crates/anvil-tui/src/settings.rs` at roughly 1,467 lines, with a second
settings UI in `settings_ui.rs`.

The current shape duplicates field knowledge:

- `Settings` owns typed effective values;
- `SettingsFile` mirrors the same fields with `Option<T>`;
- `SettingsKey` identifies fields for provenance;
- environment constants and two match tables repeat the field list;
- `save_to_path` writes every field explicitly;
- the settings overlay hardcodes `SETTINGS_ROWS = 11` and maps row indexes to
  labels, descriptions, groups, values, adjust/reset behavior, and
  `SettingsKey`;
- documentation repeats the paths and env names.

The duplication has already drifted: `editor.sidebar_percent` exists in
`Settings`, `SettingsFile`, `SettingsKey`, environment handling, and save, but
it has no settings-overlay row. Adding a field currently requires coordinated
edits across at least seven places, and the compiler does not require the
metadata to stay complete.

There are two semantic problems that are more serious than the duplication.

First, the overlay opens with `draft = self.config.settings`, which is the
already-resolved effective settings. `save_settings` then calls
`settings::save(overlay.draft)`, which writes every field. An environment
override can therefore be copied into the user settings file when the user
changes an unrelated field. `persist_theme_status` has the same full-save
behavior.

Second, reset writes the built-in default into the draft and then saves it. It
does not remove the user override. A user who resets a field that is inherited
from trusted project settings writes the default into their user file and masks
the project value. Reset should reveal the inherited value, not override it.

Theme provenance is also inconsistent. `ANVIL_THEME` marks the theme source as
`environment`, but `apply_env()` does not apply the theme value; the terminal
startup path applies it separately. The displayed source and the effective value
can therefore describe different states.

The design must preserve these existing decisions:

- project settings remain trust-gated; `[lsp.commands]` stays user-only and is
  rejected from project files ([ADR-0038](0038-project-settings-trust.md));
- external settings hot reload is not added without a separate watcher design;
  `:config-reload` remains explicit
  ([ADR-0023](0023-internal-capability-closure.md));
- `anvil-config` is not extracted until a non-TUI consumer or a second settings
  parser exists ([ADR-0024](0024-tui-state-and-domain-boundaries.md));
- the user file, trusted project file, and environment remain the only layers;
  the system-wide layer is deferred;
- no new runtime dependency is needed for a 64 KiB TOML file.

Prior art supports separating the resolved value, the explicit override, and
the discovery metadata:

- [Helix](https://docs.helix-editor.com/configuration.html) merges a global
  `config.toml` with a project `.helix/config.toml`, exposes an explicit
  `:config-reload`, and keeps language/tooling configuration in a separate
  `languages.toml`.
- [VS Code](https://code.visualstudio.com/docs/src/configure/settings) exposes user,
  workspace, folder, and language scopes; a searchable settings editor with
  modified markers and per-setting reset; and schema-backed IntelliSense.
- [Zed](https://zed.dev/docs/src/reference/all-settings) documents each setting with
  its description, default, accepted values, and scope, alongside a settings
  editor.
- [Taplo schema directives](https://github.com/tamasfe/taplo/blob/08f343be02ce1b20296470396a42f0fa47820449/site/site/configuration/directives.md)
  bind a JSON Schema to a TOML document with `#:schema`, which fits Anvil's
  existing TOML and Taplo LSP story.

## Decision

### 1. Separate effective settings from sparse patches

Keep `Settings` as the typed effective value consumed by the TUI. Introduce a
sparse `SettingsPatch` whose nested fields are all optional:

```text
Settings          typed effective values used at runtime
SettingsPatch     only explicitly supplied overrides
SettingsLayer     origin, path, and optional trust marker for one patch
resolve()         defaults + layers -> Settings + per-key source map
```

Files and environment variables parse into patches and run through the same
validators. Runtime code continues to read typed `Settings`; it must not query a
dynamic value map on the render path.

The existing precedence remains:

```text
defaults
< user settings.toml (or the complete ANVIL_SETTINGS override)
< trusted primary-root project .anvil.toml
< ANVIL_* environment variables
```

`ANVIL_SETTINGS` continues to replace both the user and project files, as
specified by ADR-0038.

### 2. Save only explicit user changes; reset removes overrides

The settings overlay keeps an effective value, the resolved source map, and a
user patch of explicit changes. Saving merges only that user patch into the
user TOML. Reset deletes the key from the user patch and re-resolves the
effective value from the remaining layers.

The save path is a syntax-only `toml_edit` update; it must not run the semantic
settings parser over the existing document before merging. Comments, ordering,
and unknown keys are preserved as opaque TOML. Loading still rejects unknown
keys, so a saved file containing an unknown key remains invalid until that key
is removed.

`:settings-export` exports the sparse user patch by default.
`:settings-export --effective` exports the resolved values, including
environment overrides, for diagnostics or machine migration. Import continues
to validate before writing the user file.

### 3. Use one field registry for metadata

Add a `SettingDef` registry with one entry per leaf key:

```text
key, dotted path, default, value kind/range, environment name,
scope policy, UI title/description/group, deprecation/alias metadata
```

The registry drives:

- environment parsing and source marking;
- the settings overlay's rows, labels, descriptions, grouping, validation, and
  reset behavior;
- save-key selection;
- generated JSON Schema and settings documentation;
- completeness tests.

The overlay iterates the registry instead of indexing a hardcoded row count.
There is no separate `settings_key(index)` mapping.

Start with a manually maintained `const` registry and completeness tests. Use a
macro or generated Rust only when the field count or measured maintenance cost
justifies it; a second field-list generator is not added now.

### 4. Keep scopes intentionally small

The only implemented scopes are global user, trusted primary-root project, and
environment. The following are deferred and require a new ADR when a concrete
consumer exists:

- language-specific settings;
- per-buffer settings;
- profiles;
- a system-wide `/etc/anvil` layer;
- per-secondary-root project settings;
- plugin-contributed settings.

The primary workspace root remains the only project settings source. Secondary
roots must not change process-wide behavior. A future `cli` origin may be added
for typed CLI overrides, but only with its own provenance label and tests.

### 5. Keep `ANVIL_THEME` as a process-only startup override

Theme remains a first-class field in the user and project files. `ANVIL_THEME`
is a process-only startup override, not a `SettingsPatch` layer: it is applied
once before the TUI starts, and `:theme` changes the running theme and writes
only theme keys to the user patch.

Provenance reports this as a distinct startup override; it must not mark the
file-resolved theme as coming from the environment. A theme change must not
rewrite unrelated effective values.

### 6. Ship a generated JSON Schema for TOML

Keep TOML. Generate `settings.schema.json` from the registry and ship it under
the release layout's `share/anvil` directory. The schema path is
install-prefix dependent, so the user settings file uses an absolute path:

```toml
#:schema /absolute/path/to/share/anvil/settings.schema.json
```

Taplo validates, completes, and documents the file. An `anvil settings schema`
command may print the installed path, but the release file alone is enough for
the first implementation.

Settings evolution is additive. Renames use registry aliases and deprecation
messages instead of a `schema_version` field; a schema version is introduced
only if a breaking migration becomes necessary.

### 7. Keep the settings UI small and source-aware

The overlay remains a three-tab editor rather than a settings-center clone. It
gains registry-driven search/filtering, modified markers, effective-source
labels, and reset-to-inherited behavior. `:settings-open` opens the user TOML in
a buffer. No VS Code-style profiles, policy system, or extension settings
surface is added.

## Consequences

Positive:

- environment overrides cannot be silently persisted;
- reset returns to project/environment/default values instead of masking them;
- adding a setting has one metadata path and one UI path;
- `sidebar_percent` and future fields have a completeness test;
- the schema and docs are generated from the same field metadata;
- TOML comments and unknown keys survive ordinary saves;
- the trust boundary and ADR-0024 boundary remain intact.

Negative:

- save/reset semantics change and existing settings tests must be rewritten in
  a few places;
- sparse save requires a patch-to-TOML editor rather than a full serializer;
- the manual registry still needs discipline until code generation is
  justified;
- language-specific settings remain unavailable until a separate ADR;
- settings hot reload remains manual;
- `:settings-export` changes its default output mode, which needs a changelog
  and documentation note.
- unknown keys are preserved by save but remain load errors until removed;
- `ANVIL_THEME` stays startup-only, so a persisted `:theme` still loses to the
  environment override on the next start;

## Verification

- Focused tests:
  - an env override is absent from the user file after saving an unrelated
    change;
  - reset removes the user key and reveals the project value;
  - reset removes the user key and reveals the environment value;
  - comments and unknown keys survive a sparse save, while the next reload still
    rejects the unknown key;
  - `ANVIL_THEME` is reported as a startup override and does not rewrite the
    file-resolved theme value;
  - every `SettingsKey` has exactly one `SettingDef`, env name, UI row, and
    schema entry;
  - `sidebar_percent` is reachable from the registry-driven overlay;
  - `:settings-export` emits a sparse user patch by default and effective values
    with `--effective`.
- Failure/cancellation tests:
  - invalid TOML, unknown keys, out-of-range values, and project
    `[lsp.commands]` keep the previous effective settings;
  - failed save leaves the previous user file and reports a path;
  - trusted vs untrusted project layers still produce the documented source and
    ignored-file results.
- Real surface smoke:
  - open `:settings`, change one field with an `ANVIL_*` override active, save,
    and inspect the user file;
  - open `settings.toml` and verify the `#:schema` directive in Taplo;
  - run `:config-reload` after an invalid edit and verify the previous settings
    remain active;
  - run the installed release with the sample settings and confirm the settings
    overlay and startup paths agree.

## Alternatives considered

### Add fields to the current structs and overlay

Rejected. It multiplies the existing full-save and reset behavior and leaves the
field metadata duplicated. The next fields would inherit the same bugs.

### Use one dynamic TOML/JSON value tree at runtime

Rejected. It would discard typed access, make defaults and validation easier to
drift, and put dynamic lookups on the UI path. Patches are dynamic only at the
boundary; effective settings stay typed.

### Move settings to JSON

Rejected. TOML already supports comments and structured tables, Anvil has
`toml_edit`, and Taplo can consume a JSON Schema for TOML. A format migration
would add churn without solving the layering problem.

### Extract `anvil-config` now

Rejected by ADR-0024. There is no non-TUI consumer or second parser today. The
model can be fixed inside `anvil-tui` first.

### Add language-specific settings immediately

Deferred. It needs a clear precedence rule, project-trust interaction, and at
least two concrete language-scoped settings. Adding it before sparse
persistence would make reset and provenance harder to reason about.

### Auto-reload settings with the file watcher now

Deferred by ADR-0023. It needs a watcher design, failure semantics, and a
decision about which settings can change while a session is active. Explicit
`:config-reload` remains the supported path.

### Generate the typed Settings struct with a proc macro immediately

Deferred. The manual typed structs are still readable and debuggable. The
registry removes the highest-value duplication; code generation can be measured
later without changing the user-facing contract.

## References

- Helix configuration: https://docs.helix-editor.com/configuration.html —
  global/project TOML layering, explicit config reload, and separate language
  configuration.
- VS Code user and workspace settings:
  https://code.visualstudio.com/docs/src/configure/settings — setting scopes,
  settings-editor search/reset, language-specific overrides, and schema-backed
  editing.
- Zed all settings: https://zed.dev/docs/src/reference/all-settings — per-setting
  defaults, accepted values, descriptions, and language-scoped behavior.
- Taplo schema directives:
  https://github.com/tamasfe/taplo/blob/08f343be02ce1b20296470396a42f0fa47820449/site/site/configuration/directives.md
  — binding a JSON Schema to a TOML document with `#:schema`.

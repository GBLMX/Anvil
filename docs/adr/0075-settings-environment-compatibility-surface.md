# ADR-0075: Settings Environment Compatibility Surface

- Status: Proposed
- Version: 1.0.0
- Date: 2026-11-24
- Related: [ADR-0038](0038-project-settings-trust.md),
  [ADR-0045](0045-settings-patch-layers.md),
  [ADR-0052](0052-language-tool-setup-and-source-resolution.md),
  [ADR-0053](0053-settings-scopes-and-language-overlays.md),
  [ADR-0067](0067-settings-file-hot-reload.md)
- Supersedes: the environment-override clauses of ADR-0053 and ADR-0052
- Superseded by:

## Context

`anvil-tui/src/settings.rs` is 3,464 lines. It owns the typed registry,
settings layers, language overlays, origin reporting, `settings check`, and
`:config-reload`.

A raw repository scan now finds 88 unique `ANVIL_*` tokens. That number is not
a user-facing settings count: it includes sandbox, signing, gateway, CI, test,
and documentation references. The settings registry defines 13 names: nine
per-field behavior variables, the `ANVIL_SETTINGS` complete-file override, and
three presentation variables. The LSP catalog adds 15 unique
`ANVIL_LSP_<LANG>` command overrides. The older 87/10/17 snapshot is
historical; “10 settings registry variables” means nine per-field variables
plus the complete-file override, not ten interchangeable settings. The
frequently quoted “63” has no documented filter and must not be used as a
target without an inventory. See
[Settings sources](../settings-sources.md#environment-inventory).

Current precedence is default, user, trusted project, then environment
(highest), plus language overlays. `ANVIL_SETTINGS` is a complete file
override, not an ordinary per-field layer. ADR-0038 keeps the project layer
trust-gated.
The problem is not another global field: editor and language behavior already has
file-backed sources, while environment variables create a second, harder-to-
inspect source and enlarge the origin-reporting surface.

The failure mode is silent configuration drift: a value appears to come from
the settings file, but an inherited environment variable changes it. Removing
environment settings without a migration policy would also break CI, doctor,
and documented automation.

## Decision

### 1. Files own editor and language behavior

`settings.toml` and `languages.toml`, with their existing default/user/project
precedence and trust rules, are the only sources for editor behavior, LSP
commands, inlay hints, search behavior, formatting, watching, and buffer
limits. The project layer remains governed by ADR-0038.

### 2. Environment variables are bootstrap or operational

Environment variables remain valid only for process/bootstrap and operational
concerns:

- file selection and binary/helper discovery (`ANVIL_SETTINGS`,
  `ANVIL_GATEWAY`, `ANVIL_EXEC_RUNNER`, `ANVIL_HOST_BIN`, `ANVIL_BIN`);
- startup presentation paths or overrides (`ANVIL_KEYMAP`, `ANVIL_THEME`,
  `ANVIL_THEMES`, terminal capability overrides);
- sandbox, signing, gateway logging, and test/CI helpers;
- values that cannot be represented safely in a settings file.

An environment variable must not be added as a new editor setting. New
settings extend the registry and settings schema instead.

### 3. Deprecate per-setting environment overrides

The following compatibility classes are deprecated:

- `ANVIL_EDITOR_*`;
- `ANVIL_SEARCH_CASE_SENSITIVE`;
- `ANVIL_LSP_INLAY_HINTS`;
- `ANVIL_LSP` and `ANVIL_LSP_<LANG>`;
- `ANVIL_KEYMAP_PRESET` and other per-setting aliases.

`[lsp.commands]` and per-root language profiles replace LSP environment
overrides. Existing users receive a warning through `settings check`,
`doctor`, and `:config-reload` for at least one minor release. Removal is
allowed no earlier than `0.8.0`, after the warning path and migration tests
land.

### 4. Source reporting distinguishes bootstrap from settings

The per-field origin display reports `default`, `user`, and `project`, plus
explicit settings-file override. It does not report an environment origin for
editor fields. A retained bootstrap variable is reported by its owning
subsystem, not as a settings source.

### 5. Keep the complete-file override

`ANVIL_SETTINGS` remains an explicit complete-file override with its current
trust and precedence semantics. This ADR does not remove the user/project
layers and does not claim that the 3,464-line settings file shrinks
proportionally.

## Consequences

Positive:

- One inspectable source for editor and language behavior.
- Origin reporting, `settings check`, and `:config-reload` become smaller and
  less ambiguous.
- Environment configuration used by CI and deployment remains supported.

Negative:

- Existing shell snippets and automation that set per-setting variables stop
  working after the removal release.
- CI, tests, and documentation must migrate to settings files or explicit
  setup commands.
- `settings.rs` still owns registry, schema, overlays, and doctor logic; the
  environment removal alone does not eliminate that file.

## Verification

- Focused tests:
  - a deprecated setting environment variable produces a warning and does not
    silently alter the resolved file setting;
  - `settings check` and `:config-reload` report `default` / `user` /
    `project` origins for editor fields;
  - `ANVIL_SETTINGS` still replaces the normal settings layers;
  - `[lsp.commands]` and per-root language profiles resolve in the documented
    order;
  - the retained bootstrap allowlist is explicit and tested.
- Failure/cancellation tests:
  - an invalid deprecated value does not fail startup;
  - an untrusted project settings file remains ignored with a reason.
- Real surface smoke:
  - `anvil settings check`, `anvil setup`, `doctor`, and the TUI settings
    overlay agree on the source of a changed value.

## References

- [Settings sources](../settings-sources.md)
- [ADR-0053](0053-settings-scopes-and-language-overlays.md)
- [ADR-0052](0052-language-tool-setup-and-source-resolution.md)
- [ADR-0067](0067-settings-file-hot-reload.md)

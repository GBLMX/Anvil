# ADR-0038: Project Settings Are Trust-Gated

- Status: Accepted
- Date: 2026-11-13
- Related: [ADR-0006](0006-upstream-dependencies.md), [ADR-0037](0037-kernel-api-baseline.md)

## Context

Anvil currently reads one global `settings.toml` and a set of `ANVIL_*`
environment variables. A repository cannot set its own theme, editor defaults,
or LSP preferences without changing the user's global file.

Ferriso, Anvil's predecessor, already demonstrated a layered configuration
model: built-in defaults, system file, user file, project file, explicit file,
and environment overrides. The useful part is the precedence and source
provenance, not a runtime dependency on Ferriso.

Project settings are also a code-execution boundary. `[lsp.commands]` names an
executable that Anvil will launch. A repository must not gain that authority
merely by being opened.

## Decision

1. Anvil adds one optional project layer at `<workspace>/.anvil.toml`.
2. The project layer is read only when the primary workspace root is trusted.
3. Precedence is:
   ```text
   built-in defaults
   < user settings.toml
   < trusted project .anvil.toml
   < ANVIL_SETTINGS
   < ANVIL_* environment variables
   ```
4. `ANVIL_SETTINGS` remains a complete file override. When it is set, neither
   the user file nor the project file is read. This preserves existing test and
   deployment behavior.
5. Project settings may set theme, editor, search, and UI keys, and
   `[lsp] inlay_hints`. They must not set `[lsp.commands]`; that table is
   rejected with the project path in the error.
6. Saving settings continues to write only the user file. Project settings are
   read-only.
7. The system-wide `/etc/anvil/settings.toml` layer is deferred. It has no
   consumer yet and would add another failure mode without solving a current
   problem.
8. No Ferriso crate becomes a dependency. The project layer uses Anvil's
   existing `toml_edit` and typed settings registry.

## Consequences

Positive:

- A repository can pin its theme and editor behavior.
- The trust boundary is explicit; untrusted roots cannot inject commands.
- Existing global settings and `ANVIL_SETTINGS` behavior remain valid.
- Settings parsing still rejects unknown sections and invalid values.

Negative:

- Editing a project setting requires editing `.anvil.toml` directly; the
  settings overlay saves only the user file.
- The primary workspace is trusted by construction, so opening a repository
  is still an implicit trust decision for non-command settings.
- Source provenance is not yet exposed in the UI; errors name the layer paths.

## Alternatives considered

### Read project settings for every root

Rejected. Untrusted secondary roots must not change process-wide settings.

### Allow `[lsp.commands]` in project settings

Rejected. It turns opening a repository into arbitrary executable selection.
A future per-project LSP command feature needs its own trust prompt and ADR.

### Use `ferriso-config` directly

Rejected. Anvil does not depend on Ferriso at runtime, and the project layer is
small enough to implement on the existing settings loader.

## Verification

- layered merge keeps user keys not overridden by the project;
- project layer is skipped when the root is untrusted;
- `[lsp.commands]` in a project file is rejected with its path;
- `ANVIL_SETTINGS` remains the complete override;
- full settings and TUI suites pass.

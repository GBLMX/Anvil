# ADR-0050: Discovery-First Defaults and Compiled Language Packs

- Status: Proposed
- Version: 1.0.0
- Date: 2026-11-17
- Related: [ADR-0025](0025-lsp-and-language-breadth.md),
  [ADR-0032](0032-language-tooling-service.md),
  [ADR-0040](0040-helix-keymap-compatibility.md),
  [ADR-0048](0048-per-root-language-tooling-and-lsp-workspace.md),
  [ADR-0049](0049-editor-control-plane.md)
- Supersedes:
- Superseded by:

## Context

Anvil's core editing engine and LSP surface are already strong, but the
first-use experience is weaker than the engine. A new user can find a file,
edit, and save, yet has to discover the remaining workflow from documentation:
diagnostics, jumps, changed files, language-server readiness, formatters, and
test/debug adapters are not presented as one coherent daily workflow.

LazyVim demonstrates a useful product pattern:

- a small set of curated defaults instead of an empty editor;
- `which-key`-driven discovery of leader groups;
- pickers for diagnostics, jumps, commands, keymaps, and search history;
- language "extras" that bundle syntax, LSP, formatting, linting, DAP, and
  tests for one language;
- a health/readiness surface that explains missing tools;
- a starter configuration that users override rather than assemble from zero.

LazyVim gets these properties from `lazy.nvim` plugin specs, Mason-managed
tools, and Neovim's Lua configuration model. Anvil cannot copy that runtime
model: it has compiled-in Rust modules, a sandboxed plugin host, no dynamic
Rust ABI, no in-process plugin UI, and no automatic download authority.

If Anvil defers this work, it keeps a capable engine with a high discovery cost.
If it copies LazyVim's runtime model, it weakens its distribution and security
boundaries. The decision must borrow the product pattern, not the plugin
manager.

## Decision

### 1. Make discoverability a first-class editor surface

Anvil keeps its existing selection-first core, TOML settings/keymap, and
compiled distribution. It adopts a discovery-first UX:

- every non-trivial command belongs to a documented keymap group;
- `which-key` shows the group name before the individual command;
- each picker has a command and a keymap path, not only a hidden implementation;
- the default leader groups remain stable and are documented in one place.

This is a UX decision, not permission to add a second command router.

### 2. Add the missing daily pickers

The first picker set is:

- diagnostics;
- jump list;
- changed files;
- command history;
- keymap list;
- search history.

Existing pickers and overlays are reused. Every picker must support cancel,
bounded result counts, stale-response rejection, and no hidden fallback that
mutates state. This closes the discovery gap identified in ADR-0040 without
importing Telescope, fzf-lua, or Snacks.

### 3. Add a language readiness surface

Anvil adds a read-only `:languages` surface. For each built-in or configured
language it reports:

- file extensions and Tree-sitter grammar status;
- LSP profile and server resolution;
- formatter and linter availability;
- optional test/debug provider availability;
- the active root/profile and the missing piece.

The surface never installs, downloads, or edits `PATH`. `anvil doctor` remains
the installation diagnostic; `:languages` explains the editor's language
readiness.

### 4. Define language packs as build-time compositions

A language pack is a feature-gated build composition, not a downloaded plugin
or a runtime bundle. A pack may include:

```text
syntax grammar
LSP profile(s)
formatter(s) and linter(s)
optional test/debug provider
recommended docs and keymap examples
```

The first packs are Rust, TypeScript/JavaScript, and C/C++. Packs are enabled at
build or distribution composition time. Selecting a pack may require a rebuild;
the UI must say so instead of implying a runtime install.

No pack may auto-install a server, execute a downloaded grammar, or bypass the
gateway/plugin trust boundary.

### 5. Keep Anvil's configuration model

Anvil does not add a Lua configuration layer. User overrides remain:

- `$XDG_CONFIG_HOME/anvil/settings.toml`;
- `$XDG_CONFIG_HOME/anvil/keymap.toml`;
- trusted project `.anvil.toml`;
- `:config-reload` for atomic reload.

A starter workspace and tutorial are documentation/onboarding surfaces, not a
second configuration runtime.

### 6. Do not copy the LazyVim runtime

Rejected for this decision:

- `lazy.nvim`-style runtime plugin loading;
- Mason-style automatic language-server installation;
- Telescope/fzf-lua/Snacks as required dependencies;
- copying the full LazyVim keymap set without resolving Anvil conflicts;
- measuring capability by plugin count.

## Consequences

Positive:

- New users see the next action instead of reading the whole manual.
- Language readiness failures become actionable before the first edit.
- Language packs give a coherent workflow without adding runtime download
  authority.
- The existing compiled distribution and security model remain intact.

Negative:

- Language packs need build-matrix and documentation maintenance.
- Pickers add bounded UI and cancellation tests.
- A language pack that requires a rebuild is less convenient than a runtime
  extra; the UI must state that tradeoff.
- Default keymap groups become part of the compatibility surface and need
  deliberate changes.

## Implementation status (2026-11-18)

- Landed: diagnostics (`Space d`), changed-files (`Space g`), jump-list
  (`Space j`), search-history (`Space H`), and command-history (`Space :`)
  pickers. `Space K` / `:keymaps` opens a keymap list for normal commands,
  leader/pending bindings, built-in keys grouped by overlay, and configured
  context overrides; `:keybinds` remains the keymapped-commands palette
  subset. `Space L` / `:languages` reports syntax grammar availability, LSP
  profile/root/server lookup, formatter fallback, and explicitly unconfigured
  linter/test/debug surfaces. Build-time packs are exposed as
  `language-pack-rust`, `language-pack-typescript`, and
  `language-pack-c-family`; `full` enables all.

## Verification

- Picker tests:
  - each picker opens, filters, selects, and cancels;
  - result caps and stale generations are enforced;
  - no picker writes files or bypasses the workspace boundary.
- Language readiness tests:
  - missing parser, LSP, formatter, linter, and provider are reported
    separately;
  - a trusted root uses its own profile;
  - no network or PATH mutation occurs.
- Feature matrix:
  - each language pack builds with its feature;
  - default composition still builds without optional grammars;
  - `full` includes the documented packs.
- Discovery smoke:
  - press the leader key and navigate to every first-picker group;
  - `:languages` on a workspace with one missing tool;
  - `:help` and starter docs match the shipped defaults.
- Performance:
  - startup and idle memory do not regress when optional packs are disabled.

## References

- [LazyVim configuration](https://www.lazyvim.org/configuration) — layered
  `config`/`plugins` overrides and defaults.
- [LazyVim keymaps](https://www.lazyvim.org/keymaps) — leader-group and
  which-key discovery pattern.
- [LazyVim extras](https://www.lazyvim.org/extras) — language/scenario extras
  enabled as a set.
- [LazyVim repository](https://github.com/LazyVim/LazyVim) — implementation of
  defaults, extras, and health surfaces.
- [lazy.nvim](https://github.com/folke/lazy.nvim) — the runtime plugin model
  this ADR explicitly does not adopt.

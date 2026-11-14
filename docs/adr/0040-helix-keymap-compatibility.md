# ADR-0040: Helix-Compatible Keymap Preset and Command Contexts

- Status: Accepted
- Implementation: `helix` preset, shared pending contexts for hover/comment,
  Space aliases, tests, and documentation landed.
- Date: 2026-11-14
- Related: [ADR-0019](0019-terminal-keyboard-enhancement.md),
  [ADR-0024](0024-tui-state-and-domain-boundaries.md),
  [ADR-0037](0037-kernel-api-baseline.md)

## Context

Helix users have established Space-leader muscle memory for file, buffer,
symbol, diagnostic, picker, and LSP actions. Anvil already covers seven of the
most common bindings:

| Helix | Anvil |
|---|---|
| `Space f` | `picker.fuzzy` |
| `Space e` | `tree.open` |
| `Space b` | built-in buffer picker |
| `Space s` | `lsp.document_symbols` |
| `Space S` | `lsp.workspace_symbols` |
| `Space r` | `editor.rename_symbol` |
| `Space a` | `lsp.code_action` |

The remaining Helix bindings cannot be copied wholesale because several
actions are missing or conflict with established Anvil defaults:

- Anvil uses `Space p` for the command palette; Helix uses `Space p` to paste.
- Anvil uses `Space F` for formatting; Helix uses `Space F` for a
  working-directory file picker.
- Anvil uses `Alt-c` to add a cursor above; Helix uses `A-c` for line comments.
- Anvil has no diagnostic picker, changed-file picker, jumplist picker,
  last-picker state, buffer-directory tree mode, or nested `Space w` prefix.
- Anvil supports line comments only; block comments are not implemented.

There is also a command-registry constraint. `execute_pending_command` asserts
that a pending command's ID is registered for `CommandContext::Pending`.
`lsp.hover` and `editor.toggle_comment` are currently registered for
`CommandContext::Normal` only. Adding `PendingCommand` variants without
updating their registry contexts would panic in debug builds and tests.

The decision must add Helix compatibility without turning the keymap into a
second routing system or silently breaking existing Anvil bindings.

## Decision

### 1. Add an optional `helix` preset; keep `ergo` as the default

Add `crates/anvil-tui/presets/anvil-helix.toml`. Selecting it must be opt-in:

```text
ANVIL_KEYMAP_PRESET=helix anvil
```

The preset is an additive overlay. Built-in Space bindings such as `Space b`
remain available even when they cannot be expressed in the preset because the
buffer-picker command has no stable `from_id` mapping yet.

### 2. First version adds four bindings

| Binding | Command ID | Kind |
|---|---|---|
| `Space h` | `lsp.references` | preset only |
| `Space ?` | `command_palette.open` | preset only |
| `Space k` | `lsp.hover` | new pending variant |
| `Space c` | `editor.toggle_comment` | new pending variant |

`Space h` and `Space ?` already resolve through
`PendingCommand::from_id`, so they need no new command code.

`Space k` and `Space c` need `PendingCommand::Hover` and
`PendingCommand::ToggleComment`, `id()`, `from_id()`, and dispatch. Their
implementation reuses the existing normal-mode behavior:

```rust
PendingCommand::Hover => model.queue_effect(Effect::LspHover),
PendingCommand::ToggleComment => model.toggle_comment(),
```

### 3. Register the shared command IDs for both contexts

Do not add pending-specific command IDs. The action is the same command in a
different input context.

In `crates/anvil-session/src/command.rs`, add:

```rust
const NORMAL_PENDING: &[CommandContext] =
    &[CommandContext::Normal, CommandContext::Pending];
```

Change:

```rust
spec(HOVER, NORMAL),
spec(TOGGLE_COMMENT, NORMAL),
```

to:

```rust
spec(HOVER, NORMAL_PENDING),
spec(TOGGLE_COMMENT, NORMAL_PENDING),
```

This makes `PendingCommand::id()` and `from_id()` consistent with the registry
and keeps the debug assertion valid.

### 4. Do not create a Helix module

The existing architecture already has the right seams:

```text
preset TOML -> PendingCommand -> keys/normal.rs dispatch
```

A separate Helix module would duplicate command resolution, preset merging,
and dispatch. New behavior belongs in the existing keymap and command
registry.

### 5. Preserve current defaults and conflicts

Do not remap:

- `Space p` away from the command palette;
- `Space F` away from formatting;
- `Alt-c` away from add-cursor-above.

A Helix user who wants `Space p` paste or `Space F` cwd picker needs those
underlying pending commands first; neither has a `PendingCommand::from_id`
entry today. The preset must not break existing users.

### 6. Defer features that are not bindings

The following are out of scope until their underlying commands exist:

| Binding | Missing capability |
|---|---|
| `Space d` / `D` | problems-panel command ID / diagnostic picker |
| `Space y` / `Y` / `P` / `R` | selection-to-system-clipboard command |
| `Space /` | interactive workspace-search prompt |
| `Space w` | nested prefix / which-key |
| `Space j` | jumplist picker |
| `Space g` | changed-files picker |
| `Space '` | last-picker state |
| `Space E` | tree rooted at buffer directory |
| `Space C` | block comments |
| `Space G` | experimental debug binding |

Adding a preset entry before the command exists would fail keymap validation
or produce a dead binding.

## Alternatives considered

### Copy the entire Helix keymap

Rejected. It would overwrite Anvil defaults, conflict with `Space p`,
`Space F`, and `Alt-c`, and still leave unsupported pickers as dead keys.

### Create a `helix` compatibility module

Rejected. It would duplicate the existing preset, command-ID, and dispatch
layers. The preset is data; the command variants are the only code needed.

### Add pending-specific command IDs

Rejected. `lsp.hover.pending` and `editor.toggle_comment.pending` would add
duplicate registry entries for identical behavior. Extending the existing
command contexts is smaller and keeps one stable ID per action.

### Make Helix the default keymap

Rejected. Existing `ergo` and `vim` muscle memory, documentation, and tests are
already shipped. Compatibility is opt-in.

## Consequences

Positive:

- Helix users get the four highest-value aliases without a new subsystem.
- The command registry remains the single source of truth for what a pending
  binding may execute.
- Existing presets and default bindings are untouched.
- Deferred bindings are documented as missing capabilities rather than
  silently doing nothing.

Negative:

- `HOVER` and `TOGGLE_COMMENT` now have two input contexts; tests must keep
  that registration explicit.
- The preset is not a complete Helix remap; some Helix actions remain absent
  until their features land.
- `Space b` remains a built-in-only binding because it lacks a stable command
  ID.

## Implementation plan

1. Add pending contexts for `HOVER` and `TOGGLE_COMMENT`.
2. Add `PendingCommand::Hover` and `PendingCommand::ToggleComment`.
3. Wire `id()`, `from_id()`, and dispatch.
4. Add `anvil-helix.toml` and register the `helix` preset.
5. Add preset parsing, resolution, and behavior tests.
6. Update `docs/keymap.md` and `help.txt`.
7. Run focused tests, workspace tests, clippy, architecture check, formatting,
   and diff checks.

## Acceptance

- `ANVIL_KEYMAP_PRESET=helix` loads successfully.
- `Space h` resolves to references.
- `Space ?` resolves to the command palette.
- `Space k` resolves to hover without a debug assertion.
- `Space c` toggles line comments through the existing comment engine.
- `ergo` and `vim` presets retain their current behavior.
- No keymap validation error is introduced by the new preset.

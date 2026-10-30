# ADR-0008: Selection-First Native Interaction Model

- Status: Accepted for Phase 0
- Date: 2026-10-30
- Related: [ADR-0003](0003-idl.md), [ADR-0004](0004-text-model.md)

## Context

Anvil needs one interaction model before the editor core and plugin API harden.
The two realistic starting points are:

- a Vim-style modal core with operator-pending state (`d`, `c`, `y` waiting for
  a motion);
- a selection-first core where every action has an explicit target.

A modal core is familiar, but it adds a second state machine on top of the text
buffer. Operator-pending state interacts with counts, registers, pending keys,
multi-selection, undo grouping, and plugin keymaps. Getting that state machine
right before the editor itself exists would make the first native model the most
complex one.

The selection model is smaller: motions change a selection, and actions consume
the current selection. A caret is an empty selection. `w`, `d`, `c`, and `y`
have direct semantics without an operator stack. It also gives future
multi-selection and multi-cursor work one shared primitive instead of a separate
mode-specific path.

## Decision

1. **Phase 0 ships exactly one native interaction model: selection-first.** The
   core has no normal/insert/visual mode split and no operator-pending stack.
2. **Motions and actions are native core semantics.** Selection extension,
   deletion, change, yank, put, undo grouping, and multi-selection invariants
   belong in Rust. Plugins do not define what `word`, `delete`, or `change`
   means.
3. **Key mappings are plugins.** Lua/TS keymap plugins bind input events to core
   commands. The core does not embed Vim, Helix, or Emacs key tables.
4. **A future Vim adapter composes the native primitives.** It may add an
   operator-pending layer that temporarily stores a motion and then invokes the
   native action, but it does not replace the core action engine.
5. **Interaction mode is session state, not process-global state.** The editor
   snapshot carries an `interaction_mode` value. Phase 0 has one value,
   `select`.
6. **The plugin API exposes a queryable and switchable session mode.**
   `editor.get_view` returns `interaction_mode`; `editor.set_mode` changes it
   under the `editor.mode` permission. Phase 0 accepts only `select`, and
   unknown mode values are rejected. The mode is explicit session state, not a
   plugin-defined mode or process-global switch.
7. **Multiple modal kernels and multi-cursor are later features.** They are not a
   Phase 0 architecture burden. The selection primitive must not preclude them.
8. **Vim `dd`/`yy` lower to selection-first composition.** A keymap may expose
   them, but the core has no separate line-delete or line-yank primitive.
   `dd` selects the current line, including its trailing line break when one
   exists, then deletes that selection; the caret remains at the original
   line-start offset rather than advancing as in Vim. `yy` selects the current
   line under the same rule and yanks it. The minimal unnamed register is
   implemented; named registers and system clipboard are later additions.

## Consequences

Positive:

- One state machine owns editing semantics.
- Actions have explicit targets and deterministic undo boundaries.
- Multi-selection can share the same motion/action engine.
- Keymap plugins can be replaced without changing core semantics.
- Vim compatibility remains possible as an adapter, not a rewrite.

Negative:

- The first keymap will feel less Vim-native until an adapter exists.
- Adding a second native mode is an additive API version bump, not a new
  transport method.
- Keymap latency depends on the plugin transport; core editing must not.

## Alternatives considered

### Vim-first modal core (rejected for Phase 0)

Directly implements the familiar model, but makes operator-pending state part of
the first core state machine and complicates selection, undo, and multi-cursor
interactions before they are measured.

### Two native kernels in Phase 0 (rejected)

A selection kernel plus a modal kernel doubles the action execution paths and
creates ambiguous ownership of undo and selection state. Add a second kernel
only after the first has tests and a measured need.

### Plugins define motion/action semantics (rejected)

Would let keymap plugins diverge on word boundaries, selection direction, and
undo grouping. Those invariants belong to the Rust core.

## Verification expectations

- Core motion/action tests cover empty and non-empty selections, direction,
  grapheme and display-cell boundaries, and undo grouping.
- No Phase 0 test path may require an operator-pending state.
- View serialization includes `interaction_mode`; an unknown value is rejected.
- `editor.set_mode` rejects unknown modes and calls without `editor.mode`.
- Keymap plugin tests exercise binding only; they do not redefine motion
  semantics.

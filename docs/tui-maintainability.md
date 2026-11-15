# TUI Maintainability Budget

Status: Active; Phase 0 baseline recorded.

The TUI is intentionally one crate until a second consumer or an independent
boundary is measured. This budget tracks structural coupling instead of treating
file count or crate count as a goal.

## Current baseline

| Metric | Baseline |
|---|---:|
| Rust source files | 102 |
| Source lines | 47,792 |
| Normal dependencies | 30 |
| `impl Model` blocks | 66 |
| Files containing `impl Model` | 61 |
| Methods taking `&mut self` | 513 |
| `Model::new` constructions | 431 |
| Files over 800 lines | 15 |

`TUI_LOC` is informational because feature work can change it. The structural
metrics are budgets: new page state should not add another `impl Model` or
another full-`Model` test construction.

## Commands

```bash
scripts/tui-metrics.sh
scripts/tui-metrics.sh --check
```

`--check` reads `docs/tui-maintainability.baseline` and fails if a structural
budget grows. A deliberate extraction that lowers a metric should update the
baseline in the same change.

## Target direction

- Page state owns its update logic and tests.
- `Model` composes domain state and routes cross-domain transitions.
- Page renderers read state; they do not start I/O.
- Runtime polling is grouped under domain state, not added to the terminal loop
  one service at a time.
- Crate extraction requires a second consumer, a clean state boundary, a wrong
  dependency edge, or measured build/ownership pressure.

See [ADR-0024](adr/0024-tui-state-and-domain-boundaries.md),
[ADR-0041](adr/0041-tui-primitive-extraction.md), and
[ADR-0042](adr/0042-selection-first-session-frontend.md).

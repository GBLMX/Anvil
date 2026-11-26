# Analysis Backlog

Status: Proposed. This is a research queue, not a release schedule. It does
not change [ROADMAP.md](../../ROADMAP.md), the internal-polish exit gates, or any
ADR status.

Last reviewed: 2026-11-18.

## Purpose

Record topics that need external reference research and a bounded Anvil
decision before implementation. Do not start several of these at once.

Each analysis should produce one of:

- a short design memo for a reversible local change;
- an ADR when the change defines a new contract, scope, or security boundary;
- a benchmark note when the decision depends on measured behavior.

## Current gates

These proposed ADRs are already queued. Finish or reject one before opening a
new large design track:

- [ADR-0053](../../adr/0053-settings-scopes-and-language-overlays.md) — explicit
  settings scopes, language overlays, and registry completeness;
- [ADR-0054](../../adr/0054-file-picker-query-and-index-lifecycle.md) — file picker
  query, ranking, and index lifecycle;
- [ADR-0055](../../adr/0055-start-dashboard-actions-and-context.md) — start dashboard
  actions, focus, and contextual recents.

They are not implementation authorization until accepted.

## P0 — daily editor workflows

### 1. Workspace search and replace

Current state: basic search and substitution exist; workspace-wide search,
result grouping, multi-file replace safety, and large-result behavior need a
coherent design.

Questions:

- internal search versus gateway/rg-backed search;
- streaming results, cancellation, and bounded result sets;
- result grouping by root/file and stable selection semantics;
- project replace preview, conflicts, rollback, binary/large files;
- trust and path confinement.

Research targets: Telescope `live_grep`, fzf-lua grep, Helix global search,
VS Code Search, ripgrep behavior.

### 2. Git workflow

Current state: diff, blame, hunk staging, conflict listing, and resolution
exist; a complete daily Git workflow is not designed.

Questions:

- hunk and line-level staging/unstaging;
- branch/upstream state and commit workflow;
- conflict resolution and recovery;
- which mutations must remain explicit and non-automatic;
- performance and stale status handling.

Research targets: lazygit, magit, gitless, VS Code Git.

### 3. Session recovery and undo boundaries

Current state: session restore, swap files, and crash backups exist; persistent
undo remains deferred by [ADR-0026](../../adr/0026-persistent-undo-decision.md).

Questions:

- restore versus recovery versus unsaved-buffer policy;
- swap conflict explanations and safe cleanup;
- crash recovery test matrix;
- whether persistent undo is worth the storage/versioning complexity.

Research targets: Vim swap/undo, Helix sessions, Zed restore, ADR-0026.

### 4. Search/replace semantics

Current state: local and whole-file substitution have bounded match limits;
multi-file semantics and Unicode/regex behavior need an explicit contract.

Questions:

- regex versus literal search by default;
- Unicode case folding and grapheme-safe ranges;
- multi-file transaction and rollback;
- discoverable operation report and failure behavior.

Research targets: Vim `:s`, Helix selection/replace, VS Code replace.

## P1 — discovery and language work

### 5. Command palette, which-key, and keymap consistency

Current state: command registry, keymap list, command history, and leader
groups exist; the command architecture remains partial.

Questions:

- one source for command ID, label, key hint, context, and availability;
- which-key versus command palette versus keymap list responsibility;
- conflict detection, aliases, and context precedence;
- discoverability without a second command router.

Research targets: Helix which-key, VS Code Command Palette, LazyVim discovery.

### 6. LSP advanced UX and performance

Current state: completion, diagnostics, symbols, code lens, semantic tokens,
call hierarchy, inlay hints, and per-root workers exist; advanced behavior and
performance budgets need validation.

Questions:

- multi-server merge rules;
- workspace diagnostics and progress;
- request cancellation, stale revisions, and error presentation;
- large-project latency and memory budgets.

Research targets: LSP 3.17 specification, Helix, Zed, VS Code.

### 7. Multi-root, trust, and language settings

Current state: per-root gateways/LSP and primary-root trust exist; settings and
recent behavior are not fully root-aware. ADR-0053 covers language settings.

Questions:

- secondary-root scope and ignored-project reasons;
- language overlays versus per-root LSP profiles;
- recent files/workspaces and current-root selection;
- trust transitions and cleanup.

Research targets: VS Code multi-root workspaces, ADR-0038, ADR-0048, ADR-0053.

## P2 — extension surface

### 8. Plugin UI and package UX

Current state: sandboxed plugin hosts, provider protocols, signed packages,
content-addressed store, install/uninstall/rollback exist; plugin UI and
marketplace surfaces are incomplete.

Questions:

- which UI contributions are safe and worth supporting;
- settings/command/status contributions without a second UI framework;
- package provenance, consent, enable/disable, and offline behavior;
- security boundary and failure modes.

Research targets: VS Code extension contributions, Zed extensions, existing
Anvil plugin ADRs.

### 9. DAP, test explorer, and terminal

Current state: trusted DAP adapters and provider protocols exist; full debug,
test-result, and terminal UX is not integrated.

Questions:

- adapter lifecycle and launch/attach configuration;
- test discovery/result protocol and failure navigation;
- PTY focus, paste, resize, and process cleanup;
- what remains trusted external process behavior.

Research targets: DAP specification, nvim-dap, VS Code Testing, VS Code
Terminal.

### 10. Accessibility, themes, and typography

Current state: reduced motion, high contrast, semantic themes, ASCII icon
fallback, and color detection exist.

Questions:

- screen-reader and terminal accessibility constraints;
- `NO_COLOR`, 16-color, CJK/emoji width, and low-contrast terminals;
- theme preview and persistence semantics;
- selected-state visibility without color alone.

Research targets: WCAG, terminal accessibility guidance, Helix themes.

## P3 — remote and release operations

### 11. Remote/SSH workspace behavior

Current state: remote workflow documentation exists; remote file access, LSP,
and terminal behavior are not a complete product contract.

Questions:

- path mapping and gateway ownership;
- remote LSP and language tool availability;
- credentials/trust and command execution boundaries;
- reconnect and degraded-mode behavior.

Research targets: VS Code Remote SSH, Helix remote workflow, Anvil
`docs/src/multi-host-install.md#remote-ssh-workflow`.

### 12. Release, update, and local telemetry

Current state: signed release workflow, installer, package store, and doctor
exist; marketplace/update UX is incomplete; AI telemetry HUD is a future plan.

Questions:

- update channels, rollback, and offline verification;
- consent/provenance for packages and updates;
- local EPM/APM/Burst telemetry privacy and retention;
- what data must never leave the machine.

Research targets: VS Code update/marketplace, Zed extensions, Anvil release and
telemetry plans.

## Cross-cutting method

For each selected topic:

1. State the concrete user workflow and failure case.
2. Inspect Anvil code, ADRs, tests, and known issues first.
3. Compare at least two mature editors or specifications.
4. Define the smallest reversible decision and its security/trust boundary.
5. Add measurable acceptance criteria; use a benchmark when performance is
   part of the claim.
6. Write a memo or ADR; do not implement from an unaccepted design.

# ADR-0055: Start Dashboard Actions, Focus, and Contextual Recents

- Status: Accepted; implemented 2026-11-19
- Version: 1.0.0
- Implementation: dashboard focus/selection, command-backed actions, contextual
  recents, missing/untrusted state, mouse parity, and session Continue landed.
  Cross-workspace switching now uses the process handoff in ADR-0062 instead
  of silently opening a second workspace.
- Date: 2026-11-18
- Related: [ADR-0024](0024-tui-state-and-domain-boundaries.md),
  [ADR-0040](0040-helix-keymap-compatibility.md),
  [ADR-0050](0050-discovery-first-language-packs.md),
  [ADR-0051](0051-external-editor-reference-models.md),
  [ADR-0054](0054-file-picker-query-and-index-lifecycle.md)
- Supersedes:
- Superseded by:

## Context

The current start dashboard already follows the local TUI design:

- wide terminals use `Start` and `Recent` panels;
- narrow terminals use one column;
- recent files are limited to openable files in trusted roots;
- recent files support mouse activation;
- no large ASCII logo is used;
- the dashboard is an overlay, not a second editor state.

It is still a mostly static welcome card:

- The action list is hardcoded text and key labels.
- Except for `r`, `?`, and `Esc`, any key closes the dashboard and is then
  processed by normal mode.
- Actions cannot be selected with `j`/`k`, activated with `Enter`, or clicked.
- Recent files are clickable but cannot be selected from the keyboard.
- Recent workspaces are display-only.
- Recent workspaces show only the basename, so two different `WS` paths are
  indistinguishable.
- `r` opens recent files; there is no equivalent action for recent
  workspaces.
- Missing or untrusted recent paths are not explained.
- First-run, returning-user, and restorable-session states share the same
  static content.

External products support a more useful model:

- Zed opens a Welcome page in the center pane when no folder is open, offers
  quick actions such as opening a folder or viewing documentation, and removes
  the page once a file or folder is opened.
- Snacks dashboard uses declarative sections for keys, projects, recent files,
  sessions, startup, and terminal output. Section items can bind commands,
  keymaps, or functions.
- VS Code uses its Welcome page for onboarding/walkthroughs, while file and
  folder navigation remain separate Quick Open/Open Recent surfaces.

Anvil should adopt the actionable dashboard shape without copying a plugin
dashboard framework or turning the start page into a second command palette.

## Decision

### 1. Keep the start page an overlay

The dashboard remains `OverlayKind`/start-page state rendered over the empty
editor. It does not become a second editor, a file picker, or a persistent
workspace page.

The existing wide two-column and narrow one-column layouts remain. Large ASCII
art, animation, and decorative dashboard sections are not added.

### 2. Give the dashboard an explicit focus model

`StartPageState` gains:

```text
focus: Actions | Files | Workspaces
action_selected: usize
file_selected: usize
workspace_selected: usize
```

Interaction:

| Key | Action |
|---|---|
| `Tab` / `Shift-Tab` | move focus between Actions, Files, and Workspaces |
| `j` / `k` or `Up` / `Down` | move within the focused section |
| `Enter` | activate the selected action or recent entry |
| mouse click | focus and activate the clicked row |
| `Esc` | close the dashboard |
| `?` | open help |
| `:q` | quit |

A normal key that is not part of the dashboard key contract must not silently
dismiss the dashboard. This replaces the current “any unmatched key closes the
dashboard” behavior.

The dashboard remains keyboard-first, but every clickable recent file gains the
same keyboard path.

### 3. Drive actions from the command registry

The left action list is a curated set of command IDs, not duplicated command
strings. The dashboard resolves label, key hint, availability, and activation
from the existing command registry/keymap metadata.

Initial actions:

| Key | Command / behavior |
|---|---|
| `i` | insert/new buffer |
| `Space f` | find file |
| `Space e` | file tree |
| `Space b` | buffers |
| `Space p` | command palette |
| `r` | recent files |
| `w` | focus recent workspaces |
| `?` | help |
| `:q` | quit |

`r` remains the recent-files fuzzy picker. `w` moves focus to the Workspaces
section rather than overloading the meaning of `r`.

Actions whose prerequisite is unavailable remain visible but disabled with a
reason; they are not silently removed. This keeps discovery stable while
making failures explainable.

### 4. Make recents contextual and actionable

The `Recent` panel has three optional sections:

```text
Continue        only when a restorable session exists
FILES           trusted recent files
WORKSPACES      canonical recent workspace paths
```

#### Continue

If the existing startup/session restore path reports a restorable session, show
one `Continue` row. Activating it uses the existing session restore path; the
dashboard does not parse session files itself.

#### Recent files

- Source: the existing `openable_recent_files()` filter for trusted roots.
  That filter does not prove the file still exists, so the dashboard resolves
  existence through the existing workspace/file metadata path before enabling
  a row.
- Missing rows are marked `[missing]` and disabled; the user learns the reason
  immediately instead of discovering it only when `Enter` fails.
- Display: workspace/root label plus a shortened relative path; the filename
  remains identifiable when the path is truncated.
- Activation: open in the current pane.
- File rows support split/new-buffer actions only where the editor already has
  a stable command for them.
- Untrusted files are not shown as openable; if displayed for explanation,
  they are explicitly marked and disabled.

#### Recent workspaces

- Deduplicate by canonical path.
- Display at least the final two path components, plus the canonical path when
  space permits; basename alone is insufficient.
- Mark the current workspace.
- Mark `missing`, `untrusted`, or `unavailable` paths and refuse activation.
- Activating a valid workspace uses the existing workspace open/trust flow; the
  dashboard must not auto-trust a project.

The derived lists remain owned by the existing recent/session state. The
dashboard does not maintain a second persistent copy.

### 5. Add explicit empty and first-run states

The dashboard content depends on available context:

- no recent files: show `Space f find file` as an actionable hint;
- no recent workspaces: show the workspace-open/help path;
- first run: emphasize file finding and tree navigation;
- returning run with recents: emphasize Continue, files, and workspaces;
- untrusted/missing workspace: show the reason rather than a generic empty
  state.

Empty states remain short and actionable. They do not become onboarding
walkthroughs or permanent documentation pages.

### 6. Preserve trust, accessibility, and terminal constraints

- Recent files continue to come only from trusted roots.
- Recent workspaces do not bypass the existing trust check.
- Selected rows use the established selected surface and never rely on color
  alone.
- Icons remain optional with ASCII fallback.
- No non-portable transparency, shadow, or animation is introduced.
- The status line remains the place for errors; the dashboard is not a
  notification surface.

### 7. Defer configurable dashboards

The following are not part of this decision:

- user-configurable dashboard sections;
- plugin-contributed dashboard items;
- profiles or multiple named dashboards;
- remote workspace/clone UI;
- a full recent-file preview inside the dashboard;
- a second command palette.

They require a separate ADR with a concrete consumer.

## Consequences

Positive:

- the start page becomes usable without remembering static key hints;
- recent files and workspaces gain keyboard and mouse parity;
- duplicate workspace names become distinguishable;
- first-run and returning-user content becomes contextual;
- actions no longer drift from the command registry;
- the dashboard remains a bounded overlay rather than a second UI system.

Negative:

- `StartPageState` gains focus and selection state;
- command availability and disabled reasons must be represented;
- workspace rows need canonical-path and trust/missing metadata;
- the start page gains more tests and mouse hit regions;
- session “Continue” depends on the existing restore contract.

## Verification

Focused tests:

- `j`/`k`, `Tab`, and `Enter` work in each dashboard section;
- any non-dashboard key does not close the dashboard;
- mouse activation of actions, files, and workspaces;
- `r` opens recent files; `w` focuses recent workspaces;
- duplicate basenames with different canonical paths are distinguishable;
- current, missing, and untrusted workspace rows render and behave correctly;
- recent files remain filtered by trusted roots;
- a deleted recent file renders as `[missing]` and cannot invoke open/split;
- empty recent states show actionable hints;
- narrow and wide layouts retain bounded panels and selected rows;
- `?` returns to the dashboard after help closes;
- `Esc` and `:q` retain their existing behavior;
- selected indexes survive resize and list changes without out-of-range access;
- unavailable actions show a reason and perform no effect.

Manual checks:

- first launch with no recents;
- returning workspace with several recent files;
- multi-root workspace with duplicate basenames;
- delete a recent file while the dashboard is closed and confirm the row is
  marked `[missing]` and disabled on return;
- attempt to activate an untrusted recent workspace;
- click and keyboard activation produce the same result.

## References

- [Zed Getting Started — Welcome Page](https://zed.dev/docs/getting-started)
  — contextual welcome page with quick actions, disappearing after a project
  is opened.
- [Snacks dashboard documentation](https://github.com/folke/snacks.nvim/blob/main/docs/dashboard.md)
  — declarative sections, section actions, recent files/projects, and session
  entries.
- [VS Code Tips and Tricks — Welcome](https://code.visualstudio.com/docs/editing/getting-started/tips-and-tricks)
  — Welcome page as onboarding/walkthrough surface distinct from day-to-day
  file navigation.
- [ADR-0024](0024-tui-state-and-domain-boundaries.md)
  — start-page state ownership and overlay boundary.
- [ADR-0050](0050-discovery-first-language-packs.md)
  — discovery-first defaults and curated action surface.
- [ADR-0054](0054-file-picker-query-and-index-lifecycle.md)
  — file-picker query and recent-file interaction boundary.

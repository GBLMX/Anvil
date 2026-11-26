# ADR-0067: Settings File Hot Reload

- Status: Accepted; implemented 2026-11-20
- Version: 1.0.0
- Date: 2026-11-20
- Related: [ADR-0045](0045-settings-patch-layers.md),
  [ADR-0046](0046-full-workspace-watch.md),
  [ADR-0053](0053-settings-scopes-and-language-overlays.md),
  [ADR-0063](0063-command-architecture-and-execution-boundary.md)
- Supersedes: the “external settings hot reload remains manual/out of scope”
  clauses in ADR-0023, ADR-0045, ADR-0046, and ADR-0053 for settings and
  language files only
- Superseded by:

## Context

`:config-reload` reloads settings, keymap, and theme together. Settings and
language files change during normal editing, and requiring an explicit command
makes the Settings overlay and external file editing diverge. The earlier ADRs
deferred hot reload until a watcher boundary existed.

The workspace watcher is not the right transport here: it is scoped to
workspace paths and can be disabled with `editor.file_watching`, while user
settings live outside the workspace. A settings reload must also preserve the
last valid configuration on a partially written or invalid file.

## Decision

### 1. Poll bounded settings files

The TUI polls a small, explicit set of settings files once per second and
compares their bytes:

- the user `settings.toml`, or the explicit `ANVIL_SETTINGS` file;
- the trusted primary-root `.anvil.toml`;
- the user `languages.toml`;
- the trusted primary-root `.anvil/languages.toml`.

When `ANVIL_SETTINGS` is set, it is the complete override and only that file is
watched. Project files are watched only while the primary root is trusted. The
watch set is synchronized after a trust change so a newly trusted project file
is picked up.

No new watcher dependency is added. The files are small; polling avoids a
second notification transport and works on files that do not exist yet.

### 2. Reload settings and language overlays only

A change applies the same validated layer pipeline used at startup:
`sources`, sparse user/project patches, inherited values, language overlays,
and capability/validation checks. LSP configuration is notified through the
existing path.

The keymap remains explicit: keymap files are not part of this watcher.
Custom theme files already have their own reload path. A theme value inside
`settings.toml` is a setting and therefore follows this decision.

### 3. Preserve the last valid configuration

An invalid, partial, or unreadable file is not applied. The current settings
and language overlays stay active, the status line reports the reload error,
and the watcher refreshes its fingerprints so one bad write does not spam every
poll. The next content change retries.

On success the status line reports `settings reloaded from ...` and the new
configuration is active.

### 4. Do not clobber an open Settings overlay

If the Settings overlay is open, a pending external change is recorded but not
applied. The overlay keeps its draft and explicit interaction. After the
overlay closes, the pending change is applied. This prevents a background
reload from discarding or silently mixing with in-progress edits.

### 5. Trust and path changes

The watcher recomputes its path set from the current primary-root trust state.
Adding or removing a watched path triggers the same reload path. Project
settings remain unreadable and unwatched while the root is untrusted.

## Consequences

Positive:

- Settings and language overlays update without a manual command.
- Invalid writes preserve the last good configuration.
- The Settings overlay cannot be clobbered by a background reload.

Negative:

- There is up to one second of latency.
- Settings files are read once per second while the TUI runs.
- Keymap files still require `:config-reload` or a restart.
- Only the primary project settings/languages files are watched; secondary
  roots do not contribute process-wide settings.

## Verification

- Unit test covers content change, path addition/removal, and deletion.
- Invalid TOML leaves the old settings active and reports an error.
- Trust change adds project files to the watch set.
- Open Settings overlay defers the reload until it closes.
- Real TUI smoke edits `settings.toml`, observes `settings reloaded`, and
  confirms the setting applies.
- `:config-reload` remains available for an explicit full reload.

# Theme and Settings Plan

Status: active. Built-in themes and built-in Settings only; plugin-provided
themes or plugin-rendered Settings are out of scope. Theme work does not
require a full Settings page; the generic Settings overlay is a later gate.

Scope: semantic themes, built-in palettes, theme selection/persistence, live
preview, and the minimum settings infrastructure needed to support them. It
does not add plugin-owned UI or a marketplace.

## 0. Decision

Built-in is the selected direction. Themes are compiled into `anvil-tui` or
loaded from Anvil's own settings file; they are not plugin packages. Settings
pages remain built-in surfaces. Plugins may consume editor APIs, but they
cannot replace the theme registry or render Settings.

Priority relative to plugin work:

1. Close the already-started `0.6.1` package-store/update slice.
2. Built-in theme core and selection.
3. Built-in Settings overlay only when its gate is met.
4. Plugin marketplace; not before signing/provenance and the `0.5.x` feedback
   surface.

## 1. Goals

1. Replace the hardcoded palette in `crates/anvil-tui/src/theme.rs` with a
   semantic `Theme` value.
2. Ship several common built-in themes, including light and dark variants.
3. Select themes through `:theme`, the command palette, and a persisted
   settings file.
4. Preview a theme before applying it; cancelling restores the previous theme.
5. Keep keymap configuration separate; do not migrate `keymap.toml`.
6. Add a Settings overlay only when more than themes need editing.

## 2. Non-goals

- Plugin-provided themes or plugin-rendered Settings/TUI.
- Hot reload of settings files.
- Project-level settings layers, remote overlays, or SDK runtime settings.
- A full clone of OhMyPi's ten-tab Settings surface.
- Custom theme authoring in the first slice; built-ins first.

## 3. Current Anvil state

- `crates/anvil-tui/src/theme.rs` owns compile-time `Color` constants and
  `Style` helpers. All renderers depend on those constants.
- `crates/anvil-tui/src/keymap.rs` already provides the TOML configuration
  precedent: explicit path resolution, validation, presets, and defaults kept
  on parse failure.
- `crates/anvil-tui/src/command_palette.rs` provides the selection surface for
  a future `theme.select` command.
- There is no general settings registry and no theme configuration file.
- Anvil explicitly keeps UI built in; `docs/0.6.x-plan.md` states
  "No plugin-owned UI."

## 4. Design

### 4.1 Theme model

`Theme` stores resolved `ratatui::style::Color` values grouped by semantics:

```text
background: bg, surface, surface_alt, code_block
chrome:     border, text, muted, dim, accent, selection
status:     success, warning, error, magenta, white
syntax:     keyword, string, comment, number, function, type,
            property, constant, heading, link, code, emphasis,
            list, quote, table
```

Rules:

- Renderers read `Theme` methods, never module-level colors.
- Built-in themes are Rust constants, not runtime files.
- Custom themes are parsed into the same `Theme` type; invalid themes are
  rejected as a whole.
- A theme must declare every semantic token; inheritance is not allowed in
  the first slice.
- Light themes must set all backgrounds explicitly; do not rely on terminal
  defaults.

### 4.2 Theme selection and precedence

Anvil's first implementation is a deliberate subset of OhMyPi's layering
[OMP-1][OMP-2]:

```text
runtime (:theme command)
  > ANVIL_THEME
  > $XDG_CONFIG_HOME/anvil/settings.toml
  > built-in default
```

A runtime `:theme` or `:theme-auto` choice is stored as a `ThemeChoice`
(`Fixed(id)` or `Auto`) and written to `settings.toml` immediately.
`:theme-save` re-writes the current runtime choice explicitly; neither path
persists an env value. `ANVIL_THEME` selects a
fixed theme ID and is a non-fallback environment override; there is no
fallback-env variant until a concrete need exists.

Settings file:

```toml
[theme]
mode = "fixed" # fixed | auto
name = "anvil-dark" # fixed mode only
dark = "anvil-dark" # auto mode only
light = "solarized-light" # auto mode only
```

Resolution inside the file is explicit; `name` and `dark`/`light` are never
competing fallbacks:

| File state | Result |
|---|---|
| no `[theme]` | `Fixed("anvil-dark")` |
| `mode = "fixed"` or omitted | `Fixed(name)`, with `name` defaulting to `anvil-dark` |
| `mode = "auto"` | `Auto { dark, light }`, defaulting to `anvil-dark` / `solarized-light` |
| `mode = "auto"` plus `name` | invalid file; reject the whole file |
| `mode = "fixed"` plus `dark` or `light` | invalid file; reject the whole file |
| unknown key or unknown theme ID | invalid file; reject the whole file |

`ANVIL_THEME` is always `Fixed(id)` and bypasses file `mode`. A runtime
`:theme` or `:theme-auto` outranks it. With `mode = "auto"`, terminal
background detection selects `light` or `dark`; if detection is unavailable,
use `dark`.

Fallback semantics are split by lifecycle:

- **Startup:** the whole file is parsed and validated before any theme is
  applied. A malformed file, invalid mode, unknown key, or unknown theme ID
  uses the built-in `anvil-dark` as the startup default and reports the error
  in the status line. No partial application.
- **Runtime:** `:theme <unknown>` or a failed preview keeps the currently
  active theme and reports the error. It never resets to the built-in default.
- **Persistence:** `:theme`, `:theme-auto`, command-palette commits, and
  `:theme-save` validate the current choice and write atomically. A failed
  write leaves the active theme unchanged and reports the error; the previous
  file remains valid.

### 4.3 Built-in themes

Initial set:

| ID | Kind | Notes |
|---|---|---|
| `anvil-dark` | dark | Current palette, default |
| `tokyonight` | dark | Tokyo Night, shared Yazi/Helix palette |
| `omp-kde` | dark | omp KDE CachyOS palette |
| `helix-base16` | dark | Helix base16 default dark, adapted for AA |
| `one-dark` | dark | One Dark family |
| `gruvbox-dark` | dark | Warm, high-contrast |
| `nord` | dark | Cool blue-gray |
| `catppuccin-mocha` | dark | Mocha palette |
| `solarized-light` | light | Light counterpart |

Each theme must pass the existing WCAG-AA contrast test for normal text and
the status/syntax tokens used on their own backgrounds.

### 4.4 Settings overlay gate

Do not build a full Settings page in the theme slice. The first slice uses:

```text
:theme
:theme gruvbox-dark
Space p -> theme.select
```

A Settings overlay is justified only when at least three independent domains
exist (for example theme, editor behavior, and LSP). Its minimum shape is:

- Tabs: `Appearance`, `Editor`.
- Sections inside each tab; no arbitrary nesting.
- Rows generated from a schema registry, not hand-written UI lists.
- Boolean, enum, text, and action controls only.
- Type-to-search within the overlay.
- Changed-from-default markers and a reset action.
- No plugin-owned pages.

[OMP-3][OMP-4]

## 5. Inspiration sources

The following paths are from the installed OhMyPi `18.18.0` package at
`~/.bun/install/global/node_modules/@oh-my-pi/`.

| ID | Inspiration | Source | What Anvil reuses |
|---|---|---|---|
| OMP-1 | Typed settings registry | `pi-coding-agent/src/config/registry.ts` | Declare once, read through typed handles; runtime > env > overlay > project > global > default; fallback env yields to configured layers |
| OMP-2 | Effective-env and panel separation | `registry.ts` `#effectiveEnv()` / `layered()` | Runtime outranks env; Settings edits persistent layers, not env values |
| OMP-3 | Schema-to-UI host adapter | `pi-coding-agent/src/config/settings-ui.ts` | UI consumes generic entries/get/set/unset instead of importing every setting |
| OMP-4 | Generic tabbed overlay | `pi-tui/src/overlays/settings-selector.ts` | Tabs, section jump, search, changed counts, live preview hooks |
| OMP-5 | Settings list behavior | `pi-tui/src/components/settings-list.ts` | Searchable rows, section headings, changed markers, menu/editor controls |
| OMP-6 | Settings layer storage | `pi-coding-agent/src/config/settings.ts` | Layered persistence, provenance, atomic writes, keep-last-good reload |
| OMP-7 | Theme settings | `pi-coding-agent/src/modes/settings.ts` | `theme.dark` / `theme.light` are runtime-option string settings; effects apply the mapping |
| OMP-8 | Theme runtime and preview | `pi-tui/src/theme/theme.ts` | `previewTheme()` applies without persisting; cancel restores |
| OMP-9 | Built-in/custom theme loading | `pi-tui/src/theme/loader.ts` | Built-ins plus a user themes directory, schema validation, sorted availability |
| OMP-10 | Tab/section schema | `pi-tui/src/overlays/settings-defs.ts` | Fixed tab order, per-tab section order, conditional visibility |

What is intentionally not copied:

- `SETTING_TABS` includes ten tabs plus Plugins; Anvil starts with two at most.
- OhMyPi has hundreds of declarations and a large store; Anvil starts with
  theme settings and adds domains only when a second consumer exists.
- Plugin settings pages and remote/overlay layers are out of scope.
- OhMyPi's JSON theme format is not adopted wholesale; Anvil uses its existing
  TOML convention unless a concrete need for JSON wins.

## 6. Implementation slices

### 6.1 Theme core
Status: done. `Theme` owns resolved colors; renderers use accessors; the
current palette is `anvil-dark`.

- Add `Theme` and `ThemeSpec` types.
- Replace `theme::BG` and related constants with accessors or a bound
  `Theme` reference passed to renderers.
- Keep compatibility helpers only if they reduce churn; remove them before the
  slice closes.
- Add `anvil-dark` using the current palette.
- Add contrast tests for every built-in theme.
- Verification: existing TUI render tests pass; one render test per theme
  asserts background and foreground styles on representative widgets.

### 6.2 Selection and persistence
Status: done. `settings.toml`, automatic persistence from `:theme` /
`:theme-auto` and command-palette commits, `:theme-save`, `ANVIL_THEME`,
command-palette theme rows, preview, and cancel restore landed.

- Add `settings.toml` loading with explicit path resolution, modeled after
  keymap configuration.
- Add `:theme`, `:theme-save`, `:theme-auto`.
- Add `theme.select` to the command palette, with current theme marked.
- Add live preview and cancel/restore.
- Add `ANVIL_THEME`.
- Tests: fixed/auto schema matrix, unknown key, unknown theme, malformed TOML,
  startup default vs runtime keep-active, persistence, preview cancel, and
  auto light/dark fallback.

### 6.3 Built-in palette set
Status: done. Nine built-ins land: `anvil-dark`, `tokyonight`, `omp-kde`,
`helix-base16`, `one-dark`, `gruvbox-dark`, `nord`, `catppuccin-mocha`, and
`solarized-light`.

- Add `one-dark`, `gruvbox-dark`, `nord`, `catppuccin-mocha`,
  `solarized-light`.
- Verify light-theme contrast and status-line readability.
- Add visual smoke coverage for a light theme, a dark theme, and syntax
  highlighting.

### 6.4 Settings overlay gate

Status: the typed registry, three domains, and minimal `:settings` overlay
landed; hot reload remains gated.

Proceed only if accepted by the review gate:

- Add a minimal generic settings registry.
- Add an `Appearance` tab with theme rows and an `Editor` tab placeholder only
  if a real second domain exists.
- Reuse the command-palette selection primitives before adding new widgets.
- Tests: filtering, changed marker, reset, preview cancel, unknown setting.

### 6.5 Documentation and release

- Update `README.md` with theme selection and settings-file location.
- Add a `docs/theme.md` for theme tokens and custom-theme rules once custom
  themes are in scope.
- Update `CHANGELOG.md`.
- Run `cargo fmt --all`, workspace tests, clippy, architecture check, and the
  installed-binary smoke test.

## 7. Risks and decisions

1. **Theme refactor touches every renderer.**
   Mitigation: mechanical replacement first, then delete old constants; verify
   with render tests before adding palettes.
2. **Light themes expose assumptions.**
   Many widgets currently assume dark surfaces. Audit every explicit
   foreground/background pair before shipping a light default.
3. **Settings scope creep.**
   The Settings overlay is gated; themes must not be blocked on it.
4. **Env semantics.**
   Do not let `ANVIL_THEME` outrank a runtime `:theme` choice. This follows
   OhMyPi's corrected precedence, not the earlier env-first assumption.
5. **Custom themes.**
   Defer until built-ins are stable; require complete token sets and reject
   partial themes.

## 8. Acceptance criteria

- `anvil-dark` is behavior-identical to the current palette.
- At least five additional themes render correctly.
- `:theme`, `:theme gruvbox-dark`, and command-palette selection work.
- Preview changes the live UI; cancel restores the previous theme.
- Theme choice persists to `settings.toml` and survives restart.
- `ANVIL_THEME` overrides the file but not a runtime `:theme` choice.
- Runtime invalid theme names keep the active theme; startup invalid files use
  `anvil-dark`; both report an error without partial application.
- No plugin can render or replace the Settings UI.
- Full test/clippy/fmt/architecture checks pass.

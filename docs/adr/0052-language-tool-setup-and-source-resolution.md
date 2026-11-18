# ADR-0052: Explicit Language Tool Setup and Source Resolution

- Status: Accepted; implemented native providers 2026-11-19; managed downloads deferred
- Version: 1.5.1
- Implementation: shared source resolver, setup check/plan/apply, provenance state,
  language/settings diagnostics, and managed-state discovery landed. Managed
  downloads remain deferred as documented below.
- Date: 2026-11-18
- Related: [ADR-0020](0020-content-addressed-plugin-store.md),
  [ADR-0025](0025-lsp-and-language-breadth.md),
  [ADR-0032](0032-language-tooling-service.md),
  [ADR-0038](0038-project-settings-trust.md),
  [ADR-0048](0048-per-root-language-tooling-and-lsp-workspace.md),
  [ADR-0050](0050-discovery-first-language-packs.md),
  [ADR-0051](0051-external-editor-reference-models.md)
- Supersedes:
- Superseded by:

## Context

Anvil reports language readiness but has no setup path:

- `:languages` and `anvil doctor` are read-only;
- no `anvil setup` or `install-tools` command exists;
- `dist/anvil/install.sh` copies binaries and plugins but has no language-tool
  options;
- release layouts previously used the standard composition without `--features
  full`, so TypeScript/JavaScript and C/C++ syntax were absent; plain local
  builds still use that standard composition;
- `language-pack-rust` is empty because Rust syntax is always compiled;
- `[lsp.commands]`, `[lsp.roots.<path>.<lang>] command`, and
  `ANVIL_LSP_<LANG>`/`ANVIL_LSP` already select explicit server paths;
- managed plugins already use an XDG data directory, content-addressed storage,
  locks, and atomic state writes.

Language servers are external executables with ecosystem-specific sources:

- rust-analyzer is distributed through rustup, distro packages, and standalone
  releases;
- clangd packages are often older than the official release;
- typescript-language-server is distributed through npm and needs a compatible
  TypeScript package;
- gopls and Python servers are tied to their language toolchains.

A single installation order or a silent installer would add network,
supply-chain, PATH, and privilege behavior to an editor whose distribution and
sandbox model intentionally avoid those defaults. The CLI interaction itself
also needs a stable contract: prompts, progress, streams, and exit codes must
be predictable in a terminal and in CI.

## Decision

### 1. Add `anvil setup` as an explicit subcommand

`anvil setup` is a non-startup subcommand, not an editor startup step:

```text
anvil setup                    # default: check
anvil setup check              # short found/missing status
anvil setup plan               # full provider plan, no changes
anvil setup apply              # execute after confirmation
anvil setup apply --yes        # execute without confirmation
anvil setup plan --json        # machine-readable result
anvil setup --tool rust-analyzer
anvil setup --profile rust,ts
anvil setup --print-path       # print a shell snippet only
```

Supported flags:

| Flag | Behavior |
|---|---|
| `--json` | stdout contains one JSON document; progress and diagnostics go to stderr |
| `--plain` | stable one-record-per-line text; no dynamic layout |
| `--dry-run` | alias for `plan`; no network, no writes |
| `--yes` | skip confirmation; does not imply `apply` |
| `--no-input` | never prompt; fail if input or confirmation is required |
| `--quiet` | suppress non-essential human output; errors remain on stderr |
| `--verbose` | include provider output and command details |
| `--no-color` | disable ANSI color; also honor `NO_COLOR` |
| `--print-path` | print a reversible PATH snippet; no install |

`--quiet` and `--verbose` are mutually exclusive; using both is a usage error.

Rules:

- `check`, `plan`, and `--print-path` perform no network access and write no
  files;
- `apply` requires the explicit subcommand;
- prompts are allowed only when stdin is a terminal and `--no-input` is absent;
- `--json` implies non-interactive behavior: `apply --json` without `--yes`
  fails rather than prompting;
- `apply` never runs automatically from `:languages`, `doctor`, or startup;
- `install.sh` may print the `anvil setup` hint, but does not silently install
  language tools;
- `anvil --help` lists `setup`; `anvil setup --help` shows examples;
- the interface is line-based; a separate full-screen TUI wizard is not part of
  this decision;
- a separate `anvil-setup` binary is deferred. The subcommand reuses the
  existing distribution entrypoint and avoids a second catalog/version skew.

### 2. Define stream, prompt, progress, and exit-code behavior

#### Streams

- Human-readable primary output goes to `stdout`;
- progress, provider logs, warnings, and errors go to `stderr`;
- `--json` writes only the final JSON object to `stdout`;
- `--plain` keeps one record per line for scripts;
- `--json` and `--plain` are mutually exclusive; usage error otherwise;
- color is disabled when stdout/stderr is not a TTY, when `NO_COLOR` is
  non-empty, or when `--no-color` is passed.

Example human `plan` output on stdout:

```text
Language tools (linux/x86_64)

  rust-analyzer              missing  provider=rustup
    command: rustup component add rust-analyzer
    root: no  path: no change
  clangd                     found    /usr/bin/clangd 23.1.5

Summary: 1 found, 1 missing, 0 blocked
Nothing changed. Run `anvil setup apply` after reviewing.
```

#### Prompts

- Only prompt when stdin is a TTY;
- default answer is “no” for installation confirmation;
- `--yes` skips confirmation; `--no-input` forbids prompting;
- `--yes` and `--no-input` are separate: one confirms, one forbids interaction;
- system package commands that require root are printed and exit with status
  `3` by default; setup does not silently invoke `sudo`;
- `--yes` must not be accepted as a substitute for an explicit `apply`.
- `apply` preflights every requested tool; if any provider is blocked (root,
  missing prerequisite, or required confirmation), it exits `3` before changing
  anything.

#### Progress and cancellation

- print the first status line before any slow network operation;
- TTY: one spinner or single-line progress indicator;
- non-TTY: plain stage lines, never `\r` animation;
- v1 installs serially to keep output deterministic; parallel installs need a
  separate decision and interleaving tests;
- on failure, print the provider's relevant output and the full log path;
- Ctrl-C exits with `130` and leaves no active partial state;
- state writes use a private stage, lock, fsync, and rename.

#### Exit codes

| Code | Meaning |
|---|---|
| `0` | success, or nothing to install |
| `1` | provider, network, or installation failure |
| `2` | invalid invocation, flag combination, or missing required input |
| `3` | blocked: confirmation, root, offline, or missing prerequisite |
| `130` | interrupted by SIGINT |

The exit code is the stable interface; detailed failures belong in stderr and
the `--json` result.

### 3. Resolve explicit user choices before discovery

The setup and readiness surfaces must preserve the existing LSP override order:

1. `[lsp.roots."<root>".<lang>] command` from user settings;
2. `ANVIL_LSP_<LANG>` or `ANVIL_LSP`;
3. `[lsp.commands]` from user settings;
4. Anvil-managed tool directory;
5. `PATH` and common user tool directories, including mise/asdf shims.

The per-root profile is applied last in the current resolver and therefore has
the highest precedence. The environment override is checked before the global
`[lsp.commands]` table. Managed tools are inserted only after explicit user
choices and before generic PATH discovery.

Fallback discovery checks `~/.local/share/mise/shims`, `~/.asdf/shims`,
`$MISE_DATA_DIR/shims`, and `$ASDF_DATA_DIR/shims`, with custom data
directories before their defaults. This is executable discovery only:
Anvil does not parse project version files or select a toolchain version.
See [ADR-0048](0048-per-root-language-tooling-and-lsp-workspace.md).

If an explicit path is configured but missing or non-executable, setup reports
an error. It must not silently fall back to another server: the user selected a
specific executable.

`doctor` and `:languages` must use the same catalog and precedence, or clearly
label a PATH-only result. A server configured in `[lsp.roots]` must not be
reported as missing merely because it is outside `PATH`.

### 4. Use per-tool provider ordering

A tool manifest owns the ordered providers. The order is not global
“package manager first” policy.

| Tool family | Preferred provider order | Compatibility note |
|---|---|---|
| `rust-analyzer` | rustup component, official release, distro package | also check `rust-src` |
| `clangd` | recent distro/LLVM package or official standalone release | distro packages may lag; match project toolchain |
| `typescript-language-server` / `vtsls` | pinned npm install | also install a compatible `typescript` |
| `gopls` | pinned `go install`, distro package | match the Go toolchain |
| `pyright` / `pylsp` | pinned npm or pipx, distro package | choose one Python server |
| `jdtls`, `lua-language-server`, `zls` | distro package or official release | verify the required runtime |

The manifest is release-owned and compiled into the setup surface for v1. It is
not a remote registry and is never read from project files. Each provider is a
typed operation (`Pacman`, `Apt`, `Dnf`, `Rustup`, `Npm`, `Go`, `Pipx`,
`ReleaseDownload`) with explicit arguments, not an arbitrary shell string.
`ReleaseDownload` is phase 3; v1 uses native providers only.

Package managers have different non-interactive controls. They are passed only
with explicit `--yes`, never by default:

| Provider | Non-interactive form |
|---|---|
| `pacman` | `--needed --noconfirm` |
| `apt` | `-y` |
| `dnf` | `-y` |
| `winget` | `--accept-package-agreements --accept-source-agreements --silent --disable-interactivity` |

`winget` is not enabled on the current Linux-only target; it is listed because
its controls show why provider-specific flags cannot be collapsed. In v1,
providers that require root are plan-only: `apply` prints the exact command and
exits `3`. The flags above describe a future explicit root execution mode, not
an automatic v1 path.

These flags suppress different things (package confirmation, license/source
agreements, installer UI), so Anvil must not model them as one generic
`noninteractive` boolean.

### 5. Keep language packs build-time only

Syntax packs remain compiled compositions:

- Rust syntax is always present;
- TypeScript/JavaScript and C/C++ syntax require their feature packs;
- the release pipeline must ship at least one artifact containing all
  documented packs;
- if release size requires a split, publish explicit `standard` and `full`
  artifacts;
- do not expose per-language runtime installation or independent syntax
  checkboxes.

`anvil setup` installs runtime tools only. It does not mutate the compiled
syntax surface.

### 6. Separate managed tools from plugin packages

Managed downloads and installation state use:

```text
$XDG_DATA_HOME/anvil/tools/
  state.json
  state.lock
  store/<sha256>/
```

Rules:

- `state.json` is schema-versioned and records tool id, provider, package or
  source URL, version, executable path, checksum when downloaded, and
  installed/confirmed timestamps;
- content-addressed `store/<sha256>/` is used only for managed downloads;
- system package installs are recorded as `provider=system` and do not claim
  Anvil-owned uninstall or file ownership;
- writes use a private stage, lock, fsync, and rename; failed installs leave no
  active state;
- the directory is `0700`; state and lock files are `0600`;
- XDG fallback is `$HOME/.local/share`;
- this is separate from the signed `.anvil-plugin` store; language servers are
  not plugins and the plugin trust model does not apply.

### 7. Do not modify shell PATH by default

Managed tool directories are searched by the LSP resolver and by the LSP child
process environment, not by editing `.zshrc`/`.bashrc`.

`anvil setup --print-path` may print a shell snippet. Writing a shell profile
requires a separate explicit operation and must be reversible. A successful
install output must state whether the executable is reachable from Anvil
without a PATH change.

### 8. Keep credential and privilege boundaries explicit

- system package installation is the only path that may require root;
- setup never invokes `sudo` silently or uses `sudo -S`/stdin passwords;
- non-interactive execution is limited to explicit `--yes`;
- managed downloads use pinned versions and SHA-256 checksums, never `latest`;
- package-manager installs rely on the package manager's signatures and
  repository policy;
- `npm`, `go`, `cargo`, and `pipx` providers are shown as code-executing
  installs and require confirmation;
- project-local manifests cannot select an executable (ADR-0038);
- no provider downloads or executes a script from the workspace.

### 9. Implement in bounded phases

1. **Detection and planning**: shared catalog, `anvil setup` check/plan, JSON
   output, no writes or network.
2. **Native providers**: rustup/npm/go/pipx and system package managers, fake
   provider tests, no managed downloader.
3. **Managed downloads**: pinned release checksums, content-addressed store,
   atomic state, rollback/removal of managed tools only.
4. **Release variants**: full artifact required; optional standard/full split;
   readiness tests must report compiled syntax availability accurately.

## Consequences

Positive:

- Missing tools become actionable without granting startup download authority.
- Explicit `[lsp.roots]`, `[lsp.commands]`, and environment paths are preserved.
- Per-tool provider order avoids installing stale distro packages when the
  ecosystem provides a better official path.
- Managed tools remain separate from signed plugins and user shell state.
- Scripts can use `--json`, stable exit codes, and non-interactive behavior.

Negative:

- Provider metadata and distro package names require maintenance.
- Package managers may prompt, require root, or install outside PATH.
- Managed downloads add checksum/update/rollback work and archive handling.
- Release full/standard variants add build and documentation maintenance.
- Readiness and setup must share a resolver or they will drift.
- The CLI contract adds tests for streams, TTY behavior, cancellation, and
  provider-specific non-interactive flags.

## Alternatives considered

- **Mason-style automatic installer at startup**: rejected. It adds a remote
  registry, download authority, and PATH mutation to the editor startup path.
- **Global system-package-manager-first order**: rejected. Distro packages are
  often older than official language tooling; clangd explicitly warns about
  this.
- **Per-language runtime syntax packs**: rejected. Syntax is compiled into the
  distribution; runtime installation would create a second distribution model.
- **Reuse the plugin package store**: rejected. Plugins use signed
  `.anvil-plugin` packages and a different trust boundary.
- **A separate `anvil-setup` binary first**: deferred. The subcommand avoids a
  second artifact; the installer shell remains the bootstrap path.
- **Read project-local tool manifests**: rejected. A repository must not be
  able to choose an executable or install source.
- **A full-screen TUI wizard**: rejected for v1. It cannot be scripted and
  duplicates the editor's UI layer; line-based output is the stable surface.
- **Automatic `sudo`**: rejected. Root operations are printed by default and
  require explicit user action.

## Implementation status (2026-11-18)

- Release layouts now build the full official composition with every documented
  language pack; plain local builds remain standard unless `full` is selected.
- `anvil setup` / `check` / `plan` and native `apply` are implemented using the
  shared settings-aware resolver, including the compiled LaTeX/Typst service;
- `apply` executes user-level native providers after confirmation and blocks
  root-requiring or manual providers before changing anything;
- successful native apply records tool provenance in
  `$XDG_DATA_HOME/anvil/tools/state.json` with a lock and atomic replacement;
- Managed downloads are deferred: no current provider lacks a native path, and
  adding an HTTP client is a separate trust/dependency decision. Native
  providers are the only apply path.
- Deferred setup flags: `--profile`, `--dry-run`, `--quiet`, `--verbose`,
  `--no-color`, and `--print-path`; usage documents only implemented flags.
- `:languages` and `anvil doctor` remain read-only.
- The `:languages` footer points at manual installation and
  `[lsp.commands]`/`[lsp.roots]`, and states that `doctor` only verifies
  `PATH`.

## Verification

- `anvil setup check` and `plan` make no network calls and write no files.
- `--json` emits parseable JSON on stdout with no human text; progress and
  errors are only on stderr.
- `--plain` emits one record per line.
- Non-TTY runs never prompt; `--no-input` rejects every interactive path.
- `--yes` does not imply `apply`; `apply --json --yes` is required for a fully
  non-interactive install.
- A fake provider test proves `apply` records state only after success and
  returns the documented exit code.
- Precedence tests cover root profile, environment, global command, managed
  directory, and PATH, including an invalid explicit path.
- `doctor` and `:languages` agree when a server is configured through settings
  or an environment override.
- A standard release reports missing TypeScript/JavaScript and C/C++ syntax;
  a full release reports them as available.
- Managed state writes are locked and atomic; stale state is rejected when the
  executable disappears.
- Ctrl-C exits `130` without leaving active state.
- No shell profile is modified without an explicit opt-in.
- Project settings cannot select an LSP command.

## References

- [Command Line Interface Guidelines](https://clig.dev/) — stdout/stderr,
  `--json`/`--plain`, TTY detection, `--no-input`, `--dry-run`, progress,
  confirmation, colors, and Ctrl-C behavior.
- [NO_COLOR](https://no-color.org/) — `NO_COLOR` convention and precedence.
- [rust-analyzer installation](https://rust-analyzer.github.io/book/installation.html)
  — rustup components, standard-library source, and editor setup.
- [rust-analyzer binary](https://rust-analyzer.github.io/book/rust_analyzer_binary.html)
  — standalone releases, rustup component, Arch package, and Homebrew.
- [rustup other installation methods](https://rust-lang.github.io/rustup/installation/other.html)
  — installer arguments, `--no-modify-path`, profiles, and SHA-256 files.
- [clangd installation](https://clangd.llvm.org/installation) — package-manager
  versions, standalone releases, and the recommendation for a recent clangd.
- [typescript-language-server README](https://unpkg.com/typescript-language-server@5.1.3/README.md)
  — npm installation and `--stdio` launch.
- [uv installer options](https://docs.astral.sh/uv/reference/installer/) —
  installation directory, shell-modification opt-out, and unmanaged installs.
- [winget install](https://learn.microsoft.com/en-us/windows/package-manager/winget/install)
  — separate agreement, silent, and interactivity flags.
- [Mason.nvim](https://github.com/mason-org/mason.nvim) — explicit package
  commands, data-directory installs, PATH insertion behavior, registries, and
  optional install-time firewall.
- [XDG Base Directory Specification](https://dueno.pages.freedesktop.org/xdg-specs/basedir-spec/latest/ar01s03.html)
  — `$XDG_DATA_HOME` default and user data directory semantics.
- [Plugin package store plan](../0.6.1-package-store-plan.md) — local
  content-addressed, locked, atomic state pattern.
- [Installing Anvil](../../INSTALL.md) — current release layout and language-server
  prerequisites.
- [README language settings](../../README.md) — `[lsp.commands]` and
  `[lsp.roots]` override examples.
- [ADR-0050](0050-discovery-first-language-packs.md) — compiled language packs
  and readiness surface.

# ADR-0052: Explicit Language Tool Setup and Source Resolution

- Status: Proposed
- Version: 1.0.0
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

Anvil reports language readiness but does not provide a setup path:

- `:languages` and `anvil doctor` are read-only;
- no `anvil setup` or `install-tools` command exists;
- `dist/anvil/install.sh` copies binaries and plugins but has no language-tool
  options;
- the release layout is built as `anvil-distro-standard` without `--features
  full`, so it does not compile the TypeScript/JavaScript web grammar or the
  C/C++ grammar by default;
- `language-pack-rust` is empty because Rust syntax is always compiled;
- `[lsp.commands]`, `[lsp.roots.<path>.<lang>] command`, and
  `ANVIL_LSP_<LANG>`/`ANVIL_LSP` already let users choose an explicit server
  path;
- managed plugins already use an XDG data directory, content-addressed storage,
  locks, and atomic state writes.

Language servers are external executables. Their installation sources differ
by ecosystem:

- rust-analyzer is distributed through rustup, distro packages, and standalone
  releases;
- clangd packages are often older than the official clangd release;
- typescript-language-server is distributed through npm and requires a
  compatible TypeScript package;
- gopls and Python servers are tied to their language toolchains.

A single global installation order is therefore incorrect. Silent installation
would add network, supply-chain, PATH, and privilege behavior to an editor whose
distribution and sandbox model intentionally avoid those defaults.

## Decision

### 1. Add `anvil setup` as an explicit subcommand

`anvil setup` is a non-startup subcommand, not an editor startup step:

```text
anvil setup                 # check and print a plan (default)
anvil setup check           # explicit check
anvil setup plan            # print provider commands and provenance
anvil setup apply           # execute confirmed installations
anvil setup --json          # schema-versioned machine-readable output
anvil setup --tool <id>     # restrict the operation
anvil setup --profile <p>   # restrict to a language/tool profile
```

Rules:

- `check` and `plan` perform no network access and write no files;
- `apply` requires the explicit flag;
- non-interactive package-manager flags (`--noconfirm`, `-y`, `--yes`) require
  a separate `--yes`;
- `apply` never runs automatically from `:languages`, `doctor`, or startup;
- `install.sh` may print the `anvil setup` hint, but does not silently install
  language tools;
- a separate `anvil-setup` binary is deferred; the subcommand reuses the
  existing distribution entrypoint and avoids a second catalog/version skew.

### 2. Resolve explicit user choices before discovery

The setup and readiness surfaces must preserve the existing LSP override order:

1. `[lsp.roots."<root>".<lang>] command` from user settings;
2. `ANVIL_LSP_<LANG>` or `ANVIL_LSP`;
3. `[lsp.commands]` from user settings;
4. Anvil-managed tool directory;
5. `PATH`.

The per-root profile is applied last in the current resolver and therefore has
the highest precedence. The environment override is checked before the global
`[lsp.commands]` table. Managed tools are inserted only after explicit user
choices and before generic PATH discovery.

If an explicit path is configured but missing or non-executable, setup reports
an error. It must not silently fall back to another server: the user selected a
specific executable.

`doctor` and `:languages` must use the same catalog and precedence, or clearly
label a PATH-only result. A server configured in `[lsp.roots]` must not be
reported as missing merely because it is outside `PATH`.

### 3. Use per-tool provider ordering

A tool manifest owns the ordered providers. The order is not global
"package manager first" policy.

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

### 4. Keep language packs build-time only

Syntax packs remain compiled compositions:

- Rust syntax is always present;
- TypeScript/JavaScript and C/C++ syntax require their feature packs;
- the release pipeline must ship at least one artifact containing all
  documented packs;
- if release size requires a split, publish explicit `standard` and `full`
  artifacts; do not expose per-language runtime installation or independent
  syntax checkboxes.

`anvil setup` installs runtime tools only. It does not mutate the compiled
syntax surface.

### 5. Separate managed tools from plugin packages

Managed downloads and installation state use:

```text
$XDG_DATA_HOME/anvil/tools/
  state.json
  state.lock
  store/<sha256>/
```

Rules:

- `state.json` is schema-versioned and records tool id, provider, package or
  source URL, version, executable path, checksum when downloaded,
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

### 6. Do not modify shell PATH by default

Managed tool directories are searched by the LSP resolver and by child process
launch, not by editing `.zshrc`/`.bashrc`.

An opt-in `anvil setup --print-path` may print a shell snippet. Writing a shell
profile requires a separate explicit operation and must be reversible. A
successful install output must state whether the executable is reachable from
Anvil without a PATH change.

### 7. Keep credential and privilege boundaries explicit

- System package installation is the only path that may require root;
- setup never invokes `sudo` silently or uses `sudo -S`/stdin passwords;
- non-interactive execution is limited to explicit `--yes`;
- managed downloads use pinned versions and SHA-256 checksums, never `latest`;
- package-manager installs rely on the package manager's signatures and
  repository policy;
- `npm`, `go`, `cargo`, and `pipx` providers are shown as code-executing
  installs and require confirmation;
- project-local manifests cannot select an executable (ADR-0038);
- no provider downloads or executes a script from the workspace.

### 8. Implement in bounded phases

1. **Detection and planning**: shared catalog, `anvil setup` default check,
   JSON output, no writes or network.
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
- Installation provenance can be audited and tested.

Negative:

- Provider metadata and distro package names require maintenance.
- Package managers may prompt, require root, or install outside PATH.
- Managed downloads add checksum/update/rollback work and archive handling.
- Release full/standard variants add build and documentation maintenance.
- Readiness and setup must share a resolver or they will drift.

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

## Implementation status (2026-11-18)

- Current release layout still builds the standard composition without the full
  language packs.
- No `anvil setup` command exists yet.
- `:languages` and `anvil doctor` remain read-only.
- The `:languages` footer was corrected to point at manual installation and
  `[lsp.commands]`/`[lsp.roots]`, and to state that `doctor` only verifies
  `PATH`.

## Verification

- `anvil setup check` and `plan` make no network calls and write no files.
- A fake provider test proves `apply` records state only after success.
- Precedence tests cover root profile, environment, global command, managed
  directory, and PATH, including an invalid explicit path.
- `doctor` and `:languages` agree when a server is configured through settings
  or an environment override.
- A standard release reports missing TypeScript/JavaScript and C/C++ syntax;
  a full release reports them as available.
- Managed state writes are locked and atomic; stale state is rejected when the
  executable disappears.
- No shell profile is modified without an explicit opt-in.
- Project settings cannot select an LSP command.

## References

- [rust-analyzer installation](https://rust-analyzer.github.io/book/installation.html)
  — rustup components, standard-library source, and editor setup.
- [rust-analyzer binary](https://rust-analyzer.github.io/book/rust_analyzer_binary.html)
  — standalone releases, rustup component, Arch package, and Homebrew.
- [clangd installation](https://clangd.llvm.org/installation) — package-manager
  versions, standalone releases, and the recommendation for a recent clangd.
- [typescript-language-server README](https://unpkg.com/typescript-language-server@5.1.3/README.md)
  — npm installation and `--stdio` launch.
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

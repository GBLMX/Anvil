# ADR-0048: Per-root language tooling and LSP workspace configuration

- Status: Proposed
- Version: 1.1.0
- Implementation: implemented 2026-11-16
- Date: 2026-11-16
- Supersedes: ADR-0025 §5 "LSP is primary-root-only for now" clause only
- Superseded by:
- Related: [ADR-0018](0018-workspace-tree-watch.md),
  [ADR-0025](0025-lsp-and-language-breadth.md),
  [ADR-0032](0032-language-tooling-service.md),
  [ADR-0038](0038-project-settings-trust.md),
  [ADR-0045](0045-settings-patch-layers.md)

## Context

Anvil already supports multiple trusted workspace roots with per-root gateways.
LSP support has not followed that model. ADR-0025 deliberately kept LSP on the
primary root; this ADR removes that limitation and defines the replacement
boundary.

### Current behavior

- `LspState.workers` is keyed by `(tool, language_id)`. The key has no root, so
  two roots using the same server share one worker.
- `LspState.config` holds one active `(tool, language_id)`. Only one server is
  active in the UI at a time.
- `start_lsp` obtains its connection from the primary
  `workspace.gateway_session`, not the buffer's owning root gateway.
- `LspWorker::start` receives the primary workspace path and searches root
  markers only within that path.
- The gateway's `lsp.start` starts the process with
  `Dir: s.cfg.Workspace`; a nested project root is not the process working
  directory.
- `lsp.start` has no environment parameter. The gateway supplies a fixed
  Rust/cargo-oriented environment, so a per-root toolchain cannot be selected.
- `workspace/workspaceFolders` is advertised to the server, but the client
  answers `workspace/workspaceFolders` with an empty array.
- `workspace/configuration` answers every requested item with `{}`. A server
  therefore cannot read per-root settings.
- `workspace/didChangeConfiguration` is never sent; `:config-reload` does not
  propagate server settings.
- User command overrides are global by language id (`[lsp.commands]`).
  Project settings deliberately reject `[lsp.commands]`.
- `:lsp-status` prints `(primary)` and does not identify the owning root.

### Product need

A multi-root workspace can contain a Rust 2021 project, a Rust nightly
project, a Go module, and a C++ compilation database. Each root may require:

- a different server executable or wrapper;
- different environment variables and `PATH`;
- a different project root selected by root markers;
- different `initializationOptions`;
- different `workspace/configuration` values.

The implementation must not merge those states or silently route a request to
the primary root.

### External evidence

The LSP specification defines `rootUri`, `workspaceFolders`,
`workspace/configuration`, `workspace/workspaceFolders`, and
`workspace/didChangeConfiguration` as separate workspace-scoped concepts. A
server with one workspace folder is valid; a client may also provide multiple
folders when one server can serve them.

Editors that support multi-root projects generally make settings
folder-relative. VS Code applies resource-scoped folder settings; Helix
supports root marker selection and per-server `config`/`environment`; rust-analyzer
accepts project discovery and workspace-scoped configuration; gopls documents
per-workspace-folder settings; clangd resolves project configuration and
compilation databases relative to the source tree.

The design below uses those mechanisms but keeps Anvil's existing trust and
gateway boundaries.

## Decision

### 1. Key LSP workers by owning root

Introduce a root-scoped identity:

```text
LspKey {
    root: PathBuf,       // canonical owning WorkspaceRoot
    tool: String,
    language_id: String,
}
```

`root` is the owning canonical workspace root, not the nested project root.
The nested project root is part of the worker's launch configuration and is
reported separately.

Workers, retries, status, labels, inlay-hint capability, and any
server-specific UI cache must be keyed by `LspKey`. Do not key workers only by
`(tool, language_id)`.

The active key is a UI convenience for the current buffer. Request routing must
resolve the worker from the file's owning root, not from the single active key.
No request may fall back to the primary root.

### 2. One fresh LSP connection per worker

Each root-scoped worker opens its own fresh LSP connection through the owning
root gateway:

```text
root_gateways[root].session.connect_lsp()
```

Each gateway connection continues to own exactly one LSP manager. Do not add a
second LSP manager or a server-id multiplexer to one connection in this ADR.
This preserves the existing `lsp.start` / `lsp.send` / `lsp.stop` notification
shape and avoids cross-server notification ambiguity.

The worker thread owns its `GatewayClient` for its lifetime. Remove the shared
`LspState.client`; it cannot safely serve multiple root-scoped workers.

Retire the pre-opened LSP client returned by `GatewaySession::start`. Update
the primary startup path, root-gateway creation, `reconnect_gateway`,
`Model::drop`, and LSP test helpers so none of them store a session-wide
`lsp_client`. Each worker calls `connect_lsp()` and owns the resulting
connection. Leaving an unused pre-opened LSP connection would violate the
one-connection-per-worker model.

If the gateway connection cannot be created, the worker enters the existing
failed state for that root and reports the owning root. It must not silently
use the primary gateway.

### 3. Separate owning root from project root

For a file under root `R`:

- `owning_root = R`;
- `project_root` is the highest directory between the file and `R` that
  contains a configured root marker;
- if no marker matches, `project_root = R`.

`LspKey.root` is `owning_root`. The worker's launch root and LSP workspace
folder are `project_root`.

Root-marker search is bounded by `owning_root`. A marker or path outside that
root is never considered. The existing topmost-marker rule is preserved.

The gateway's `lsp.start` gains a `root` field. The process working directory is
`root`, not the gateway's workspace root. The gateway validates that `root` is
an existing directory beneath its configured workspace using the existing
beneath-workspace path resolver. Symlink or `..` escapes are rejected.

The LSP initialization handshake uses:

- `rootUri = file_uri(project_root)`;
- `workspaceFolders = [{ uri: file_uri(project_root), name: root_name }]`;
- `rootPath = null` and is not relied on;
- `initializationOptions` from the selected profile, or the catalog default;
- client capability `workspace.workspaceFolders = true`;
- client capability `workspace.configuration = true`;
- client capability `workspace.didChangeConfiguration`.

`root_name` is the workspace root label when available, otherwise the project
root's final path component. One worker serves one project root; a future ADR
may add a server that intentionally spans multiple roots.

`workspace/workspaceFolders` returns that one folder, never an empty array.
`workspace/didChangeWorkspaceFolders` is not sent because this ADR does not run
one server across multiple roots. Adding or removing a root starts or stops its
workers.

### 4. Resolve user-level per-root launch profiles

Keep the built-in language-server catalog as the lowest-precedence source.
Extend user settings with root profiles keyed by canonical workspace path and
language id:

```toml
[lsp]
inlay_hints = true

[lsp.commands]
# Existing global fallback, keyed by language id.
rust = "/usr/bin/rust-analyzer"

[lsp.settings]
# User-wide server defaults, keyed by configuration section.
"rust-analyzer" = { cargo = { features = "all" } }

[lsp.roots."/home/me/work/stable"]
rust = {
    command = ["rustup", "run", "1.85.0", "rust-analyzer"],
    env = { RUSTUP_TOOLCHAIN = "1.85.0" },
    root_markers = ["Cargo.toml"],
    initialization = { cargo = { features = "all" } },
    settings = { "rust-analyzer" = { cargo = { features = "all" } } },
}

[lsp.roots."/home/me/work/nightly"]
rust = {
    command = ["rustup", "run", "nightly", "rust-analyzer"],
    env = { RUSTUP_TOOLCHAIN = "nightly" },
}
```

Supported profile fields:

- `command`: a string executable or argv array. It overrides the global
  `[lsp.commands]` entry and catalog executable.
- `env`: string-to-string environment overlay. Values are literal; no shell
  expansion or shell execution.
- `root_markers`: optional root-marker override. Empty or omitted uses the
  catalog markers.
- `initialization`: optional `initializationOptions` value. It replaces the
  catalog initialization options for this root.
- `settings`: optional section map used by `workspace/configuration`.

Resolution order:

1. built-in catalog and existing `ANVIL_LSP_*` executable fallback;
2. user `[lsp.commands]` and `[lsp.settings]` defaults;
3. user root profile for the exact canonical root and language id.

A root profile without `command` still contributes `env`, `initialization`,
`settings`, and `root_markers`. A root profile with no matching key falls back
to the global user settings and catalog.

`settings` merges recursively: object keys merge, arrays and scalars replace.
`env` overlays the gateway's base environment. `initialization` is not merged;
it is an owned profile value.

### 5. Treat toolchain selection as an explicit launch profile

This ADR does not add automatic toolchain detection.

- Do not parse `rust-toolchain.toml`, `.tool-versions`, `.python-version`, or
  similar files in Anvil.
- Do not infer a compiler version from `Cargo.toml`, `go.mod`, or a Python
  virtualenv.
- A user selects a toolchain by choosing a wrapper command and/or environment,
  for example `rustup run`, `mise exec`, `asdf exec`, or a project-specific
  server binary.
- Native project files and toolchain managers remain owned by the language
  server and the user's toolchain. Anvil only supplies the launch command,
  environment, project root, and LSP settings.

This keeps the implementation small and avoids a second toolchain manager.

### 6. Make LSP workspace configuration live and root-scoped

Each worker carries an immutable launch profile plus a replaceable server
settings snapshot.

- During `initialize`, answer server requests using the initial settings
  snapshot.
- `workspace/configuration` resolves each item's `section` against the
  worker's `settings`:
  - exact section match first;
  - then dotted-path lookup for nested sections;
  - missing values return `null`, not `{}`;
  - a `scopeUri` outside the worker's project root returns `null`.
- If an item has no `section`, return the full settings object, or `null` when
  no settings exist.
- `workspace/workspaceFolders` returns the worker's one project-root folder.
- On `:config-reload`, send `workspace/didChangeConfiguration` with the new
  full settings object to every affected worker.
- Changes to `command`, `args`, `env`, `root_markers`, `initialization`, or the
  resolved project root require a worker restart, because they cannot be
  applied through `workspace/didChangeConfiguration`.
- Settings-only changes do not restart the server.
- A settings update received during initialization is coalesced into the latest
  snapshot and sent after the `initialized` notification; it does not restart
  the worker.

Do not mutate a worker's initialization options in place. A restart is the
only way to apply them.

### 7. Route all LSP operations through the owning worker

For the current or any open document:

- sync, save, close, completion, hover, definition, references, symbols,
  diagnostics, formatting, rename, code actions, code lens, folding, semantic
  tokens, inlay hints, and call hierarchy use `worker_for_path(path)`.
- If the owning worker is missing, start it for that root. Do not use another
  root's worker while it starts.
- Diagnostics are stored by absolute path, but the plugin event's root is the
  owning root.
- Inlay-hint capability, semantic-token legends, status, retry counters, and
  labels are per `LspKey`.
- `:lsp-status`, `:lsp-start`, and `:lsp-stop` operate on the current buffer's
  owning root. Other roots are unaffected.
- Closing the last open document for a root stops its worker after pending
  requests are discarded.
- Root removal, trust revocation, and gateway loss stop all of that root's
  workers.
- Gateway reconnect is root-scoped. Reconnecting or losing the primary gateway
  must not clear or stop workers owned by secondary roots; only workers whose
  own connection failed are discarded.

The LSP request surface and `WorkspaceEdit` rules from ADR-0025 remain
unchanged.

### 8. Preserve the security boundary

Only user settings may define launch commands, environment, initialization
options, and server settings.

Project `.anvil.toml` must continue to reject `[lsp.commands]`. It must also
reject the new executable or server-influencing fields:

```text
[lsp.commands]
[lsp.roots]
[lsp.settings]
```

This is required because server settings can contain command overrides such as
rust-analyzer build-script commands. A trusted project file is still not an
acceptable place to inject an executable or environment override.

Native project configuration files read directly by the language server
(`.clangd`, `rust-analyzer.toml`, compilation databases) remain the server's
own trust boundary. Anvil does not parse them in this ADR.

Root profile keys are canonical UTF-8 absolute paths. A path that is not
canonical, relative, or not representable in TOML does not match; the root falls
back to global defaults and reports that fallback in `:lsp-status`.

### 9. Bound process fan-out

Use configurable but hard-capped limits:

```text
MAX_LSP_WORKERS = 8
MAX_LSP_WORKERS_PER_ROOT = 4
MAX_LSP_ENV_VARS = 64
MAX_LSP_ENV_KEY_BYTES = 128
MAX_LSP_ENV_VALUE_BYTES = 4096
```

Workers are retained only while their root has an open document. Starting a
worker beyond the total cap evicts the least-recently-used non-active worker;
if there is no evictable worker, the new worker is not started and the status
reports the limit. Exceeding the per-root cap fails that start rather than
starving other roots.

The existing settings-file size limit continues to bound profile data. Do not
add unbounded environment or settings payloads to the LSP protocol.

### 10. Keep the migration compatible

- Existing `[lsp.commands]` continues to work as a global user fallback.
- Workspaces without root profiles continue to use the built-in catalog.
- The primary-root behavior changes only where it was wrong: requests now route
  by owning root and initialization uses the correct project root.
- `workspace/configuration` returns `null` for missing items instead of `{}`;
  this is a protocol-correct improvement for servers that distinguish the two.
- ADR-0025's primary-root-only clause is superseded only for LSP root routing.
  Its request surface, language breadth, large-file threshold, and trust
  boundary remain accepted.

## Consequences

Positive:

- Each root can use its own server binary, toolchain environment, project
  root, initialization options, and server settings.
- Requests and diagnostics cannot silently cross root boundaries.
- LSP workspace-folder and configuration requests become truthful.
- The existing per-root gateway and trust model is reused.
- One connection per worker avoids a complex multiplexed LSP protocol.

Negative:

- LSP state keys and UI caches become root-qualified.
- Multiple roots can start multiple language-server processes; memory and CPU
  use increase.
- Root profiles are explicit user configuration, not automatic project
  detection.
- Project files cannot inject LSP commands or settings; users must maintain
  root profiles themselves.
- A server that is intentionally shared across roots needs a future ADR.

## Alternatives considered

### One server with multiple workspace folders for all roots

Rejected for this decision. It cannot vary the executable, environment, or
toolchain per root, and one crash or indexing load affects every root. The LSP
protocol supports it, but it is the wrong default for a multi-toolchain
workspace.

### One multiplexed LSP manager per gateway connection

Rejected. It requires server ids in every notification and changes
`lsp.start`/`lsp.send`/`lsp.stop`. One fresh connection per worker already
provides isolation with the existing protocol.

### Keep the primary-root-only boundary

Rejected. It makes multi-root workspaces misleading and routes work to the
wrong server.

### Project `.anvil.toml` defines LSP commands, environment, or settings

Rejected. Those values can execute commands or alter server behavior. Project
settings remain non-executable; only user settings may define LSP launch
profiles.

### Automatically detect toolchains from project files

Rejected. It duplicates rustup, mise, asdf, and language-specific tooling, and
would need separate correctness and failure models.

### Run one server per language globally and share it across roots

Rejected. The server's `rootUri`, workspace folders, environment, and settings
would be wrong for at least one root.

## Verification

Focused configuration tests:

- a root profile overrides the global command, environment, initialization,
  root markers, and settings;
- a profile without a command still applies environment and settings;
- exact canonical root matching; relative, symlinked, and non-UTF-8 paths fall
  back with a reported reason;
- global `[lsp.settings]` merges with root settings; arrays replace;
- project `.anvil.toml` rejects `[lsp.commands]`, `[lsp.roots]`, and
  `[lsp.settings]`;
- environment key/value and settings size bounds are enforced.

Worker and protocol tests:

- two roots with the same language start two workers and two LSP connections;
- each initialize contains the correct `rootUri`, one `workspaceFolders` entry,
  and the correct `initializationOptions`;
- `workspace/workspaceFolders` returns one folder, not an empty array;
- `workspace/configuration` resolves exact sections, dotted sections, missing
  sections as `null`, and out-of-root `scopeUri` as `null`;
- `workspace/didChangeConfiguration` is sent on settings reload but not on a
  settings-only change that did not actually change values;
- a launch-profile change restarts only the affected worker;
- `workspace/didChangeWorkspaceFolders` is not sent for per-root workers.

Routing and lifecycle tests:

- no session-wide pre-opened `lsp_client` remains in startup, reconnect, drop,
  or test helpers;
- a primary gateway reconnect leaves secondary-root workers running;
- a settings update during initialization is applied after `initialized`;
- definition, completion, hover, references, diagnostics, format, rename, and
  code lens requests from root B use root B's worker;
- no request falls back to the primary root when root B has no worker;
- same-basename files in two roots keep separate diagnostics and status;
- `:lsp-status` and `:lsp-stop` identify the owning root;
- closing the last document for one root does not stop another root's worker;
- root removal and trust revocation stop only that root's workers;
- worker-cap eviction is deterministic and reports the limit;
- a fake root gateway proves each worker opens a fresh LSP connection.

Real-surface smoke:

- open two roots with different toolchains and the same language;
- verify separate `:lsp-status` roots and server PIDs;
- request diagnostics, completion, and go-to-definition from each root;
- edit a settings-only value and verify `workspace/didChangeConfiguration`;
- change a command/environment profile and verify one worker restarts;
- remove one root and confirm the other server stays alive;
- run the existing `scripts/lsp-real-smoke.sh` in single-root mode to prove no
  regression.

## References

- Language Server Protocol 3.17 specification:
  https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/
  - `InitializeParams.rootUri` and `workspaceFolders`, the
    `workspace/workspaceFolders` request, `workspace/configuration`, and
    `workspace/didChangeConfiguration` define the workspace-scoped protocol
    behavior used here.
- VS Code multi-root workspaces:
  https://code.visualstudio.com/docs/editing/workspaces/multi-root-workspaces.md
  - Folder-scoped settings and the rule that only resource-scoped settings apply
    from each root. Supports the decision to root-scope server settings.
- Helix languages configuration:
  https://docs.helix-editor.com/master/languages.html
  - `roots` and `workspace-lsp-roots` for project-root selection, and
    per-language-server `config`/`environment`. Supports the per-root profile
    and project-root split.
- Helix language-server documentation:
  https://docs.helix-editor.com/master/lsp.html
  - Explains server configuration and restart behavior in a multi-language
    client. Supports the decision to treat launch changes as restart-only.
- rust-analyzer configuration:
  https://rust-analyzer.github.io/book/configuration.html
  - `initializationOptions`, `linkedProjects`, workspace-scoped cargo settings,
    and the work-in-progress `rust-analyzer.toml` behavior. Supports
    initialization and settings separation.
- gopls settings:
  https://go.dev/gopls/settings
  - Documents that clients may configure settings per workspace folder and
    exposes workspace-scoped `env`, `directoryFilters`, and related settings.
- clangd configuration:
  https://clangd.llvm.org/config
  - Project `.clangd` files, compilation database lookup, and scoped
    configuration fragments. Supports leaving native project configuration to
    the server.

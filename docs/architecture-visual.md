# Anvil Architecture Maps

Status: current as of 2026-11-24. Solid paths are implemented, including
asynchronous and out-of-process calls; dashed paths are deferred marketplace
work. `0.7.0` internal polish is complete, and `0.8.0` is the current workspace
release candidate. The `0.7.1`–`0.7.4` official modules are landed, and
marketplace/signed-index surfaces remain deferred. Layer rules and ownership stay in
[architecture.md](architecture.md); this file is the diagram view.

## 1. Process and trust boundaries

```mermaid
flowchart TB
    subgraph FRONTEND["anvil-tui + anvil-session · trusted"]
        UI["input · model · view<br/>buffers · overlays · roots"]
        SESSION["anvil-session<br/>documents · commands · effects"]
        TREE["file tree UI"]
        LSPUI["LSP UI"]
        GITUI["Git UI"]
        PLUGINS["plugin supervisor<br/>activation · backoff · drain"]
        CONTROL["anvil-control<br/>control socket v3"]
        UI --> SESSION
        UI --> TREE
        UI --> LSPUI
        UI --> GITUI
        UI --> PLUGINS
        UI --> CONTROL
    end

    subgraph WORKERS["background workers"]
        FW["FileWorker<br/>one per trusted root"]
        LW["LspWorker<br/>one per root + language"]
    end

    TREE --> FW
    LSPUI --> LW

    IPC["anvil-ipc<br/>GatewayClient"]
    FW --> IPC
    LW --> IPC

    subgraph GATEWAY["anvil-gateway · Go · per session · trusted"]
        ROUTER["JSON-RPC router"]
        POLICY["capability policy"]
        FS["fsroot<br/>openat2 · RESOLVE_BENEATH"]
        EXEC["execpolicy"]
        FETCH["fetch<br/>SSRF constrained"]
        LSPMGR["internal/lsp"]
        WATCH["internal/fswatch<br/>inotify · cooldown"]
        PKG["internal/pkg + pluginindex"]
        ROUTER --> POLICY
        ROUTER --> FS
        ROUTER --> EXEC
        ROUTER --> FETCH
        ROUTER --> LSPMGR
        ROUTER --> WATCH
        ROUTER --> PKG
    end

    IPC -- gateway UDS --> ROUTER
    EXEC --> RUNNER["anvil-exec-runner<br/>Landlock · seccomp · cgroup"]
    RUNNER --> TOOLS["rg · fd"]
    LSPMGR --> LS["LSP processes<br/>trusted · isolated from plugin host"]
    WATCH -- "watch.events / watch.status" --> ROUTER

    subgraph PLUGIN["anvil-plugin-host · untrusted"]
        HOST["Luau runtime<br/>Landlock · seccomp · cgroup"]
    end

    PLUGINS -- "editor.sock" --> HOST
    HOST -- "sys.* + token" --> ROUTER

    CTL["anvil-ctl · anvil-mcp<br/>same-UID local adapters"]
    CONTROL <--> CTL
```

### Trust boundaries

| Component | Trust | Responsibility |
|---|---|---|
| `anvil-tui` | trusted | input, view, overlays, workspace roots, plugin activation |
| `anvil-session` | trusted | documents, command registry, effects, frontend-neutral state |
| `anvil-control` | trusted | workspace-keyed control socket and command manifest |
| `anvil-gateway` | trusted | per-session policy, file I/O, LSP, fetch, exec, watchers |
| `anvil-exec-runner` | trusted | sandboxed `rg` / `fd` launch |
| LSP process | trusted | language server; separate from the plugin sandbox |
| `anvil-plugin-host` | untrusted | Luau entry; Landlock/seccomp/cgroup; capability token only |
| Official modules | trusted | compiled-in source-level services |
| `anvil-ctl` / `anvil-mcp` | same-UID local adapter | routes through `anvil-control`; never writes workspace files directly |

## 2. File, LSP, and watch flow

```mermaid
flowchart LR
    TREE["tree UI"] --> FW["FileWorker<br/>per trusted root"]
    LSPUI["LSP UI"] --> LW["LspWorker<br/>per root + language"]
    FW --> IPC["GatewayClient"]
    LW --> IPC
    IPC --> ROUTER["gateway router"]

    ROUTER --> FS["fsroot"]
    ROUTER --> LSP["internal/lsp"]
    ROUTER --> WATCH["internal/fswatch"]

    WATCH -- "watch.events<br/>watch.status" --> ROUTER
    ROUTER -- notifications --> IPC
    IPC -- events --> FW
    IPC -- responses --> LW
    FW --> TREE
    LW --> LSPUI

    FS --> FILES["workspace files"]
    LSP --> SERVERS["rust-analyzer · gopls · pyright<br/>typescript-language-server · ..."]
```

Each trusted root owns one `FileWorker`; each root/language pair owns one
`LspWorker`. Native watch status is reported through the same gateway
connection, while polling remains the fallback for unsupported filesystems,
skipped directories, and failed or cooling-down native registration.

## 3. Control and plugin planes

```mermaid
flowchart LR
    CTL["anvil-ctl"] --> CONTROL["anvil-control"]
    MCP["anvil-mcp<br/>stdio MCP"] --> CONTROL
    CONTROL --> ACTOR["TUI actor / command queue"]
    ACTOR --> STATE["Model + session state"]

    EDITOR["editor actor"] -- "editor.sock" --> HOST["plugin host"]
    HOST -- "sys.* + token" --> GATEWAY["gateway router"]
    HOST -- "editor/ui/event.*" --> EDITOR
    GATEWAY --> POLICY["capability policy"]
```

`anvil-control` is editor-owned and keyed by canonical workspace. It is not a
plugin grant channel. Plugins cross two different boundaries: `editor.*` calls
go to the editor, while `sys.*` calls go to the gateway with a per-plugin token.

## 4. Distribution and module composition

The official-only path is landed: release layout, installer, bundled fallback,
and managed store.

```mermaid
flowchart LR
    SRC["plugins/official"] --> LAYOUT["release-layout.sh"]
    LAYOUT --> PACK["official .anvil-plugin + .sig"]
    LAYOUT --> ART["tarball<br/>install.sh · SHA256SUMS.sig · SBOM · attestation"]
    ART --> INSTALL["install.sh"]
    INSTALL --> BIN["bin/"]
    INSTALL --> BUNDLED["share/anvil/official/plugins"]
    INSTALL --> STORE["share/anvil/packages"]
    BUNDLED --> DISCOVERY["plugin discovery"]
    STORE --> BOOT["anvil-gateway -install-bundled"]
    BOOT --> MANAGED["managed content-addressed store"]
    MANAGED --> DISCOVERY
```

Managed packages override bundled sources with the same plugin ID. The installer
is idempotent and is rerun on upgrade.

The marketplace path is deferred until the signed index, consent, revocation,
and production signing gates close. Package bytes never travel through
`sys.fetch`.

```mermaid
flowchart LR
    UI["marketplace / plugin manager<br/>deferred"] -.-> SERVICE["install service"]
    SERVICE -.-> FETCH["sys.fetch<br/>index + revocations"]
    SERVICE -.-> DOWNLOAD["pkg.download<br/>bounded queue"]
    FETCH -.-> VERIFY["verify index + package signature"]
    DOWNLOAD -.-> VERIFY
    VERIFY -.-> STORE["content-addressed store"]
    STORE -.-> DISCOVERY["discovery + manager"]
```

## 5. Delivery status

| Area | Status |
|---|---|
| `0.4.x` plugin lifecycle, I/O, session/protocol | landed |
| `0.5.x` modal editing and TUI interaction | landed |
| `0.6.0` internal polish | complete |
| `0.8.0` release candidate | prepared; ADR-0077 API break; production signing/provenance external |
| `0.6.1` package store and installed state | landed |
| `0.7.1`–`0.7.4` official modules | landed |
| Native workspace watch + live status | landed |
| Marketplace, signed-index UI, install/update consent | deferred |
| Release signing/provenance with production keys | external gate |
| Reference-machine performance baseline | recorded; rerun at boundaries |

Exit gates live in [internal-polish-plan.md](internal-polish-plan.md).
Architecture decisions and status live in the [ADR index](adr/README.md).

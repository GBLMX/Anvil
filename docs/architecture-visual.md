# Anvil Architecture Maps

Status: current as of `v0.2.9`. Solid paths are implemented. Dashed paths are
planned or debug-only.

## Current architecture

```mermaid
flowchart TB
    subgraph TUI["anvil-tui · Rust · trusted"]
        MAIN["main / run loop"]
        KEYS["keys.rs<br/>input dispatch"]
        MODEL["Model<br/>buffers · cursor · viewport<br/>pending state · command state"]
        RENDER["render.rs<br/>editor · gutter · popups"]
        SYNTAX["syntax.rs worker<br/>Tree-sitter"]
        TREEUI["file_tree_ui.rs<br/>tree · fuzzy state"]
        LSPUI["lsp_ui.rs<br/>requests · completion · hover · definition"]
        HOST["host_session<br/>debug plugin launcher"]
        IPC["anvil-ipc<br/>GatewayClient"]

        MAIN --> KEYS --> MODEL
        MODEL --> RENDER
        MODEL --> SYNTAX
        MODEL --> TREEUI
        MODEL --> LSPUI
        MODEL --> IPC
    end

    subgraph WORKERS["background workers"]
        FW["FileWorker<br/>own GatewayClient"]
        LW["LspWorker<br/>own GatewayClient"]
        OPEN["future async open/save<br/>not implemented"]
    end

    TREEUI --> FW
    LSPUI --> LW
    MODEL -.-> OPEN

    subgraph GATEWAY["anvil-gateway · Go · per session"]
        ROUTER["JSON-RPC router"]
        POLICY["capability policy"]
        FS["fsroot<br/>openat2 · RESOLVE_BENEATH"]
        EXEC["execpolicy"]
        FETCH["fetch<br/>SSRF constrained"]
        LSPMGR["internal/lsp<br/>process manager"]
    end

    IPC -- main UDS --> ROUTER
    FW -- worker UDS --> ROUTER
    LW -- worker UDS --> ROUTER

    ROUTER --> POLICY
    ROUTER --> FS
    ROUTER --> EXEC
    ROUTER --> FETCH
    ROUTER --> LSPMGR

    EXEC --> RUNNER["anvil-exec-runner<br/>Landlock · seccomp · cgroup"]
    RUNNER --> RG["rg / fd"]

    LSPMGR --> LS["rust-analyzer · gopls · pyright<br/>trusted · no sandbox"]

    subgraph PLUGIN["anvil-plugin-host · untrusted · current empty/debug"]
        PH["process group<br/>Landlock · seccomp · cgroup"]
    end

    HOST -. token FD + debug --> PH
    PH -. sys.* UDS + token --> ROUTER

    subgraph CORE["Rust libraries"]
        EDITOR["anvil-editor"]
        CORE_TXT["anvil-core<br/>PieceTable · LineIndex"]
        API["anvil-api<br/>contracts"]
    end

    MODEL --> EDITOR
    MODEL --> CORE_TXT
    EDITOR --> API
```

### Current trust boundaries

| Component | Trust | Responsibility |
|---|---|---|
| `anvil-tui` | trusted | buffer, input, UI, command state |
| `anvil-gateway` | trusted | per-session policy and I/O |
| `anvil-exec-runner` | trusted | sandboxed `rg` / `fd` launch |
| LSP process | trusted | `-no-sandbox-lsp`, separate from plugin-host |
| `anvil-plugin-host` | untrusted | empty sandbox/debug host; no Lua/QuickJS runtime yet |

## Future architecture

Dashed components are planned, not implemented.

```mermaid
flowchart TB
    subgraph TUI["anvil-tui · future"]
        KEYS2["keys.rs"]
        RENDER2["render.rs"]
        MODEL2["model.rs<br/>buffers · state · lifecycle"]
        FILEIO["file_io.rs<br/>async open/save"]
        CMD["commands.rs<br/>Ex · search · substitute"]
        LSPUI2["lsp_ui.rs<br/>completion · hover · definition"]
        WEDIT["workspace_edit.rs<br/>TextEdit / WorkspaceEdit"]
        ACTIONS["Code Actions / Quick Fix"]
        RENAME["Rename"]
        FORMAT["Formatting"]
        SYMBOLS["Document Symbols"]
        TREE2["file_tree_ui.rs"]

        KEYS2 --> MODEL2
        MODEL2 --> RENDER2
        MODEL2 --> FILEIO
        MODEL2 --> CMD
        MODEL2 --> LSPUI2
        MODEL2 --> WEDIT
        WEDIT --> ACTIONS
        WEDIT --> RENAME
        WEDIT --> FORMAT
        LSPUI2 --> SYMBOLS
        MODEL2 --> TREE2
    end

    subgraph ASYNC["async workers"]
        OW["open/save worker<br/>generation cancellation"]
        FW2["FileWorker"]
        LW2["LspWorker"]
    end

    FILEIO --> OW
    TREE2 --> FW2
    LSPUI2 --> LW2

    subgraph LSP["LSP layer"]
        LM["language manager<br/>Rust · Go · Python"]
        LS2["rust-analyzer · gopls · pyright"]
        LM --> LS2
    end

    LW2 --> LM

    subgraph GATEWAY2["anvil-gateway · future"]
        ROUTER2["JSON-RPC router"]
        POLICY2["capability policy"]
        FS2["fsroot · openat2"]
        EXEC2["execpolicy"]
        FETCH2["fetch"]
        CONTROL["grant / control socket"]
        LSPMGR2["internal/lsp"]
    end

    OW --> ROUTER2
    FW2 --> ROUTER2
    LW2 --> ROUTER2

    ROUTER2 --> POLICY2
    ROUTER2 --> FS2
    ROUTER2 --> EXEC2
    ROUTER2 --> FETCH2
    ROUTER2 --> LSPMGR2
    CONTROL --> POLICY2
    LSPMGR2 --> LM

    EXEC2 --> RUNNER2["anvil-exec-runner"]
    RUNNER2 --> TOOLS["rg · fd"]

    subgraph PH2["anvil-plugin-host · future"]
        VM["Lua / QuickJS runtime"]
        SANDBOX["Landlock · seccomp · cgroup"]
        VM --> SANDBOX
    end

    CONTROL -. grants + token --> PH2
    PH2 -. sys.* --> ROUTER2

    subgraph CORE2["Rust libraries"]
        EDITOR2["anvil-editor"]
        CORE2_TXT["anvil-core"]
        API2["anvil-api"]
    end

    MODEL2 --> EDITOR2
    MODEL2 --> CORE2_TXT
    EDITOR2 --> API2
```

## Planned sequence

1. **WorkspaceEdit engine** — shared UTF-16 edit application for Code Actions,
   Rename, and Formatting.
2. **LSP second tier** — Code Actions, Rename, Formatting, Document Symbols.
3. **Async open/save** — remove remaining synchronous file I/O from the UI path.
4. **Model extraction** — `model.rs`, `file_io.rs`, `commands.rs`; current
   `lib.rs` still owns the Model core.
5. **Plugin runtime** — Lua/QuickJS host, control-socket grants, VM isolation.

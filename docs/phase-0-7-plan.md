# 0.3.0 → 0.4.0 / Phase 0.7 Plan (Polish & Specification)

Status: in progress inside the [Polish & Specification release train](polish-and-specification.md).
`v0.3.0` has trusted LSP diagnostics, completion, hover, definition, rename,
Code Actions, formatting, and document symbols for Rust/Go/Python, an async
tree/fuzzy worker, and a split TUI (`keys`, `render`, `lsp_ui`,
`file_tree_ui`). Plugin-host remains an empty sandboxed process (no VM); syntax
highlighting is asynchronous but not incremental.

## Priorities

1. **Plugin-host isolation** — the remaining Phase 0.6 security boundary and a
   prerequisite for a plugin ecosystem.
2. **LSP contract closure** — decide or implement command-style Code Actions,
   close formatting options, and keep the advertised surface test-backed.
3. **Specification and documentation hardening** — keys, commands, schemas,
   error codes, trust boundaries, and known limitations.
4. **Reliability and performance evidence** — crash/reconnect paths,
   regression measurements, and release gates.

## 1. Plugin-host isolation

Contract: [ADR-0012](adr/0012-plugin-host-isolation.md).
Empty-host slice implemented: token FD, fd closure, Landlock FS read + no-exec rules, seccomp native-ABI + AF_UNIX-only sockets (TCP/UDP denied), `session.hello`, host-side UDS EOF exit, and isolation tests. VM pending; cgroup attach, OOM classification, framed host dispatch, and the TUI debug HostSession are implemented.

Required outcome:

- Plugin processes run under the same workspace/openat2 boundary as gateway
  file I/O, with no ambient host filesystem or network access.
- A plugin cannot reuse the TUI's gateway token or inherited FDs.
- Capability grants are per plugin and fail closed.
- Crash, cancellation, timeout, and gateway restart leave no live children.

Acceptance:

- An outside-workspace read/write attempt fails.
- A plugin cannot exec an unallowlisted binary.
- Plugin death does not take down the TUI or gateway.
- Security tests cover FD inheritance, token reuse, and workspace escape.

Remaining gate: VM/runtime, gateway control-socket token registration, and
end-to-end `sys.*` dispatch beyond the current `sys.ping`/mock `sys.exec`.

## 2. Tree-sitter syntax highlighting

Required outcome:

- Full parse on a debounced background worker; incremental `Tree::edit` is
  deferred to a later phase.
- Highlights are derived state, not stored in `PieceTable`.
- Plain text and unsupported languages remain usable without a parser.
- Scroll and search stay within the 0.1.0 latency envelope.

Acceptance:

- 100 MB plain text open remains < 200 ms; no parser is loaded for plain
  text.
- A 250 KiB Rust file key-to-screen stays < 1 ms Rust-side; files over
  256 KiB intentionally skip highlighting.
- Editing before/after syntax spans does not corrupt the text or index.
- Parser errors never panic the TUI.

## 3. LSP client

Required outcome:

- Rust LSP wire codec/diagnostics, Go `internal/lsp` manager, gateway
  `lsp.start/send/stop` routes, and client notification queuing are implemented.
  TUI worker, diagnostics, completion, hover, and definition are implemented and
  validated against rust-analyzer, gopls, and pyright. Code Actions, Rename,
  Formatting, and Document Symbols are implemented over a shared
  `WorkspaceEdit` engine, including background buffers and explicit `:wa`
  saves.
- Per-workspace language-server lifecycle; no server is started until a
  matching buffer is opened.
- Diagnostics, hover, definition, and completion are opt-in and cancellable.
- Trusted local language servers use the gateway's `-no-sandbox-lsp` process
  path; plugin-host remains the sandboxed boundary for third-party plugins.
- Server crash/restart does not lose buffer text or undo history.

Acceptance:

- Rust and one other language server can be started/stopped cleanly.
- Diagnostics appear for a real error and clear after correction.
- Definition/hover round-trip works on a fixture workspace.
- Shutdown leaves no orphan language-server process.

## 4. `:reconnect`

0.1.0 poisons the client on pipeline desynchronization and requires a TUI
restart. 0.1.1 may add:

- `:reconnect` is implemented: it stops the old gateway child, starts a new
  session, re-handshakes, and clears poison only after a successful handshake.
- Buffers remain in memory; no unsaved text is discarded.
- Failure keeps the client poisoned and reports the reason.
- This is recovery UX, not a relaxation of the security boundary.

## Out of scope for 0.1.1

- macOS support.
- 50 ms 100 MB open. Current requirement is 200 ms open/save; 50 ms needs
  delayed indexing plus fd passing.
- Replacing `PieceTable` or the gateway transport wholesale.

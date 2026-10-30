# 0.1.1 / Phase 0.7 Plan

Status: planned. 0.1.0 is the current release baseline.

## Priorities

1. **Plugin-host isolation** — the remaining Phase 0.6 security boundary and a
   prerequisite for a plugin ecosystem.
2. **Tree-sitter syntax highlighting** — the largest daily-experience gap.
3. **LSP client** — the step from a text editor to a development environment.
4. **`:reconnect` poison recovery** — quality-of-life, not a security boundary.

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

Blocking open item from 0.6: gateway control-socket registration and
inherited-FD token delivery.

## 2. Tree-sitter syntax highlighting

Required outcome:

- Incremental parse per buffer; no full-document reparse on each keystroke.
- Highlights are derived state, not stored in `PieceTable`.
- Plain text and unsupported languages remain usable without a parser.
- Scroll and search stay within the 0.1.0 latency envelope.

Acceptance:

- 100 MB plain text open remains < 200 ms; no parser is loaded for plain text.
- A 1 MB Rust file key-to-screen stays < 1 ms Rust-side.
- Editing before/after syntax spans does not corrupt the text or index.
- Parser errors never panic the TUI.

## 3. LSP client

Required outcome:

- Per-workspace language-server lifecycle; no server is started until a
  matching buffer is opened.
- Diagnostics, hover, definition, and completion are opt-in and cancellable.
- LSP traffic cannot bypass the gateway permission model.
- Server crash/restart does not lose buffer text or undo history.

Acceptance:

- Rust and one other language server can be started/stopped cleanly.
- Diagnostics appear for a real error and clear after correction.
- Definition/hover round-trip works on a fixture workspace.
- Shutdown leaves no orphan language-server process.

## 4. `:reconnect`

0.1.0 poisons the client on pipeline desynchronization and requires a TUI
restart. 0.1.1 may add:

- `:reconnect` stops the old gateway child, starts a new session, re-handshakes,
  and clears poison only after a successful handshake.
- Buffers remain in memory; no unsaved text is discarded.
- Failure keeps the client poisoned and reports the reason.
- This is recovery UX, not a relaxation of the security boundary.

## Out of scope for 0.1.1

- macOS support.
- 50 ms 100 MB open. Current requirement is 200 ms open/save; 50 ms needs
  delayed indexing plus fd passing.
- Replacing `PieceTable` or the gateway transport wholesale.

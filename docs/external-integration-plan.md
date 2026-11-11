# External Integration Plan

Status: 1a, 1b, 2, 3, the protocol v3 control plane, and the thin MCP adapter
are implemented. Real-TUI CLI and MCP smoke tests pass, and Codex successfully
called `anvil_status` and `anvil_query` through the adapter.

The remaining work is dependency cleanup and dogfood-driven tool removal. The
sections below preserve the gap analysis that drove the implementation; items
marked resolved are no longer blockers.

## 1. Current contract

### 1.1 Editor CLI

- `anvil [OPTIONS] [FILE|-]`
- `--editor` treats the positional path as the file to edit and permits that
  exact outside-workspace temporary path.
- `anvil -`, or non-terminal stdin, creates a `[stdin]` buffer and reattaches
  keyboard input to `/dev/tty`.
- `--read-only` rejects buffer mutations.
- `:cq` exits nonzero; a failed save in `--editor` mode stays sticky.
- `:w path` is save-as. `:wq path` saves then quits.
- stdin locations are a side list; `Alt-j` / `Alt-k` select and `Enter` opens.
- `--editor` and stdin instances do not claim the control socket, so they can
  run alongside the normal interactive instance.

### 1.2 Control socket

```text
$XDG_RUNTIME_DIR/anvil/control.sock
$XDG_RUNTIME_DIR/anvil/control.lock
```

- Parent directory mode `0700`; socket and lock mode `0600`.
- Same-UID peer verification via `SO_PEERCRED`.
- Exclusive lock; a live socket is never unlinked.
- Stale sockets are removed only after a liveness check fails.
- One control socket per user runtime directory, one editor instance.

Wire methods, JSON-RPC over the existing bounded `anvil-ipc` framing:

| Method | Params | Result |
|---|---|---|
| `control.status` | `{}` | version, workspace, file, dirty |
| `control.open` | `path`, optional `line`, `column` | accepted status |
| `control.save` | optional `path` | saved status |
| `control.edit` | `mode` (`set`/`insert`), `text` | new revision/status |
| `control.query` | `{}` | buffer, cursor, selection, tabs |
| `control.exec` | `command` | editor status |
| `control.quit` | optional `force` | accepted status |

`control.exec` runs any Ex command Anvil supports through the editor state
machine. It is not a general remote shell: there is no `:!` and no arbitrary
command syntax. It still reaches commands that spawn helper processes, notably
`:pkg install` (`GatewaySession::start_installer`) and `:plugin-exec` (plugin
host); those are part of the P0 capability policy gap described below. There is
no cross-user access.

### 1.3 `anvil-ctl`

```text
anvil-ctl [--json] [--timeout-ms N] [--spawn] status
anvil-ctl ... open <FILE> [+LINE[:COL]]
anvil-ctl ... save [FILE]
anvil-ctl ... edit set <TEXT|--stdin>
anvil-ctl ... edit insert <TEXT|--stdin>
anvil-ctl ... query [START[:END]]
anvil-ctl ... command <COMMAND>
anvil-ctl ... exec <COMMAND>
anvil-ctl ... quit [--force]
```

`--json` emits one object:

```json
{"ok":true,"command":"query","data":{}}
{"ok":false,"error":{"exit_code":5,"kind":"rpc","message":"...","rpc_code":-32002}}
```

Exit codes:

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | local/internal error |
| 2 | CLI usage error |
| 3 | no running editor instance |
| 4 | timeout |
| 5 | editor RPC error |

Command arguments are validated before connecting, so a bad command never
reports `no_instance` first. `--spawn` is opt-in and requires a terminal;
headless callers receive a usage error instead of a background TUI.

### 1.4 Verification surface

`scripts/ctl-smoke.sh` drives a real TUI through `anvil-ctl` and asserts:

- open, cursor placement, buffer text, revision;
- `edit set`, undo, redo;
- save and on-disk content;
- `:git-diff`, `:git-hunk-next`, `:git-stage-hunk`, and the resulting Git index;
- unsaved `quit` returns RPC exit 5;
- `quit --force` succeeds;
- session output exists.

Failure preserves the command transcript, stderr, gateway log, and captured
terminal screen under `/tmp/anvil-ctl-smoke.*`.

## 2. Resolved gaps and remaining work

These gaps drove the implementation. Resolved items remain here as design
history; the remaining work is at the end of this section.

### 2.1 Resolved: command policy

`control.exec` can run every editor Ex command, including:

- `:pkg install ...` / `:pkg install-file ...`
- `:plugin-exec`, `:plugin-enable`, `:plugin-start`, `:plugin-stop`
- `:theme`, `:theme-save`

Those are legitimate interactive commands, but they are not editor-navigation
primitives. An MCP client should not be able to install packages or change
persistent global configuration merely because it can edit a buffer.

Implemented: commands carry `Capability::{Read, Edit, Git, Admin}` and the
default remote surface denies admin commands. The same
`validate_remote_command` policy is used by the TUI, `anvil-ctl`, and MCP.

### 2.2 Resolved: bounded query

`control.query` accepts `start_line`, `end_line`, and `include_text`. The
result reports `line_count`, the returned range, and `text_truncated`; text is
still capped at `MAX_QUERY_TEXT` (256 KiB).

### 2.3 Resolved: range replacement

`control.edit` supports `set`, `insert`, and `replace`. `replace` takes
one-based line/byte-column bounds, requires `expected_revision`, rejects
invalid UTF-8 boundaries, and applies as one undo transaction. Deletion is
`replace` with empty text.


### 2.4 Resolved: stdin payloads

MCP sends edit text as JSON over the socket. `anvil-ctl edit set --stdin` and
`anvil-ctl edit insert --stdin` avoid argv limits for scripts.

### 2.5 Resolved: protocol discovery

`control.status` returns protocol version 3 and the capability list:

```json
{
  "protocol_version": "3",
  "capabilities": ["status", "open", "save", "edit", "query", "exec", "quit"]
}
```

### 2.6 Resolved: smoke harness in CI

`scripts/ctl-smoke.sh` and `scripts/mcp-smoke.sh` run in the Linux
`control-smoke` CI job against the real TUI and the real gateway.
The remaining work is dependency cleanup (`anvil-api` schema features) and
dogfood-driven MCP tool removal. Both are P2 and not blockers.

## 3. Work plan

### P0: completed contract hardening

All P0 items landed and are covered by unit tests plus `ctl-smoke`.

1. **Shared command manifest**
   - Put `CommandSpec`, `Capability`, and `EditMode` in `anvil-control`, not
     `anvil-tui`. `anvil-control` is already the non-UI external contract crate;
     TUI, CLI, and MCP all depend on it.
   - Do not put the manifest behind a TUI dependency. `anvil-mcp` must remain
     `anvil-mcp -> anvil-control` only.
   - Make `ControlRequest::Edit` carry `EditMode` instead of `String`.
   - `commands.rs` executes the command, completion and help checks derive from
     the manifest, `anvil-ctl` validates through it, and the MCP tool table is
     derived from it. Do not maintain a fourth command list in MCP.
   - If the manifest later outgrows `anvil-control`, move it to a small
     `anvil-command` crate and re-export it from `anvil-control`; do not make
     MCP depend on `anvil-tui`.
   - Reject `:pkg`, `:plugin-*`, `:theme*` from the default remote surface.
   - Treat package/plugin commands as process-spawning operations, not editor
     primitives; default-deny them for control and MCP clients.
   - Tests: admin commands return `POLICY_DENIED`; git/edit commands still work.
   - Document that `command` is an editor command surface, not a shell.

2. **Structured query bounds**
   - Add `start_line`/`end_line`/`include_text` to `control.query`.
   - Preserve the old response fields.
   - Tests: out-of-range clamping, empty range, truncated text, invalid range.

3. **Range replacement**
   - Add `edit replace` with line/byte-column bounds and `expected_revision`.
   - Tests: replace, delete, undo, stale revision, mid-codepoint rejection,
     read-only rejection.

4. **CLI stdin payloads**
   - Add `--stdin` for `edit set` and `edit insert`.
   - Tests: multiline text, empty text, invalid UTF-8, oversized payload.

5. **Protocol discovery**
   - Add `protocol_version` and `capabilities` to `control.status`.
   - Bump `PROTOCOL_VERSION` if the wire shape changes.
   - Update README and the CLI `status` tests.

6. **CI smoke**
   - Add a Linux job for `scripts/ctl-smoke.sh`.
   - Keep failure artifacts available from CI.
   - Do not make the job depend on a developer terminal or a real desktop.

7. **Error and security regression tests**
   - Usage error before connect: exit 2.
   - Missing instance: exit 3.
   - Timeout: exit 4.
   - RPC error: exit 5.
   - Socket/dir modes and peer mismatch behavior.
   - Remote `open` outside workspace rejects.
   - Remote save/open/edit rejects while a read-only view is active.

### P1: implemented MCP adapter

`anvil-mcp` is a stdio binary with no TUI, session, gateway, or editor
dependencies. It depends on `anvil-control` for the typed client and on JSON
serialization only.

Transport: MCP stdio, one request at a time. No network listener and no
additional control protocol.

Tools:

| Tool | Backing call |
|---|---|
| `anvil_status` | `control.status` |
| `anvil_open` | `control.open` |
| `anvil_query` | bounded `control.query` |
| `anvil_edit` | `control.edit` (`set`, `insert`, `replace`) |
| `anvil_save` | `control.save` |
| `anvil_command` | allowlisted `control.exec` |
| `anvil_quit` | `control.quit` |

The tool table is generated from the `anvil-control` command manifest and
filtered by capability. Rejecting an unsupported command must happen from the
same manifest the TUI and `anvil-ctl` use.

Rules:

- no direct filesystem reads or writes;
- workspace path checks remain in the TUI actor;
- all MCP results are structured JSON text, with `isError` on failure;
- no `anvil_spawn`; if no instance exists, return the no-instance error;
- no admin tools in the default tool list;
- document that the MCP client runs with the user's authority.

Acceptance:

- an MCP client can initialize, list tools, open a file, query a range, edit,
  save, run `:git-diff`, and quit;
- an edit after a stale revision fails without changing the buffer;
- no-instance, timeout, policy-denied, and RPC errors are distinguishable;
- `scripts/ctl-smoke.sh` and an MCP transcript smoke both pass in CI.

### P2: dogfood status

- Completed: Codex called `anvil_status` and `anvil_query` through the adapter
  against a real TUI.
- All seven tools are exercised by `scripts/mcp-smoke.sh`; none are removed yet
  because one dogfood turn is not enough evidence.
- Multi-file replace stays deferred until measured demand.
- Do not add session multiplexing, TCP, or cross-user control without a new
  threat model.

## 4. Order

1. Remote command capability policy.
2. Protocol discovery and structured query bounds.
3. Range replacement and CLI stdin payloads.
4. CI control-plane smoke.
5. MCP adapter.
6. External-agent dogfood.

The order is intentional: MCP is a consumer of the control contract, not a
reason to widen it. If the contract is still moving, MCP will encode the
instability.

## 5. Non-goals

- A general remote shell or arbitrary process execution.
- A second terminal input stack.
- Remote control across users or machines.
- Auto-spawning a TUI from a non-terminal process.
- Editing files outside the workspace without an explicit flag and a documented
  trust decision.
- Exposing package installation, plugin lifecycle, or persistent settings to
  every control client.
- Inline rich links in the raw log before the Locations panel exists.
- Windows drive-letter parsing; Linux-first remains the contract.

## 6. Verification

Every slice runs:

```sh
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
go test ./go/...
go vet ./go/...
scripts/ctl-smoke.sh
```

Changed control or MCP surfaces additionally require a real TUI smoke and, once
the adapter exists, an MCP stdio transcript smoke. Changed distribution
surfaces require the install-layout smoke.

## 7. Design history

The original plan treated the gateway socket as a reusable editor control
endpoint, treated `$EDITOR` cancellation as synonymous with `:q!`, and assumed
a pipe could be consumed while crossterm still read stdin. Those assumptions
were wrong:

- the gateway socket is per-session and authenticated for a different protocol;
- `:cq` is the explicit nonzero editor exit;
- stdin must be read before the TUI starts and keyboard input must be
  reattached to `/dev/tty`.

The implemented design keeps the gateway and control sockets separate, puts the
control server in the TUI runtime, and routes every mutation through the actor.
That separation is now a fixed boundary, not a future option.

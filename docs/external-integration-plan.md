# External Integration Plan

Status: 1a (`--editor`, `:cq`, sticky save failure), 1b (`anvil -`, stdin,
`/dev/tty`, `--read-only`), 2 (single-instance control and `anvil-ctl`), and
3 (stdin Locations panel) are implemented. The control plane is at protocol
version 2, with a real-TUI CLI smoke harness.

The remaining work is not another transport. It is contract hardening followed
by a thin MCP adapter over the existing control socket. MCP must not read or
write workspace files directly.

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
machine. It is not a remote shell: there is no `:!`, no process spawn, and no
cross-user access. It can still reach package/plugin/theme commands, which is a
policy gap described below.

### 1.3 `anvil-ctl`

```text
anvil-ctl [--json] [--timeout-ms N] [--spawn] status
anvil-ctl ... open <FILE> [+LINE[:COL]]
anvil-ctl ... save [FILE]
anvil-ctl ... edit set <TEXT>
anvil-ctl ... edit insert <TEXT>
anvil-ctl ... query
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

## 2. Known gaps

These are evidence-backed gaps between the current control plane and a safe MCP
adapter.

### 2.1 Command policy is too broad

`control.exec` can run every editor Ex command, including:

- `:pkg install ...` / `:pkg install-file ...`
- `:plugin-exec`, `:plugin-enable`, `:plugin-start`, `:plugin-stop`
- `:theme`, `:theme-save`

Those are legitimate interactive commands, but they are not editor-navigation
primitives. An MCP client should not be able to install packages or change
persistent global configuration merely because it can edit a buffer.

Decision needed: remote command capabilities. Recommended default:

- `read`: status, query, open;
- `edit`: edit, save, buffer mutation, editor commands;
- `git`: diff, hunk navigation, stage/unstage;
- `admin`: package, plugin lifecycle, theme persistence, settings persistence.

MCP exposes only `read`, `edit`, and `git` by default. `admin` requires an
explicit opt-in and must not be discoverable as a normal tool.

### 2.2 Query returns the whole buffer

`control.query` returns buffer text up to `MAX_QUERY_TEXT` (256 KiB) in one
response. That is acceptable for the CLI smoke test and normal source files,
but it is a poor MCP contract:

- large buffers waste tokens and latency;
- there is no way to request only the visible region or a line range;
- the response does not expose a stable page cursor.

Add optional query bounds:

```json
{"start_line":1,"end_line":200,"include_text":true}
```

The default stays compatible. The result must include total line count,
returned range, and `text_truncated`.

### 2.3 Editing is coarse

`control.edit` supports only `set` (replace the whole buffer) and `insert` at
the cursor. There is no surgical range replacement or deletion.

Add a bounded, revision-guarded operation:

```json
{
  "mode": "replace",
  "start": {"line": 12, "byte_column": 5},
  "end": {"line": 12, "byte_column": 17},
  "text": "new_name",
  "expected_revision": 7
}
```

Rules:

- one-based lines and byte columns within the line;
- reject invalid UTF-8 boundaries;
- reject stale `expected_revision`;
- reject read-only buffers and help views;
- apply as one editor transaction so undo is one step.

Deletion is `replace` with empty text.

### 2.4 Large payloads do not fit argv

`anvil-ctl edit set <TEXT>` puts the payload in argv. The MCP adapter should use
`anvil-control::Client` directly and send JSON over the socket, avoiding
`ARG_MAX` and shell quoting. The CLI should still gain a stdin form for scripts:

```text
anvil-ctl edit set --stdin
anvil-ctl edit insert --stdin
```

### 2.5 Protocol capabilities are not discoverable

`PROTOCOL_VERSION` exists, but `control.status` does not return it, and there is
no method/capability list. Add:

```json
{
  "protocol_version": "2",
  "capabilities": ["status", "open", "save", "edit.set", "edit.insert",
                   "query", "exec", "quit"]
}
```

This lets MCP fail clearly when an older editor is running instead of
discovering missing methods through RPC errors.

### 2.6 The smoke harness is not in CI

`scripts/ctl-smoke.sh` is currently manual. It needs a Linux CI job that
installs its runtime tools and runs it against the release or debug binary.
Without that, the control contract can regress while unit tests stay green.

Prerequisites to verify in CI: `tmux`, `python3`, `git`, a writable
`XDG_RUNTIME_DIR`, and no pre-existing Anvil instance.

## 3. Work plan

### P0: contract hardening

Do these before writing MCP.

1. **Remote command policy**
   - Define capability levels and a command allowlist.
   - Reject `:pkg`, `:plugin-*`, `:theme*` from the default remote surface.
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

### P1: thin MCP adapter

Create one binary crate, `anvil-mcp`, with no TUI, session, gateway, or editor
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

### P2: dogfood

- Run the MCP adapter against a real external agent, not only a test client.
- Record which tools the agent actually uses; remove unused tool surface.
- Revisit multi-file replace only after measured demand.
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

# External Integration Plan

Status: proposed. This plan covers Unix editor integration: `$EDITOR`, stdin
pipelines, diagnostic-aware pipe mode, and single-instance remote control. It
is separate from [small-features-plan.md](small-features-plan.md); these changes
cross the CLI, terminal, workspace boundary, and IPC layers, so they are not
small keymap additions.

Verdict: the three issues are directionally correct, but the acceptance
criteria need sharper contracts. In particular, `$EDITOR` cancellation is not
the same as `:q!`, the existing gateway socket is not an editor control socket,
and a pipe cannot be consumed while crossterm still reads terminal input from
stdin.

## 1. Current facts

- `anvil` currently accepts one optional `FILE`, `-w/--workspace`, `--`, help,
  and version. There is no `--read-only`, `-`, stdin mode, or remote mode.
- `Model::open_path` rejects files outside the workspace with
  `path is outside workspace`.
- `:wq` exits 0 on a successful save. `:q!` also exits 0. There is no `:cq`
  or other explicit nonzero editor exit.
- `:w` with no filename reports `E: no file name`; `:w path` already performs
  save-as.
- The only current read-only surface is the embedded help page
  (`help_back`); there is no per-buffer read-only flag.
- The terminal loop uses crossterm on stdin. If stdin is a pipe, normal event
  reading no longer has a terminal input source.
- `GatewaySession` creates a random per-session socket under the temp directory
  such as `/tmp/anvil-tui-<pid>-<random>.sock`. It is a gateway handshake
  channel, not a stable editor-control endpoint.
- `anvil-ipc` already provides bounded, framed UDS transport and can be reused
  for a new control protocol.
- There is no compiler-log parser, location model, or clickable location panel.

## 2. Issue 1: `$EDITOR` and stdin

### 2.1 What already works

- Anvil is a foreground TUI, so it already blocks the invoking process while it
  is open.
- A normal `:wq` save exits 0.
- A startup error exits 1.
- `git commit` normally passes `.git/COMMIT_EDITMSG`, which is inside the
  repository workspace and already opens.
- `:w path` already saves an unnamed buffer.

### 2.2 Blockers

1. **Exit code contract is incomplete.** `:q!` exits 0. Vim-style cancellation
   with a nonzero status uses `:cq`, not `:q!`. Add `:cq` and define save
   failure as nonzero. `:q!` stays 0 for intentional cancellation, but in
   `--editor` mode a prior save failure remains sticky and makes the final
   exit nonzero.
2. **External temp files are rejected.** `crontab -e`, some `systemctl edit`
   paths, and other tools use files outside the cwd workspace. Current
   `open_path` rejects them.
3. **stdin is not a file source.** The TUI reads events from stdin. A pipe
   would be consumed by the editor loop or return EOF.
4. **Read-only is help-only.** `--read-only` needs a per-buffer flag and
   mutation guard.
5. **Unnamed save is only a command hint.** `:w` already says “no file name”,
   but `[stdin]`/`[Read-Only]` needs a visible status and a clear
   `:w <path>` instruction.

### 2.3 Proposed contract

Exit codes:

| Code | Meaning |
|---|---|
| 0 | successful `:wq`, clean `:q`, or intentional `:q!` with no prior save failure |
| 1 | startup error, save failure, later quit after a save failure in `--editor` mode, or explicit `:cq` |
| 2 | CLI usage error |

`$EDITOR` mode:

- Add `--editor` as an explicit mode that treats the single positional path as
  the file to edit.
- When `--editor` is used without `-w`, allow that exact canonical path even
  when it is outside the current workspace. The workspace remains the cwd for
  tree/fuzzy/LSP unless `-w` is supplied.
- Do not silently widen the workspace to `/tmp` or `/`.
- For temporary files, document `EDITOR='anvil --editor'`; if `$EDITOR` is just
  `anvil`, keep the current workspace rule and fail with the explicit path
  error rather than guessing.
- `--editor` must not bypass file-type, size, UTF-8, or read-only checks.

stdin mode:

- Accept `anvil -` explicitly and also detect non-terminal stdin.
- Read stdin before entering the TUI, bounded by the existing open limit
  (currently 64 MiB). Reject oversized or invalid UTF-8 input with a clear
  status/error.
- After reading, open `/dev/tty` for keyboard input and replace the event
  source, or use an equivalent terminal-input abstraction. Do not read events
  from the consumed pipe.
- Require a terminal for stdout unless a future non-interactive output mode is
  explicitly added.
- Create an unnamed buffer with title `[stdin]`.
- Default stdin buffer: editable, no path. `:w <path>` works; `:w` without a
  path reports `E: no file name; use :w <path>`.
- `--read-only` creates a read-only stdin buffer. `:w` and mutating commands
  reject; decide explicitly whether `:w <path>` is allowed. Recommended:
  allow `:w <path>` as the explicit copy-out path, reject same-buffer writes.
- Show `[stdin]` and `[Read-Only]` in the buffer/status surface.

### 2.4 Tests

- PTY: `anvil --editor <temp-outside-workspace>`, edit, `:wq`, exit 0, content
  persisted.
- PTY: `:cq` exits 1 and does not claim success.
- PTY: in `--editor` mode, a failed write sets a sticky failure; `:q!` does not
  mask it and the process exits 1.
- Pipe: `printf 'a\nb\n' | anvil -` with a PTY for stdout; buffer shows
  `[stdin]`; `:w out.txt` writes.
- Pipe with `--read-only`: edit commands reject, `:w out.txt` follows the
  chosen contract.
- Non-TTY stdout, unavailable `/dev/tty`, oversized stdin, invalid UTF-8, and
  binary input all fail clearly without hanging.
- `git commit` smoke with `EDITOR='anvil --editor'`.

## 3. Issue 2: single instance and `anvil-ctl`

### 3.1 What can be reused

- `anvil-ipc` framing, bounded frames, and UDS transport.
- The editor event loop as the place where remote requests become model
  actions.
- Existing `open_path`, command execution, and status rendering.

### 3.2 What cannot be reused directly

- The current gateway socket is random, per session, and authenticated for
  gateway handshakes. It is not a stable editor control endpoint.
- `anvil-session` owns host/plugin session state, not UI tabs and cursor
  placement. The control server belongs in the TUI runtime or a small
  `anvil-control` crate; it must not force UI dependencies into
  `anvil-session`.

### 3.3 Proposed contract

- New control protocol over UDS, reusing `anvil-ipc` framing.
- Stable path: `$XDG_RUNTIME_DIR/anvil/control.sock`; fail clearly if the
  runtime dir is unavailable. Parent directory mode `0700`, socket `0600`.
- Verify peer UID with `SO_PEERCRED`; reject other users.
- Use a lock file plus stale-socket cleanup; a connect failure must not unlink
  a socket owned by a live process.
- Methods:
  - `ping`/`status`: version, workspace, active file, dirty state.
  - `open {path, line?, column?}`: canonicalize, enforce the active workspace,
    queue the open on the event loop, and jump to the clamped position.
  - `exec {command}`: allowlist only internal editor commands; no `:!`, no
    package install, no arbitrary shell. Start with `:format`, `:w`, `:wq`,
    and `:q` only if their remote semantics are explicit.
- Remote requests must never block rendering or file I/O; they enqueue work and
  return an accepted/rejected response.
- A second `anvil <file>` should open in the running instance by default only
  when the target is inside that instance's workspace. Different workspaces
  are rejected with a message, not silently switched.
- `anvil-ctl open <file> +<line>[:<col>]`.
- `anvil-ctl exec <command>`.
- `anvil-ctl status`.
- Ship `anvil-ctl` as a separate binary in the release layout and installer; it
  must not require the TUI's full dependency graph.
- `anvil-ctl` exits nonzero when no instance exists by default. Auto-spawn is
  opt-in (`--spawn`) and must require a usable terminal; never spawn a TUI from
  a headless pipe.

### 3.4 Tests

- Same-UID client succeeds; different UID is rejected (where testable).
- Socket 0600 and parent 0700.
- Stale socket cleanup; live socket never removed.
- Two concurrent starts acquire one instance and one control socket.
- Remote open with line/column; folded line and out-of-range clamping.
- Remote open outside workspace rejects.
- `exec` allowlist rejects shell and package commands.
- Remote request while a modal/dialog is open is queued or rejected
  deterministically.
- Server exit and client timeout return a clear nonzero status.

## 4. Issue 3: diagnostic-aware pipe mode

### 4.1 Contract

- The raw stdin remains a text buffer. Parsed locations are a side list, not a
  rewrite of the log text.
- MVP UI: a bounded Locations panel below/above the log with keyboard selection
  and Enter-to-open. Inline clickable text is a later optimization.
- Mouse click on a visible location row follows the existing mouse hit-testing
  rules.
- Supported patterns initially:
  - `path:line:column: message`
  - `path:line: message` with column 1
  - `--> path:line:column` (Rust style)
  - `path(line,column): message` (clang/MSVC-like)
- Strip ANSI CSI sequences before parsing. Do not strip ordinary brackets or
  paths.
- Resolve relative paths against the process cwd/workspace. Canonicalize before
  opening.
- Bound input and output:
  - stdin cap: 64 MiB;
  - max parsed lines: 100,000;
  - max locations: 10,000;
  - max message display width per row, with truncation;
  - no unbounded regex backtracking.
- Existing files inside the workspace: open and clamp `line`/`column`.
- Existing files outside the workspace: reject by default; future
  `--allow-external` may open read-only.
- Missing files: keep the log line as plain text; no error popup.
- A malformed line is a normal log line, not a parser error.

### 4.2 Tests

- Parser unit tests for all supported formats, ANSI prefixes, spaces in paths,
  relative paths, nonexistent paths, out-of-workspace paths, enormous lines,
  and malformed input.
- TUI test: pipe `src/main.rs:2:3: error`; Enter opens the file at the clamped
  position.
- Pipe with no locations: no panel, only `[stdin]`.
- Duplicate locations are deduplicated or shown with a count; decide once.
- Narrow-terminal rendering and mouse selection.

## 5. Order

1. `$EDITOR` exit-code contract and external-temp-file path.
2. stdin read, `/dev/tty` input, unnamed buffer, `--read-only`.
3. Diagnostic Locations panel on top of stdin mode.
4. Single-instance control socket and `anvil-ctl`.

Issue 2 can run in parallel with Issue 1 only if it has a separate owner and
does not edit the terminal loop. Issue 3 depends on Issue 1b.

## 6. Non-goals

- A general remote shell or arbitrary command execution.
- A second terminal input stack.
- Remote control across users or machines.
- Auto-spawning a TUI from a non-terminal process.
- Editing files outside the workspace without an explicit flag and a documented
  trust decision.
- Inline rich links in the raw log before the Locations panel exists.
- Windows drive-letter parsing; Linux-first remains the contract.

## 7. Acceptance corrections

These changes to the issue text are recommended:

- “Exit 1 on cancel” should be “`:cq` exits 1; `:q!` remains a successful
  intentional exit unless a prior `--editor` save failure is sticky.”
- “Reuse existing UDS” should be “reuse `anvil-ipc` framing; add a new editor
  control socket.”
- “stdin pipe” should explicitly include reopening `/dev/tty` for input.
- “Clickable inline links” should start as a keyboard/mouse Locations panel.
- “Auto-spawn” should be opt-in and terminal-only.

# Remote/SSH workflow

Anvil currently runs on the machine that owns the files. SSH is terminal
transport, not a remote-development protocol: there is no Zed-style remote
workspace/index/runtime separation. Local Anvil opens remote files only if they
are mounted or otherwise reachable by the remote Anvil process.

## Install and run on the remote host

Build or install the full layout on that host and keep the companion binaries
beside `anvil`:

```bash
anvil doctor --profile installed
anvil --workspace /path/to/project
```

Language servers are not bundled. Install the servers you need on the remote
host's `PATH`; local servers cannot serve remote files.

## SSH environment

SSH usually does not forward `COLORTERM`. A server-side process often sees
`TERM=xterm-256color` and no `COLORTERM`, so automatic truecolor detection
falls back to 256 colors.

Options:

```bash
# Force only when the local terminal and SSH path support 24-bit color.
ANVIL_TRUECOLOR=1 anvil --workspace /path/to/project
anvil --truecolor=always --workspace /path/to/project
```

Or forward the variable deliberately:

```text
# ~/.ssh/config on the client
SendEnv COLORTERM

# /etc/ssh/sshd_config on the server
AcceptEnv COLORTERM
```

Do not force truecolor just because a screenshot looks correct. Check with
`anvil doctor --json` and inspect the `truecolor` and `remote` checks.

## tmux and screen

The multiplexer is another terminal. Both the outer terminal and tmux/screen
must allow RGB. For tmux, configure an appropriate terminal and RGB feature
set for the installed version, then verify inside the session:

```bash
printf 'TERM=%s COLORTERM=%s TMUX=%s\n' "$TERM" "$COLORTERM" "${TMUX:-}"
anvil doctor --json
```

Anvil warns when an SSH session is inside tmux/screen because forwarding
cannot be assumed.

## Control socket

The control socket is per workspace and normally lives under
`$XDG_RUNTIME_DIR/anvil/`. SSH sessions may not set `XDG_RUNTIME_DIR`; Anvil
falls back to `~/.local/state/anvil/runtime/anvil/`.

```bash
anvil-ctl status
anvil-ctl --workspace /path/to/project status
```

The socket is separate from the gateway/stdin socket and is removed on normal
shutdown.

## Diagnostics

Run these on the remote host:

```bash
anvil doctor --json
anvil-ctl status
```

Inside Anvil:

```text
:lsp-status
:watch-status
:config-reload
```

The `remote` doctor check reports SSH/tmux color-forwarding gaps. The existing
`truecolor`, `runtime-dir`, `config`, `locale`, and `lsp` checks also apply.

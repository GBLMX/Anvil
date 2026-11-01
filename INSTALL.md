# Installing Anvil

Anvil is currently built from source. Install the editor and its three sibling
tools into the same directory:

```text
anvil
anvil-gateway
anvil-exec-runner
anvil-plugin-host
```

The editor discovers the gateway beside itself. If you move only `anvil`, file
I/O is refused unless `ANVIL_GATEWAY` points at the gateway binary.

## Requirements

- Linux; Landlock requires Linux 5.13 or newer.
- Rust 1.88 or newer (the repository pins the toolchain in
  `rust-toolchain.toml`).
- Go 1.26.10 or newer (the repository pins the version in `go.mod`).
- A locale with UTF-8 support.

Language servers are not bundled. Install the servers you want on `PATH`:
`rust-analyzer`, `gopls`, `pyright`/`pylsp`, `typescript-language-server` or
`vtsls`, `clangd`, `jdtls`, `lua-language-server`,
`vscode-json-language-server`, `yaml-language-server`, and/or
`bash-language-server`.

## Build

From the repository root:

```bash
cargo build --release -p anvil-tui -p anvil-plugin-host
GOTOOLCHAIN=go1.26.10 go build -o target/release/anvil-gateway ./go/cmd/anvil-gateway
GOTOOLCHAIN=go1.26.10 go build -o target/release/anvil-exec-runner ./go/cmd/anvil-exec-runner
```

## Install for one user

```bash
install -d "$HOME/.local/bin"
install -m 0755 \
  target/release/anvil \
  target/release/anvil-gateway \
  target/release/anvil-exec-runner \
  target/release/anvil-plugin-host \
  "$HOME/.local/bin/"
```

Make sure `$HOME/.local/bin` is on `PATH`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

## Verify

```bash
anvil --version
anvil --help
```

Open a workspace:

```bash
cd ~/src/my-project
anvil --workspace "$PWD"
```

Confirm:

- the editor renders and `:q` exits;
- `Space e` opens the file tree;
- opening and saving a file works;
- `:reconnect` starts a fresh gateway;
- an external `touch`, `mkdir`, `rm`, or `mv` refreshes the tree after the
  debounce interval.

If opening or saving reports that the gateway is unavailable, check that all
four binaries are in the same directory and that the gateway starts with the
same user as the editor.

## Runtime boundaries

- The gateway is per editor session; it is not a global daemon or a systemd
  socket-activated service.
- The gateway performs workspace path confinement and `fs.read`/`fs.write`
  permission checks. It is not the plugin sandbox.
- `sys.exec` runs through `anvil-exec-runner`, which applies Landlock/seccomp.
- cgroup v2 limits are optional. Without a delegated cgroup root, exec falls
  back to `RLIMIT_NPROC`; see [the deployment model](docs/deployment.md).
- Plugins are not loaded in the editor process.

## Environment overrides

| Variable | Purpose |
|---|---|
| `ANVIL_GATEWAY` | Explicit path to `anvil-gateway` |
| `ANVIL_GATEWAY_LOG` | Gateway stderr destination |
| `ANVIL_RG` | Explicit `rg` path for fuzzy file listing |
| `ANVIL_GIT` | Explicit `git` path for gutter diffs |

## Uninstall

```bash
rm "$HOME/.local/bin/anvil" \
   "$HOME/.local/bin/anvil-gateway" \
   "$HOME/.local/bin/anvil-exec-runner" \
   "$HOME/.local/bin/anvil-plugin-host"
```

Editor state, workspace files, and language-server caches are not removed by
the uninstall.

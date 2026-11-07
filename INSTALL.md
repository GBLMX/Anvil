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
`vscode-json-language-server`, `yaml-language-server`,
`bash-language-server`, `marksman`, `taplo`, `vscode-html-language-server`,
`vscode-css-language-server`, and/or `zls`.

## Build

From the repository root:

```bash
cargo build --release -p anvil-tui -p anvil-plugin-host
GOTOOLCHAIN=go1.26.10 go build -o target/release/anvil-gateway ./go/cmd/anvil-gateway
GOTOOLCHAIN=go1.26.10 go build -o target/release/anvil-exec-runner ./go/cmd/anvil-exec-runner
```

## Release layout

Build a self-contained layout with the binaries and official plugin sources:

```bash
scripts/release-layout.sh "$PWD/dist/anvil"
```

The layout is:

```text
bin/anvil
bin/anvil-gateway
bin/anvil-exec-runner
bin/anvil-plugin-host
share/anvil/plugins/trim-whitespace/
```

`anvil` discovers `../share/anvil/plugins` relative to itself. A portable
layout may instead place plugins in `bin/plugins/`.

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

Official plugin sources ship under `share/anvil/plugins`. For a manual
`~/.local/bin` install:

```bash
install -d "$HOME/.local/share/anvil/plugins"
cp -R plugins/official/. "$HOME/.local/share/anvil/plugins/"
```

If the release also ships signed packages under `share/anvil/packages`, import
them into the managed store so they can be enabled, disabled, and updated:

```bash
for package in "$HOME/.local/share/anvil/packages"/*.anvil-plugin; do
  anvil-gateway \
    -install-plugin "$package" \
    -plugin-signature "$package.sig" \
    -plugin-public-key "$(cat "$HOME/.local/share/anvil/packages/official.pub")" \
    -plugin-source official
done
```

Bundled sources are a read-only fallback. A managed package with the same ID
takes precedence.

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
- `Space f` lists workspace files, previews the selection, and Enter opens it;
- `:reconnect` starts a fresh gateway;
- creating a new file (for example `touch new.txt`), `mkdir`, `rm`, or `mv`
  refreshes the tree after the debounce interval.

If opening or saving reports that the gateway is unavailable, check that all
four binaries are in the same directory and that the gateway starts with the
same user as the editor.

## Runtime boundaries

- The gateway is per editor session; it is not a global daemon or a systemd
  socket-activated service.
- The gateway performs workspace path confinement and `fs.read`/`fs.write`
  permission checks. It is not the plugin sandbox.
- `sys.exec` runs through `anvil-exec-runner`, which applies Landlock/seccomp.
- Plugin hosts require a delegated cgroup v2 root; limits are applied before
  `execve` and startup fails closed without one. The exec runner still falls
  back to `RLIMIT_NPROC`; see [the deployment model](docs/deployment.md).
- Plugins are not loaded in the editor process.

## Environment overrides

| Variable | Purpose |
|---|---|
| `ANVIL_GATEWAY` | Explicit path to `anvil-gateway` |
| `ANVIL_GATEWAY_LOG` | Gateway stderr destination |
| `ANVIL_RG` | Explicit `rg` path for fuzzy file listing |
| `ANVIL_GIT` | Explicit `git` path for gutter diffs |
| `ANVIL_BUNDLED_PLUGIN_DIR` | Explicit bundled official plugin directory |
| `ANVIL_PLUGIN_DIR` | Directory whose child directories contain `plugin.json` |
| `ANVIL_CGROUP_ROOT` | Explicit delegated cgroup v2 root for plugin hosts |

## Uninstall

```bash
rm "$HOME/.local/bin/anvil" \
   "$HOME/.local/bin/anvil-gateway" \
   "$HOME/.local/bin/anvil-exec-runner" \
   "$HOME/.local/bin/anvil-plugin-host"
```

Editor state, workspace files, and language-server caches are not removed by
the uninstall.

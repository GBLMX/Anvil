# Multi-host install lessons

Status: Reference. Validated 2026-11-15 against three hosts.

This document records what installing Anvil on Arch, Debian, and Ubuntu
revealed about build portability, installation, and deployment. It is not a
support matrix; it is a list of constraints the release process must either
handle or document.

## Hosts

| Host | OS | CPU | glibc | Install result |
|---|---|---|---|---|
| Local workstation | Arch Linux | supports `x86-64-v3` | 2.42 | Works |
| `r420` | Debian 13 (trixie) | Xeon E5-2407 v2, `x86-64-v2` | 2.41 | Native build; works |
| `ecs` | Ubuntu 26.04 LTS | Xeon Platinum, `x86-64-v4` | 2.43 | Prebuilt layout; works |

The Debian and Ubuntu hosts are terminal-only. Verification used a TUI inside
`tmux` plus `anvil-ctl`; this is **remote TUI over SSH/tmux**, not a headless
editor service.

## Truecolor over SSH

SSH does not reliably forward `COLORTERM`. A server-side process commonly sees
`TERM=xterm-256color` and no `COLORTERM`, so automatic detection legitimately
falls back to 256 colors even when the local terminal is 24-bit capable.

Anvil already supports an explicit override:

```bash
ANVIL_TRUECOLOR=1 anvil --workspace /path/to/project
```

`ANVIL_TRUECOLOR=0` forces 256-color quantization. Without the override,
`COLORTERM=truecolor` or `COLORTERM=24bit` is honored when the SSH/tmux
environment preserves it.

`btop` looking correct is not evidence that its terminal detection succeeded:
it has explicit truecolor configuration and can force the profile.


## Findings

### 1. Build-host ISA leaks into release artifacts

The first Arch-built `anvil` failed on Debian with:

```text
anvil: CPU ISA level is lower than required
```

The ELF carried an `x86-64-v3` ISA requirement. Debian's CPU only supports
`x86-64-v2`.

Arch host-local Cargo configuration used `target-cpu=native`. Removing or
overriding Rust flags is necessary but not always sufficient: on Arch, even a
trivial GCC-linked binary carried the v3 property. A binary built on Arch is
therefore not a portable release artifact for v2 hosts.

Rule: release artifacts must be built inside a controlled oldest-supported
environment, or explicitly checked for ELF ISA properties before publication.

### 2. glibc is a hard compatibility floor

| Host | glibc |
|---|---:|
| Debian 13 | 2.41 |
| Arch | 2.42 |
| Ubuntu 26.04 | 2.43 |

A binary linked on a newer glibc may reference symbols absent from an older
target. Ubuntu success does not imply Debian compatibility.

Release builds should run in the oldest supported glibc environment, not on the
developer workstation and not in the newest CI image.

### 3. Debian source builds need a deterministic dependency source

The Debian host had Rust but no Go, and the crates.io index update blocked
native compilation. An offline vendor directory generated from the host cache
was required.

This is not an Anvil runtime issue, but it is a release reproducibility issue:
build hosts should receive a locked dependency source, not depend on a live
index update.

### 4. Mixed Rust and Go artifact sources are not reproducible

The working Debian install combined:

- Rust binaries built natively on Debian
- Go binaries copied from the Arch layout

Both ran, including the file I/O gateway smoke, but this is not a repeatable
release process. Rust and Go tools should be built in the same controlled
environment with the same target policy.

Go should be built with a compatible `GOAMD64` level, not inherited from an
arbitrary host.

### 5. User installs are not root installs

The first Debian install went to `/home/him/.local`; `root` could not find
`anvil` because `/root/.local/bin` was not on `PATH`.

Installing to `/root/.local` fixed the immediate error, but root installation
should not be the default:

- user install: `~/.local`
- system install: `/usr/local` or a distribution package
- root install: explicit system-wide deployment only

`~/.local/bin` must be added to the shell profile for the user that runs Anvil.

### 6. Version output is not an install smoke test

`anvil --version` does not start the gateway. The meaningful check is the
control path:

```text
anvil-ctl status
anvil-ctl open <file>
anvil-ctl query
anvil-ctl edit set ...
anvil-ctl save
anvil-ctl quit --force
```

On Debian and Ubuntu this verified real file open, edit, and write through the
gateway.

### 7. Plugin hosts need cgroup v2 delegation

The Debian user session exposed `cgroup2fs` but the cgroup root was not
writable. The editor and gateway worked; plugin hosts require a delegated
cgroup v2 root and otherwise fail closed.

A successful editor install does not imply plugin support. Deployment docs
must state this explicitly and provide the `ANVIL_CGROUP_ROOT` escape hatch.

## Repeatable smoke recipe

Run in a pseudo-terminal, for example `tmux`:

```bash
export PATH="$HOME/.local/bin:$PATH"
export XDG_RUNTIME_DIR=/tmp/anvil-runtime
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"

mkdir -p /tmp/anvil-smoke
printf 'hello\n' >/tmp/anvil-smoke/a.txt
tmux new-session -d -s anvil-smoke \
  "cd /tmp/anvil-smoke && XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR anvil --workspace /tmp/anvil-smoke"
sleep 3

anvil-ctl --workspace /tmp/anvil-smoke --json status
anvil-ctl --workspace /tmp/anvil-smoke --json open a.txt
anvil-ctl --workspace /tmp/anvil-smoke --json edit set "gateway-write-ok"
anvil-ctl --workspace /tmp/anvil-smoke --json save
cat /tmp/anvil-smoke/a.txt
anvil-ctl --workspace /tmp/anvil-smoke --json quit --force
```

## Release recommendations

1. Build release artifacts in the oldest supported glibc image, not on Arch.
2. Pin both Rust and C/C++ target policy to a baseline ISA; do not inherit
   `target-cpu=native`.
3. Build Rust and Go tools from one locked dependency source in one job.
4. Check ELF ISA and glibc requirements before publishing:

   ```bash
   readelf -n bin/anvil
   objdump -T bin/anvil | grep GLIBC_
   ```

5. Run the gateway smoke on at least one old-CPU, old-glibc target.
6. Document:
   - user vs system install
   - `PATH` setup
   - cgroup delegation for plugin hosts
   - the fact that SSH/tmux is remote TUI, not a headless daemon
7. Do not call an install verified from `anvil --version` alone.

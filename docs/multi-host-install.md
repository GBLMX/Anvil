# Multi-host install lessons

Status: Reference. Validated 2026-11-20 against four native hosts and
2026-11-25 against a WSL2 host.

This document records what installing Anvil on Arch, Debian, Ubuntu, Fedora,
and WSL2 revealed about build portability, installation, and deployment. It is
not a support matrix; it is a list of constraints the release process must
either handle or document.

## Hosts

| Host | OS | CPU | glibc | Install result |
|---|---|---|---|---|
| Local workstation | Arch Linux | supports `x86-64-v3` | 2.42 | Works |
| `r420` | Debian 13 (trixie) | Xeon E5-2407 v2, `x86-64-v2` | 2.41 | Native build; works |
| `ecs` | Ubuntu 26.04 LTS | Xeon Platinum, `x86-64-v4` | 2.43 | Prebuilt layout; works |
| `fedora-vm` | Fedora Linux 44 | QEMU x86_64 guest | 2.43 | Generic CLI layout and lifecycle smoke; TUI not tested |
| `wsl2` | Arch Linux (WSL2) | Intel Core i7-12700H | 2.44 | Native build, doctor, and gateway smoke; works with systemd |

The Debian and Ubuntu hosts are terminal-only. Verification used a TUI inside
`tmux` plus `anvil-ctl`; this is **remote TUI over SSH/tmux**, not a headless
editor service.

Fedora was validated through the command line, `doctor --profile installed`,
and the release lifecycle smoke. Its interactive TUI was not part of that run.

The Fedora test harness needed temporary SELinux permissive mode because the
QEMU guest-agent domain cannot execute `/tmp` binaries; the persistent SELinux
configuration was not changed.

## Truecolor over SSH

SSH does not reliably forward `COLORTERM`. A server-side process commonly sees
`TERM=xterm-256color` and no `COLORTERM`, so automatic detection legitimately
falls back to 256 colors even when the local terminal is 24-bit capable.

Anvil already supports an explicit override:

```bash
ANVIL_TRUECOLOR=1 anvil --workspace /path/to/project
```

The same choice can be passed on the command line:

```bash
anvil --truecolor=always --workspace /path/to/project
anvil --truecolor=never --workspace /path/to/project
```

`ANVIL_TRUECOLOR=0` forces 256-color quantization. Without the override,
`COLORTERM=truecolor` or `COLORTERM=24bit` is honored when the SSH/tmux
environment preserves it.

### Color profile policy

Resolution order:

1. `--truecolor=always|never` or `ANVIL_TRUECOLOR=1|0`
2. `COLORTERM=truecolor` or `COLORTERM=24bit`
3. `TERM=*direct*` or `TERM=*truecolor*`
4. Otherwise, 256-color quantization

Anvil does not default to RGB when the answer is unknown:

- SSH forwards `TERM` but not `COLORTERM` by default; missing `COLORTERM` is
  not evidence that the terminal lacks truecolor.
- `TERM=xterm-256color` explicitly declares 256 colors. Ignoring it would
  overstate capabilities for real 256-color terminals, serial consoles, and
  some `screen`/tmux configurations.
- tmux and screen may sit between Anvil and the real terminal; truecolor needs
  their terminal features/overrides, not just the outer terminal's support.
- `TERM=dumb`, CI logs, terminal recorders, and non-interactive streams may not
  support RGB SGR sequences.
- There is no reliable, side-effect-free terminal query for truecolor.
- `btop` looking correct is not proof of automatic detection; it has explicit
  truecolor configuration and can force the profile.
- `NO_COLOR` is a CLI-output convention, not a full-screen TUI color mode;
  Anvil does not use it to disable theme colors.

See [OpenSSH SendEnv](https://man.openbsd.org/ssh_config#SendEnv),
[AcceptEnv](https://man.openbsd.org/sshd_config#AcceptEnv),
[tmux FAQ](https://github.com/tmux/tmux/wiki/FAQ), and
[NO_COLOR](https://no-color.org/).

## Control socket runtime directory

The workspace control socket normally lives under
`$XDG_RUNTIME_DIR/anvil/`. SSH sessions and root shells may not provide
`XDG_RUNTIME_DIR`; Anvil falls back to:

```text
~/.local/state/anvil/runtime/anvil/
```

and creates the private socket directory with mode `0700`. `anvil-ctl` uses the
same resolution, so both sides still meet without exporting
`XDG_RUNTIME_DIR` manually.



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
| Fedora 44 | 2.43 |

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

## WSL2

Status: Validated 2026-11-25 on Arch WSL2, kernel
`6.18.58.1-microsoft-standard-WSL2`, glibc 2.44, with systemd enabled: native
build, `anvil doctor --profile installed`, and the gateway smoke below (open,
edit, save, quit). The kernel-capability notes also cover the 5.15 and 6.6 WSL
kernel lines.

Anvil has no WSL-specific code path and no native Windows build. WSL2 runs a
real Linux kernel, so the normal Linux rules apply. WSL1 does not: it has no
Landlock, seccomp, or cgroup v2, and `anvil doctor` reports errors rather than
degrading.

### Kernel capabilities

Microsoft's `WSL2-Linux-Kernel` configuration enables what Anvil needs on the
5.15, 6.6, and current kernel lines:

- `CONFIG_SECURITY_LANDLOCK=y`
- `CONFIG_SECCOMP=y`, `CONFIG_SECCOMP_FILTER=y`
- `CONFIG_CGROUPS=y`, `CONFIG_CGROUP_PIDS=y`, `CONFIG_MEMCG=y`

The Landlock ABI follows the kernel: 1 on 5.15, roughly 3-4 on 6.6, and 7 on
the validated 6.18 host. Plugin hosts require ABI 1 and enable newer features
conditionally, so a newer kernel only widens the rule set. A custom kernel
selected through `.wslconfig` (`kernel=`) may have these options removed;
confirm with `anvil doctor`.

### systemd and the plugin cgroup

WSL2 boots without systemd by default. Two consequences:

- `XDG_RUNTIME_DIR` is unset and `/run/user/<uid>` does not exist. The control
  socket falls back to `~/.local/state/anvil/runtime/anvil/`, which keeps
  `anvil` and `anvil-ctl` working.
- Plugin hosts need a writable delegated cgroup v2 subtree and fail closed
  without one (`no writable delegated cgroup v2 root`).

Enable systemd in `/etc/wsl.conf`:

```ini
[boot]
systemd=true
```

then run `wsl --shutdown` from Windows. Interactive login sessions then live
under `user.slice`, and the nearest writable ancestor is the user manager's
cgroup, so delegation detection succeeds and `runtime-dir` resolves under
`/run/user/<uid>`.

Processes started outside systemd stay in the `/non-systemd` root cgroup,
which is not writable; the ancestor walk then reaches `/sys/fs/cgroup` and
fails even with systemd enabled. WSL interop daemons and some launchers start
editors this way, so check `cat /proc/self/cgroup` and use the escape hatch
below when it reports `0::/non-systemd`.

If systemd cannot be enabled, create a delegated root after each WSL start and
export the escape hatch:

```bash
sudo mkdir -p /sys/fs/cgroup/anvil-exec
sudo chown "$USER" /sys/fs/cgroup/anvil-exec
export ANVIL_CGROUP_ROOT=/sys/fs/cgroup/anvil-exec
```

`/sys/fs/cgroup` is rebuilt on every WSL start, so the directory does not
survive a restart.

### Terminal and truecolor

Windows Terminal is 24-bit capable but does not reliably export `COLORTERM`
into the distribution. On the validated host `TERM` was `xterm-256color` with
`COLORTERM` unset, so detection fell back to 256 colors and `anvil doctor`
warned. Set `ANVIL_TRUECOLOR=1` (or pass `--truecolor=always`); inside tmux
also set `terminal-features` for RGB.

### Clipboard

Anvil uses `wl-copy`/`wl-paste` when `WAYLAND_DISPLAY` is set and `xclip`
otherwise. OSC 52 is a copy-only fallback that requires `ANVIL_OSC52=1`; paste
has no OSC 52 path. Neither backend is installed by default: the validated host
had WSLg's `WAYLAND_DISPLAY=wayland-0` but no `wl-clipboard`.

- WSLg sets `WAYLAND_DISPLAY`; installing `wl-clipboard`
  (`pacman -S wl-clipboard` on Arch WSL, `apt install wl-clipboard` on Ubuntu
  WSL) gives copy and paste through the Windows clipboard.
- `ANVIL_OSC52=1` covers copy only, and only in terminals that accept OSC 52.
- A `win32yank` wrapper for `wl-copy`/`wl-paste` is an alternative.

### Filesystem placement

Keep workspaces on the Linux filesystem (`~/src`, reached from Windows as
`\\wsl$`), not under `/mnt/c`. DrvFs/9p is much slower for `rg` and `git`
(fuzzy open, gutter diffs), does not carry Linux permission bits without
metadata enabled, and has different case and mtime semantics. Build there too,
never in `/mnt/c`.

### Windows executables on PATH

WSL appends the Windows `PATH` by default. If a Linux tool is missing, `git`,
`rg`, or a language server can resolve to a `*.exe`, which then receives Linux
paths it cannot open; gutter diffs and LSP silently fail. Check with:

```bash
command -v git rg rust-analyzer | xargs -r file
```

Install the Linux versions and, if needed, pin `ANVIL_GIT` and `ANVIL_RG`.

### Verification

```bash
ps -p 1 -o comm=                          # systemd or init
systemctl --user is-system-running
echo "$XDG_RUNTIME_DIR"; ls -ld "/run/user/$(id -u)"
cat /proc/self/cgroup; mount | grep cgroup
anvil doctor --profile installed
```

The gateway smoke recipe below still applies; run it inside `tmux` or another
pseudo-terminal.

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

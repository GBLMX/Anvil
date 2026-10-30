# ADR-0005: No Shell, No Pipeline, Per-Tool Argument Policy

- Status: Proposed; exec runner, positive seccomp allowlist, per-tool execute, argv policy, process supervisor, symlink-race test, and kernel tests implemented; gateway integration pending
- Date: 2026-10-30

## Context

The whitepaper proposed `sys.exec` with argv arrays and "structured pipeline
AST", and an executable allowlist of `rg` and `fd`.

Argv prevents shell string injection, but it does not make allowlisted tools
safe:

- `rg --pre <command>` runs an arbitrary command.
- `fd --exec <command>` runs arbitrary commands.
- `RIPGREP_CONFIG_PATH`, `FD_*`, `PATH`, `LD_PRELOAD`, `HOME`, and locale
  variables change behavior.
- `exec.CommandContext` kills the direct child, not necessarily its descendants.
- A writable `PATH` or symlink enables executable substitution.

## Decision

1. `sys.exec` accepts an argv array and an explicit working directory.
2. No shell, no shell string, no shell pipeline AST in v1. A plugin that needs a
   pipeline issues multiple calls and composes the data itself.
3. The executable is resolved to a fixed path, opened as a file descriptor, and
   executed without a `PATH` lookup. Prefer `execveat`/`fexecve`.
4. `env_clear()` is mandatory. The gateway adds only a fixed minimal environment.
5. Every allowed executable has its own argument policy. The policy rejects
   flags that execute other commands, load arbitrary configuration, or spawn
   subprocesses.
6. v1 allowlist is `rg` and `fd`, with at least these rejections:
   - `rg --pre`, `--pre-glob`, `--config-path`;
   - `fd --exec`, `--exec-batch`, `--batch-size` combinations that execute;
   - any unknown flag.
7. The child runs in a new process group. Timeout, cancellation, and reload kill
   the group, wait for it, and reap descendants.
8. The child is never spawned directly by the unsandboxed gateway. The gateway
   invokes a small `anvil-exec-runner`, which applies Landlock, seccomp, and
   `no_new_privs` before executing the tool. The runner sees a read-only
   workspace and read-only system runtime paths; writes, network, ptrace,
   namespace creation, privilege changes, and anonymous executable mappings
   are denied. Runtime directories do not grant execute; only the selected tool
   and its ELF interpreter are executable.
9. Seccomp is a positive allowlist, not a denylist. Every syscall not explicitly
   allowed returns `EPERM`. `clone` is allowed only with `CLONE_THREAD` and no
   namespace flag; `clone3` returns `ENOSYS` so glibc falls back to the
   inspectable `clone`; file-backed executable mappings remain available for
   ld.so, while anonymous `PROT_EXEC` mmap and `mprotect(PROT_EXEC)` are
   denied.
   Landlock and seccomp state is per-thread, so the runner locks the applying
   OS thread and execs on that same goroutine.
10. `openat2` validation in the gateway is defense in depth, not a substitute for
    child confinement: `rg`/`fd` reopen path strings themselves. Landlock must
    deny an escape even if a symlink is swapped after argument validation.
11. Limits: max runtime, max stdout+stderr bytes, max process count, max file
    descriptors. `Run` kills the whole process group; the runner marks all
    descriptors above stderr close-on-exec before exec. When the gateway
    supplies `ANVIL_CGROUP_PATH`, the runner sets `pids.max=64`, moves itself
    into that cgroup, and raises its soft `RLIMIT_NPROC` to the hard limit. The
    effective bound is then `min(pids.max, RLIMIT_NPROC hard)`. The runner writes
    `cgroup.procs` before its final execve and does not fork afterward, so the
    tool inherits the cgroup. The gateway creates the cgroup before launch,
    removes it after wait, and calls `CleanupStaleCgroups` to remove empty
    cgroups older than one hour at startup. Otherwise `RLIMIT_NPROC` reserves
    64 tasks above the current user task count as a coarse fallback.
12. `sys.exec` is unavailable when the runner, policy, or sandbox setup is
    incomplete. Deny by default.

## Tool argument policy

A policy is not a denylist of strings. It is a typed parser for each tool:

```text
rg:
  allowed: pattern, paths, -n, --json, --glob, --type, -i, -F, -S
  denied:  --pre, --pre-glob, --config-path, any unknown flag
  paths:   resolved through openat2 under the workspace root

fd:
  allowed: pattern, paths, --type, --hidden, --max-depth, --json
  denied:  --exec, --exec-batch, any unknown flag
```

If a plugin needs a tool that cannot be safely parsed, expose a higher-level
Anvil API instead of raw `exec`.

## Child-process handling

- `Setpgid`/process group per call.
- `context.WithTimeout` plus explicit group kill.
- `Wait` after kill; no zombie.
- `Pdeathsig` where available.
- Gateway is a subreaper/owns the child lifecycle for its session.
- The gateway invokes `anvil-exec-runner`; the runner applies Landlock/seccomp
  and `no_new_privs` before `execveat`.
- `Run` uses `Setpgid`, `Pdeathsig`, explicit group SIGKILL, `WaitDelay`, and a
  bounded shared stdout/stderr budget; truncation and timeout are explicit.
- `Run` requires an absolute executable path and defaults to an empty
  environment, so `PATH`, `LD_PRELOAD`, `RIPGREP_CONFIG_PATH`, and locale
  variables cannot be inherited accidentally.

## Why `openat2` is not enough

`openat2` protects the gateway's own opens. It does not constrain a child
process that receives a path string and reopens it. A workspace symlink can be
swapped after validation or can point outside the workspace. The exec child must
therefore inherit a kernel sandbox that denies access outside its allowed
workspace/system roots, regardless of the path it chooses.

If this cannot be implemented, remove `sys.exec` from v1. Do not ship a
process-group-only exec policy and call it a security boundary.


## Consequences

Positive:

- Removes the obvious shell-injection class.
- Prevents allowlisted tools from becoming arbitrary command launchers.
- Makes cancellation and orphan cleanup testable.
- Keeps the v1 API small.

Negative:

- Some legitimate tool usage is rejected until a typed policy exists.
- Per-tool parsers need maintenance.
- Pipelines must be composed by plugins or added later as a separate ADR.

## Alternatives considered

### Shell strings (rejected)

Injection risk and quoting ambiguity.

### Structured pipeline AST (rejected for v1)

It recreates a shell inside the gateway. The attack surface is not justified by
ten API methods.

### Executable allowlist only (rejected)

`rg --pre` and `fd --exec` demonstrate why the executable name is insufficient.

### No exec API (fallback)

If per-tool policy cannot be implemented safely, ship `sys.search` only. This is
preferable to an unsafe generic exec.

## Tests

- `rg --pre /bin/sh` rejected.
- `fd --exec id` rejected.
- `RIPGREP_CONFIG_PATH`, `LD_PRELOAD`, `PATH` injection rejected.
- workspace symlink to an outside file and workspace writes are denied by
  Landlock.
- a syscall outside the allowlist returns `EPERM`; `clone3` returns `ENOSYS`;
  anonymous `PROT_EXEC` mappings and `mprotect(PROT_EXEC)` are denied.
- `rg` and `fd` start successfully under the allowlist.
- timeout kills descendants; output cap is enforced.
- unknown flag rejected.
- working directory outside workspace rejected.
- symlinked executable rejected or resolved to an approved fd.
- a concurrent symlink-swap race cannot read outside the workspace.
- non-allowlisted binaries, including via `ld.so`, cannot be exec'd.
- the tool and its ELF interpreter start under per-tool execute rules.

# ADR-0005: No Shell, No Pipeline, Per-Tool Argument Policy

- Status: Accepted
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
8. Limits: max runtime, max stdout+stderr bytes, max process count, max file
   descriptors.
9. `sys.exec` is unavailable when the sandbox/policy is incomplete. Deny by
   default.

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
- Output is streamed into bounded buffers; truncation is explicit.

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
nine API methods.

### Executable allowlist only (rejected)

`rg --pre` and `fd --exec` demonstrate why the executable name is insufficient.

### No exec API (fallback)

If per-tool policy cannot be implemented safely, ship `sys.search` only. This is
preferable to an unsafe generic exec.

## Tests

- `rg --pre /bin/sh` rejected.
- `fd --exec id` rejected.
- `RIPGREP_CONFIG_PATH`, `LD_PRELOAD`, `PATH` injection rejected.
- timeout kills descendants.
- output cap enforced.
- unknown flag rejected.
- working directory outside workspace rejected.
- symlinked executable rejected or resolved to an approved fd.

# ADR-0001: Untrusted Plugins Run in a Separate Host Process

- Status: Accepted
- Date: 2026-10-30
- Supersedes: whitepaper assumption that Lua/QuickJS run as OS threads inside
  the Rust editor

## Context

Anvil's security claim requires plugins to be untrusted. Lua and QuickJS are
C/C++ runtimes. Running them in the editor process means a plugin bug, native
module, or VM memory-safety issue can compromise the editor and every workspace
file it can access.

A Go policy gateway cannot contain that risk: the plugin can call the language
standard library, a native binding, or a syscall without making an RPC request.

## Decision

1. Plugin runtimes never execute in the editor process.
2. Each plugin, or an explicitly declared trust group, runs in a separate
   plugin-host process.
3. The plugin host applies:
   - Landlock filesystem rules (and network rules where the kernel supports
     them);
   - seccomp syscall allowlist;
   - `no_new_privs`;
   - a fixed working directory and minimal environment;
   - memory, CPU, file-descriptor, and child-process limits.
4. The Lua/QuickJS environment exposes only the Anvil capability table. It
   removes `io`, `package`, `debug` escape surfaces, `os.execute`, `loadfile`,
   `dofile`, and native module loading.
5. The plugin host has no ambient filesystem or network authority. `sys.*`
   requests go to the gateway with a per-plugin capability token.
6. If sandbox setup fails, the plugin does not start. There is no silent fallback
   to trusted mode.
7. The Go gateway is a policy enforcement point, not the isolation boundary.

## Identity

- The editor creates one host process and one token per plugin.
- The editor registers the token hash, plugin id, workspace root, and grants
  with the gateway before the host starts.
- The gateway uses `SO_PEERCRED` plus the token to identify the host.
- One host serving multiple plugins is allowed only for an explicit trust group;
  the default is one host per plugin.

## Consequences

Positive:

- A plugin memory-safety bug is contained by the OS process and kernel sandbox.
- Reload can kill and recreate the whole runtime safely.
- The gateway sees a small, typed request surface.
- Per-plugin identity is enforceable.

Negative:

- Process startup and memory cost per plugin.
- Hot reload must handle process teardown, not just VM drop.
- Lua/QuickJS host language bindings need careful capability stripping.
- Kernel baseline rises to 5.13 for Landlock-backed untrusted plugins.

## Alternatives considered

### In-process Lua/QuickJS (rejected)

Fastest startup, but not a security boundary. It cannot support an untrusted
plugin claim.

### Wasm/WASI plugin host (deferred)

Strong memory isolation and a mature embedding model, as demonstrated by Zed's
Wasm extensions. Deferred because Lua/TS compatibility and host API design need a
separate ADR. The process boundary does not prevent adding a Wasm host later.

### Luau instead of Lua 5.4 (selected by ADR-0006)

Luau removes dangerous standard libraries and is designed for untrusted code.
ADR-0006 selects it for the v1 Lua host through `mlua`'s `luau` feature. It
still runs in the isolated host process; it does not replace the process
boundary.

## Phase 0 proof

The ADR is not considered proven until a harness demonstrates:

1. A malicious Lua plugin cannot read a file outside its workspace.
2. `io.open`, `os.execute`, `package.loadlib`, and native module loading are
   unavailable.
3. A failed Landlock/seccomp setup prevents the plugin from starting.
4. Killing the host removes its child processes and sockets.
5. A second plugin cannot use the first plugin's token.

## References

- [Landlock](https://docs.kernel.org/userspace-api/landlock.html)
- [seccomp](https://docs.kernel.org/userspace-api/seccomp_filter.html)
- [no_new_privs](https://docs.kernel.org/userspace-api/no_new_privs.html)
- [Luau sandbox](https://luau.org/sandbox/)
- [Zed extension architecture](https://zed.dev/docs/extensions/developing-extensions)

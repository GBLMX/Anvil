# Anvil Threat Model

Status: Phase 0 working document. Every invariant here needs an executable test
before Phase 1.

## 1. Scope

This model covers:

- the Rust editor process;
- the per-session Go policy gateway;
- untrusted plugin-host processes running Lua/QuickJS;
- plugin manifests, updates, and permission grants;
- file, network, and child-process access mediated by `sys.*`;
- UDS transport between the processes.

It does not cover kernel compromise, a malicious editor binary, a compromised
terminal emulator, physical access, or supply-chain compromise of the compiler
and package registries. Those are separate risks.

## 2. Assets

| Asset | Why it matters |
|---|---|
| Workspace files | Source code, secrets, configuration |
| Files outside the workspace | SSH keys, cloud credentials, browser profiles |
| User terminal | Input injection, clipboard, escape sequences |
| Network identity | Cookies, tokens, source IP, DNS metadata |
| Editor memory | Unsaved buffers and secrets |
| Child processes | Shell, language servers, `rg`, `fd` |
| User attention | Malicious notifications, fake permission prompts |
| Plugin grants | Capability durability and update integrity |

## 3. Trust boundaries

```text
trusted: user, terminal, editor
────────────────────────────────────────
untrusted: plugin code, plugin host after compromise
────────────────────────────────────────
policy: Go gateway
────────────────────────────────────────
external: filesystem, network, child processes
```

The important correction to the original whitepaper:

> The Go gateway is a policy enforcement point, not the plugin sandbox.
> The plugin-host process boundary and its kernel sandbox are the isolation
> boundary.

A plugin running in the editor process cannot be contained by a policy gateway:
it can call the Lua/QuickJS standard library, native C functions, or syscalls
directly.

## 4. Adversaries

1. **Malicious plugin author**: ships a plugin that requests narrow permissions
   and then attempts to escape them.
2. **Compromised dependency**: a Lua/QuickJS native dependency or Rust crate
   contains a memory-safety bug.
3. **Malicious project**: a repository contains a crafted `Anvil.toml`, symlinks,
   or a workspace root designed to confuse path resolution.
4. **Network attacker**: DNS rebinding, redirect, or a malicious endpoint.
5. **Local process**: another process on the same machine tries to connect to a
   gateway/editor socket.
6. **Resource attacker**: a plugin tries to exhaust memory, file descriptors,
   CPU, or child processes.

## 5. Threats and required mitigations

### T1. Path traversal / symlink escape / TOCTOU

**Attack:** `../../.ssh/id_ed25519`, absolute paths, symlink swap between policy
check and read, magic links under `/proc`.

**Required:**

- One workspace-root `O_PATH` fd per session.
- `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_MAGICLINKS`.
- `RESOLVE_NO_SYMLINKS` is a configurable hardening option; when not enabled,
  symlink targets must still remain beneath the root.
- Retry `EAGAIN` instead of treating it as a file error.
- Open once, inspect the resulting fd, read from that fd. No second resolution.
- Reject absolute paths, empty components, and non-UTF-8 path policies before
  the syscall.
- Test workspace root itself being a symlink.

### T2. SSRF / DNS rebinding / redirect

**Attack:** fetch `https://allowed.example` which resolves to `127.0.0.1`,
`169.254.169.254`, a private address, or changes its answer between validation
and connection.

**Required:**

- Exact host allowlist; no wildcard in v1.
- HTTPS and port 443 only.
- `GET`/`HEAD` by default; `POST` requires `network:post`.
- Resolve all A and AAAA answers; reject if any answer is non-global.
- Connect to a validated address while preserving Host, TLS SNI, and certificate
  verification.
- Disable redirects. If enabled later, validate every hop.
- Disable proxy environment variables.
- Cap body after decompression, header size, total time, and concurrency.
- Reject userinfo, IDN ambiguity, trailing-dot variants, IPv4-mapped IPv6,
  CGNAT, link-local, multicast, unspecified, NAT64, 6to4, and Teredo.
- Treat parser disagreement as rejection.

Reference: [OWASP SSRF Prevention Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html).

### T3. Command injection through `sys.exec`

**Attack:** use an allowlisted tool's own execution features, for example
`rg --pre <command>` or `fd --exec <command>`, or inject `PATH`, `LD_PRELOAD`,
`RIPGREP_CONFIG_PATH`, or `FD_*`.

**Required:**

- No shell, no shell string, no shell pipeline AST in v1.
- Argv array only.
- Per-executable argument policy; reject flags that spawn commands or load
  arbitrary configuration.
- `env_clear()` plus a fixed minimal environment.
- Fixed executable path opened as a file descriptor; no `PATH` lookup.
- Child in its own process group; timeout/cancel kills the whole tree.
- The gateway must not spawn `rg`/`fd` directly. It invokes
  `anvil-exec-runner`, which applies Landlock (workspace + system runtime
  read-only), seccomp (no network/ptrace/privilege changes), and
  `no_new_privs` before `execveat`.
- Landlock must deny a symlink swapped after gateway validation; `openat2` is
  only defense in depth because the child reopens paths itself.
- A symlink-race test must prove that a workspace link pointing outside cannot
  be read by `rg` or `fd`.
- Output and process count limits.

### T4. Plugin escape

**Attack:** plugin calls `io.open`, `os.execute`, `package.loadlib`, a native C
module, a QuickJS native binding, or directly invokes a syscall.

**Required:**

- Plugin host is a separate process.
- Lua/QuickJS standard libraries are reduced to an explicit allowlist; no
  `io`, `os.execute`, `package.loadlib`, `debug` escape surface, or bytecode
  loading.
- No native plugin modules in v1.
- Landlock (filesystem, and network where available), seccomp, `no_new_privs`.
- Plugin host starts with one plugin's capabilities only.
- If sandbox setup fails, do not start the plugin.

### T5. IPC spoofing / confused deputy

**Attack:** another local process connects to the gateway or editor socket and
pretends to be a plugin.

**Required:**

- Per-session runtime directory with `0700`; socket `0600`.
- `SO_PEERCRED` to identify the peer process.
- Per-plugin capability token issued by the editor and registered with the
  gateway.
- Gateway checks token, plugin id, workspace, and requested operation.
- One plugin per host process so a compromised host cannot impersonate a sibling.

### T6. Resource exhaustion

**Attack:** infinite loop, allocation flood, thousands of child processes,
unbounded response bodies, file descriptor exhaustion.

**Required:**

- Lua/QuickJS instruction and memory limits.
- Wall-clock deadline and cancellation.
- Per-plugin process, memory, fd, and output quotas.
- Gateway body/header/request limits.
- Bounded queues and backpressure; reject rather than buffer without limit.

### T7. Hot reload / cancellation failures

**Attack:** reload a plugin while it holds a file, socket, or child process;
leave a zombie or a process writing files after revocation.

**Required:**

- Reload is: stop new requests, cancel in-flight ids, kill process group, wait
  for host exit, revoke token, start a new host, issue a new token.
- No `Drop`-only cleanup for external resources.
- The gateway records `cancelled_by_reload`.
- Leak test: repeated reloads with RSS, fd, and child-process assertions.

### T8. Manifest privilege escalation

**Attack:** a plugin update adds permissions, changes its own manifest, or
reuses an old approval.

**Required:**

- Manifest permissions are requests.
- User grants per workspace; grants are keyed by plugin id and manifest hash.
- New permissions require new consent.
- Signed manifest or at minimum a lockfile with hash.
- Display requested file roots, hosts, and executables before approval.

## 6. Security invariants

1. No plugin can read/write outside the workspace through `sys.*`.
2. No plugin can reach a non-granted host, port, or method.
3. No plugin can execute a shell or a non-granted executable/argument pattern.
4. No plugin can outlive cancellation or leave a child process behind.
5. No plugin can bypass the host sandbox by using a language standard library.
6. No local process can impersonate a plugin without its capability token.
7. A failed sandbox setup fails closed.
8. A stale snapshot cannot mutate the editor buffer.

## 7. Test matrix

| Area | Tests |
|---|---|
| Paths | `..`, absolute, symlink escape, magic link, root symlink, `EAGAIN` race, hard link |
| Network | private IPv4/IPv6, metadata IP, CGNAT, DNS rebinding, redirect, gzip bomb, wrong SNI, proxy env |
| Exec | shell string, `--pre`, `--exec`, env injection, PATH swap, symlink race, runner sandbox failure, timeout, descendant kill |
| Plugin host | removed stdlib functions, native module load, memory limit, CPU interrupt |
| IPC | wrong token, replayed token, wrong workspace, concurrent cancel |
| Reload | 1000 reloads, fd/RSS/child checks |

## 8. Residual risks after Phase 0

Even with the design above:

- Landlock is unavailable before Linux 5.13; untrusted plugins must be disabled
  or the baseline raised.
- Landlock authorizes objects, not names; hard links can intentionally share
  access.
- Lua/QuickJS are C/C++ runtimes; a memory-safety bug in the VM can break the
  host. Kernel sandboxing limits the impact but does not make the host trusted.
- HTTPS certificate validation ultimately trusts the system CA store.
- A compromised editor binary can bypass every policy in this document.

These are accepted only if documented in the release notes and covered by the
Phase 0 risk review.

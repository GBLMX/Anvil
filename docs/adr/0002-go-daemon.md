# ADR-0002: Go Gateway Is a Per-Session Policy Gateway

- Status: Accepted
- Date: 2026-10-30
- Related: ADR-0001

## Context

The original whitepaper described a "Go daemon" as the control plane for plugin
permissions and external tools. That wording left two incompatible readings:

1. Go owns the plugin runtimes and is the security sandbox.
2. Go is a policy gateway in front of OS operations.

Only reading 2 is compatible with the plugin-host decision. The gateway is not
an isolation layer: a plugin that runs in the editor process could bypass it.

There is also a lifecycle question. A global daemon shared by all editor
sessions would turn one workspace's plugin compromise into a cross-workspace
risk.

## Decision

The Go component is:

- named `anvil-gateway`;
- started per editor session;
- unprivileged;
- reachable only through a session-private Unix domain socket;
- responsible for policy and I/O for `sys.read_file`, `sys.fetch`, and
  `sys.exec`;
- not a plugin runtime;
- not a global service;
- not the plugin sandbox.

The plugin host remains a separate process and owns Lua/QuickJS plus the kernel
sandbox.

## Responsibilities

The gateway owns:

- workspace-root file descriptor and `openat2` resolution;
- DNS resolution and address validation;
- HTTPS fetch, redirect policy, body limits;
- executable allowlist and argument policy;
- child process groups, timeouts, and reaping;
- request deadlines and cancellation;
- capability grant lookup.

The gateway does not:

- execute Lua/QuickJS;
- store editor state;
- expose an editor API;
- grant permissions on its own;
- survive as a shared daemon across workspaces.

## Lifecycle

```text
editor starts
  -> create runtime directory (0700)
  -> start gateway
  -> create control socket (0600)
  -> register plugin grants
  -> plugin host connects to gateway
  -> editor exits / crashes
  -> gateway sees control EOF
  -> gateway kills children and exits
```

If the gateway crashes, plugin `sys.*` calls fail closed. The editor remains
usable and may restart the gateway. If the editor crashes, the gateway must not
continue serving orphaned plugin hosts.

## Why Go

- `net/http` with a custom dialer for DNS pinning and TLS SNI preservation.
- `os/exec` with process groups and `CommandContext`.
- `golang.org/x/sys/unix` for `openat2`, `SO_PEERCRED`, and process controls.
- A memory-safe language for URL, DNS, and HTTP parsing.

Go is not chosen to host Lua/QuickJS. cgo runtimes belong in the plugin host,
where the sandbox is explicit.

## Consequences

Positive:

- One small policy process per session.
- No global cross-workspace authority.
- Go's standard library covers URL, DNS, TLS, and process management.
- The editor can survive a gateway restart.

Negative:

- A second toolchain and release artifact.
- Two process boundaries to test.
- Gateway bugs are security-relevant and must be fuzzed.
- IPC serialization and version skew must be managed.

## Alternatives considered

### Rust gateway (deferred)

One language would simplify builds and sharing types. Rejected for v1 because
the Go team owns the gateway and Go's HTTP/DNS/process libraries fit the task.
This can be revisited after the security tests exist; the IPC contract makes the
implementation replaceable.

### Global gateway (rejected)

A cross-session daemon reduces startup cost but creates a shared blast radius and
makes workspace permissions harder to reason about.

### Gateway as plugin host (rejected)

Putting Lua/QuickJS inside Go would require cgo bindings and would blur the
sandbox boundary. Keep plugin execution in the dedicated host process.

## Failure policy

- No grant: reject.
- Invalid token: reject and close.
- Malformed frame: close and disable plugin.
- Cancelled request: kill its children; never return success.
- Gateway unavailable: plugin disabled; editor remains usable.

## References

- [openat2(2)](https://man7.org/linux/man-pages/man2/openat2.2.html)
- [OWASP SSRF Prevention](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html)
- [Go x/sys/unix Openat2](https://pkg.go.dev/golang.org/x/sys/unix#Openat2)

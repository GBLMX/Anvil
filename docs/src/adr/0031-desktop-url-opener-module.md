# ADR-0031: Desktop URL Opener Module

- Status: Accepted for implementation; initial URL-opener slice landed
- Date: 2026-11-13
- Related: [ADR-0005](0005-exec-policy.md), [ADR-0028](0028-module-contract.md),
  [ADR-0030](0030-workspace-index-module.md)

## Context

Anvil has no command for handing an HTTP(S) URL to the user's desktop. Adding
one cannot use the existing `sys.exec` path unchanged: its fixed tool policy and
exec-runner sandbox are built for workspace commands, while a browser launch is
a desktop handoff. Adding `xdg-open` to the generic `sys.exec` allowlist would
widen that policy and still leave browser/DBus requirements to the sandbox.

The module plan lists desktop/XDG integration as a `0.7.4` candidate, with the
constraint that it must not become a privileged escape hatch. This ADR defines
the narrow exception needed for URL opening.

## Decision

### 1. Module and feature

- Crate: `anvil-mod-desktop`.
- Module id: `official.desktop`.
- Cargo feature: `official-desktop`.
- Standard edition: opt-in for the first slice.
- Minimal fixture: absent.
- The module contributes no arbitrary command runner. It exposes one typed
  service for explicit user-initiated URL opening.

### 2. Service contract

`anvil-module` adds one additive service slot:

```rust
pub trait DesktopService: Send + Sync {
    fn open_url(&self, url: &str) -> Result<(), DesktopError>;
}

pub enum DesktopError {
    InvalidUrl,
    Unavailable(String),
    Spawn(String),
}
```

The service accepts no opener path, command name, environment override, file
path, or extra arguments. It returns no browser output or exit status.

The slot is additive under the contract-v1 policy. It does not expose
`anvil-tui` internals and is not callable by runtime Lua/TS plugins.

### 3. URL policy

The module accepts only:

- absolute `http://` or `https://` URLs;
- at most 4096 bytes;
- no ASCII control characters or whitespace;
- a non-empty host;
- no leading `-` (so the value cannot be interpreted as an option).

The URL is passed as one argument vector element. The module never invokes a
shell, never uses string concatenation for command construction, and never
accepts `file:`, `mailto:`, custom application schemes, or relative paths in
this slice.

### 4. Launcher policy

The first implementation supports exactly one executable: `xdg-open`, resolved
from `PATH`.

- No shell.
- No user-configured opener.
- No fallback to `gio open`, `sensible-browser`, or arbitrary commands in v1.
- No `file://` or filesystem path opening in v1.
- The process is spawned detached, reaped by a waiter thread, and never waited
  on by the TUI thread.
- At most four opener processes may be in flight; excess requests fail with
  `Unavailable` instead of queueing without a bound.
- The status line reports success/failure without echoing the full URL, because
  URLs can contain credentials or query secrets.

This is a deliberate, single-purpose exception to ADR-0028 section 6. It does
not create a general process API. Filesystem, Git, network, and all other
process access remain gateway-bound.

### 5. TUI integration

The first consumer is an explicit command:

```text
:open-url <http(s)-url>
```

The command is available in command completion and dispatch only when the
distribution supplies `DesktopService`, matching the existing Git gating.
Without the service, it reports that the desktop module is not installed and
does not attempt a fallback opener. The shared remote-command policy marks it
admin-only, so `anvil-ctl`/MCP cannot trigger it.

No `gx` mapping, automatic URL detection, plugin-callable endpoint, or
"open any path" command is added in this slice. Those require a separate
decision and a clearer capability boundary.

### 6. Failure behavior

- Invalid URL: report `E: invalid URL`; do not spawn.
- `xdg-open` missing or not executable: report the module as unavailable; do
  not try another launcher.
- Spawn failure: report a bounded error; do not retry automatically.
- Child exits non-zero: the waiter reaps it and discards the status; the browser
  may have handed off successfully, so this is not treated as a security or
  editing failure.
- The editor remains usable while the opener runs.

### 7. Testing and gates

- Unit tests for scheme, host, size, control-character, leading-dash, and
  argument-boundary validation.
- A fake `xdg-open` on `PATH` proves the exact argument vector and that shell
  metacharacters are not interpreted.
- A no-service TUI test proves `:open-url` refuses cleanly.
- Feature-free, `official-desktop`, and all-module builds.
- Installed-binary smoke with a fake `xdg-open` that records its argv; no real
  browser is required in CI.
- No module dependency on `anvil-tui`.
- The generic `sys.exec` policy and exec-runner sandbox are unchanged.

## Consequences

- Anvil gains a narrow desktop handoff without opening a general command or
  filesystem escape hatch.
- `official.desktop` remains optional and can be excluded without affecting
  editing, LSP, Git, search, or the workspace index.
- The module is trusted in-process code; it is not a sandbox boundary. The
  fixed executable and argument policy are the control, not process isolation.
- URL opening is user-initiated only. No plugin, LSP server, or file content can
  trigger it.

## Non-goals

- Opening arbitrary files or directories from the TUI.
- A configurable external opener.
- Browser profile, download, or tab management.
- `file://`, `mailto:`, or custom schemes.
- A generic `DesktopService` command execution API.
- Changing gateway `sys.exec` policy.
- Replacing the clipboard adapter or other existing desktop helpers.

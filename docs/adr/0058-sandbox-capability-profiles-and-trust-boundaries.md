# ADR-0058: Sandbox Capability Profiles and Trust Boundaries

- Status: Proposed; profile selection and doctor capability report implemented 2026-11-20
- Version: 1.6.0
- Date: 2026-11-20
- Implementation note (2026-11-20): `anvil doctor --json` exposes the structured
  `sandbox` object (Landlock ABI/signal scope, seccomp, cgroup v2, plugin/exec
  status, LSP trust, control transport, deployment). `ANVIL_SANDBOX_PROFILE`
  selects `auto|strict|off` at startup; strict refuses degraded boundaries and
  off warns while the untrusted plugin boundary remains enforced. Gateway and
  doctor share the explicit/sibling/PATH runner lookup. Deployment smoke passed
  for an installed layout, local and systemd-service detection, an Arch
  container with Docker's default seccomp profile, bwrap with `/proc` and
  `/dev` replaced by empty tmpfs mounts, a real Debian 13 SSH host using a
  portable x86-64 build, and a Flatpak sandbox running the installed layout
  through the VLC app. The Flatpak run reported `deployment=flatpak`, installed
  profile, and strict plugin/exec boundaries. Snap package-level smoke is still
  blocked: `snapd` 2.78 is installed and active, but `snap debug confinement`
  reports `partial`, no snap is installed, and sudo authentication failed while
  installing `core24`. `SNAP` environment detection alone reports
  `deployment=snap`.
- Related: [ADR-0001](0001-plugin-isolation.md),
  [ADR-0002](0002-go-daemon.md),
  [ADR-0005](0005-exec-policy.md),
  [ADR-0012](0012-plugin-host-isolation.md),
  [ADR-0023](0023-internal-capability-closure.md),
  [ADR-0025](0025-lsp-and-language-breadth.md),
  [ADR-0049](0049-editor-control-plane.md)
- Supersedes:
- Superseded by:

## Context

Anvil has several independent enforcement points:

- the untrusted plugin host uses Landlock, seccomp, cgroup limits, and a token
  socket;
- `sys.exec` uses the gateway policy plus an optional `anvil-exec-runner`
  sandbox;
- language servers are trusted external tools and are deliberately not run in
  the plugin sandbox;
- `anvil-ctl` and `anvil-mcp` use a same-UID local Unix socket and
  `SO_PEERCRED`;
- the Go gateway enforces policy but is not itself a sandbox.

This is not literally one sandbox. It is one **deployment and trust model**:
Linux local desktop, one user, one workspace/session, trusted language servers.

That model is reasonable as a baseline, but it is currently implicit. The
system does not consistently report:

- which Landlock ABI is available;
- whether `LANDLOCK_SCOPE_SIGNAL` is available;
- whether seccomp is available and whether it is the primary boundary;
- whether user namespaces and cgroup v2 are usable;
- whether the exec runner was found;
- whether LSP is sandboxed or trusted;
- whether the process is inside Flatpak, Snap, a container, SSH, or a
  service.

The absence of an explicit model already produces observable drift. The Go
gateway is located through `PATH`, but `anvil-exec-runner` is only looked up as
a sibling of the gateway binary. If the gateway is found through `PATH` and
the runner is not its sibling, the gateway starts without `-allow-exec` and
`sys.exec` silently becomes unavailable. The control-plane smoke test fails at
its git-diff step in that condition.

`seccomp` alone is not a sandbox. The Linux kernel documentation explicitly
states that system-call filtering is a tool for sandbox developers, not a
complete sandbox. Landlock support is ABI-versioned and must be probed. Older
kernels lose newer restrictions, including signal scoping below ABI 6.

The supported surface is also Linux-only. Windows and macOS are deliberately
deferred, but their eventual isolation primitives are different:

- Windows: AppContainer, restricted tokens, and Job Objects;
- macOS: App Sandbox and code-signed entitlements.

Those are separate security models, not a few `cfg` branches.

## Decision

### 1. Sandbox state is a capability report, not a boolean

The runtime and `anvil doctor --json` expose a structured `sandbox` object.
The exact field set may grow, but it MUST identify:

```text
profile
plugin.landlock: present, abi, signal_scope
plugin.seccomp: available, mode
plugin.cgroup_v2
plugin.status: strict | degraded | disabled
exec.policy
exec.runner: found, path
exec.status: strict | disabled | degraded
lsp.sandboxed
lsp.trust: trusted_external
control.transport
control.peer
control.remote
deployment: local | flatpak | snap | container | service | ssh
```

A missing optional capability is never silently represented as “enabled”.
`doctor --json` is the machine-readable source for support tooling and tests.

### 2. Profiles are explicit

Supported profile values are:

```text
sandbox = auto | strict | off
```

The default is `auto`.

| Profile | Behavior |
|---|---|
| `auto` | Probe capabilities; use the strongest available boundary; fail closed for untrusted plugins; explicitly disable optional features that lack their boundary; report degraded state |
| `strict` | Require the full boundary for each enabled feature; fail startup or leave the feature disabled rather than falling back; do not claim LSP is sandboxed |
| `off` | Development escape hatch only; never default; never disables the untrusted-plugin boundary; requires an explicit warning in `doctor` and startup output |

`sandbox=off` is not a substitute for a missing capability. It is a separate,
documented development choice.

Profile selection is evaluated once at startup and is visible in `doctor`.
Changing it requires a restart.

### 3. Each boundary has one declared trust level

| Boundary | Trust | Required enforcement |
|---|---|---|
| Plugin host | Untrusted third-party code | Landlock filesystem rules, seccomp, no_new_privs, cgroup where available; fail closed |
| `sys.exec` | Trusted tool selected by the user, still sandboxed | Gateway policy plus `anvil-exec-runner`; no silent unsandboxed fallback |
| LSP | Trusted external tool, not third-party plugin code | Explicit `trusted_external` declaration; sandboxing is optional and must be reported if enabled |
| Control socket | Local same-UID client | Peer UID check, socket mode, bounded connections; local only |
| Gateway | Policy enforcement point | No direct workspace/filesystem bypass |

An untrusted plugin must never run merely because Landlock or seccomp is
missing. A trusted tool may run unsandboxed only when its trust level is
explicit and the selected profile permits it.

### 4. Capability degradation is explicit

At minimum:

- Landlock unavailable + seccomp available: plugin/exec status is
  `degraded`, not `strict`; the reduced boundary is reported.
- Landlock and seccomp unavailable: untrusted plugins do not start; exec does
  not start.
- exec runner missing: `sys.exec` is disabled with a visible reason; the
  gateway must not advertise exec, and `doctor` must warn or fail according to
  profile.
- LSP is unsandboxed unless a future implementation explicitly reports
  `lsp.sandboxed = true`; the UI and docs must not call it sandboxed.
- Host-sandbox or container constraints that prevent required paths, sockets,
  processes, or namespaces produce an explicit unsupported/degraded result.

The gateway and runner lookup must be layout-aware:

1. use an explicit configured path;
2. use the gateway's sibling `anvil-exec-runner`;
3. use a documented PATH lookup for the runner as well as the gateway;
4. report failure to find the runner instead of silently omitting
   `-allow-exec`.

### 5. Deployment profiles are separate from sandbox profiles

The following are distinct deployment targets, not one Linux mode:

- local desktop;
- Flatpak;
- Snap;
- generic container/CI;
- SSH remote terminal;
- systemd/headless service.

Flatpak and Snap already impose host isolation and may provide resources only
through portals. A nested Anvil sandbox must not assume host `/proc`,
`/dev/tty`, XDG paths, Unix socket paths, or process visibility.

Remote control, multi-user authentication, and a headless service are not
implemented by reusing the same-UID local socket. They require a separate ADR
covering transport authentication, authorization, replay protection, and
workspace selection.

### 6. Platform isolation stays in separate ADRs

Windows and macOS are not added as conditional branches to the Linux sandbox.
They require separate ADRs covering:

- Windows AppContainer/restricted token/Job Object model;
- macOS App Sandbox, entitlements, and code signing;
- how control sockets, file access, process launch, and updates map to each
  platform;
- what remains unsupported.

## Consequences

Positive:

- Users and support tooling can see what is actually sandboxed.
- Missing exec runners and degraded kernels stop being silent.
- Remote, container, and platform work cannot accidentally inherit a local
  trust assumption.
- `sandbox=off` remains auditable instead of becoming an invisible bypass.

Negative:

- Startup and `doctor` gain more capability checks and failure modes.
- Some environments will correctly report “unsupported” where they previously
  appeared to work.
- LSP remains a trusted unsandboxed boundary until a separate implementation
  changes it.
- Remote and cross-platform support stay explicitly out of scope.

## Verification

- Capability tests:
  - Landlock present/absent with ABI values;
  - seccomp present/absent;
  - `LANDLOCK_SCOPE_SIGNAL` at ABI 5 versus 6;
  - user namespace/cgroup availability;
  - Flatpak/Snap/container/SSH/service detection.
- Boundary tests:
  - plugin host does not start without an acceptable boundary;
  - exec is disabled and reported when the runner is missing;
  - runner discovery succeeds from an installed layout and from PATH;
  - LSP reports `trusted_external` and is never reported as sandboxed.
- Profile tests:
  - `auto` selects the strongest valid boundary;
  - `strict` fails or disables rather than degrading;
  - `off` still refuses untrusted plugins without a sandbox.
- `doctor --json` schema and CLI output tests.
- Host-environment smoke:
  - local, container, and installed layout;
  - SSH session where applicable;
  - no `/proc` or no `/dev/tty` where the host makes that testable.

## References

- Landlock userspace API:
  <https://docs.kernel.org/userspace-api/landlock.html> — ABI probing and
  per-ABI filesystem, network, and scope restrictions.
- seccomp BPF:
  <https://docs.kernel.org/userspace-api/seccomp_filter.html> — explicitly
  describes system-call filtering as a tool, not a sandbox.
- Flatpak sandbox permissions:
  <https://docs.flatpak.org/en/latest/sandbox-permissions.html> — host
  filesystem, network, device, and process restrictions; portals for mediated
  access.
- XDG Desktop Portal security model:
  <https://flatpak.github.io/xdg-desktop-portal/docs/security-model.html> —
  trusted desktop session versus untrusted sandboxed applications.
- Snap confinement:
  <https://snapcraft.io/docs/explanation/security/snap-confinement/> —
  strict/classic/devmode confinement levels.
- Windows AppContainer isolation:
  <https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation>
  — file, network, process, window, and credential isolation.
- Apple App Sandbox:
  <https://developer.apple.com/documentation/security/app-sandbox> —
  entitlement-based macOS sandbox model.
- OpenSSH `ForwardAgent`:
  <https://man.openbsd.org/ssh_config#ForwardAgent> — agent forwarding exposes
  the local agent to the remote host and must be treated as a trust decision.
- systemd execution environment:
  <https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html> —
  filesystem, namespace, device, and process sandboxing for services.

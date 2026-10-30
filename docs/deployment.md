# Deployment Model

Status: Phase 0.6. This document records the process and cgroup boundaries that
the editor package must provide.

## Gateway lifecycle

The gateway is **per editor session**, not a daemon and not systemd
socket-activated.

- The editor creates `$XDG_RUNTIME_DIR/anvil/<session-id>/` with mode `0700`.
- The editor starts one `anvil-gateway` process for that session.
- The gateway binds one Unix socket in that directory with mode `0600`.
- The editor owns the gateway process and its socket lifecycle.
- `Server.Close` removes the socket; a crash leaves only a stale socket and no
  usable authority.
- Starting a second plugin session starts a second gateway process. The cold
  start is one Go process; it is paid once per session before plugin handshake.

This follows ADR-0002: a global daemon would share authority across workspaces.
Systemd socket activation is not used because it would create a shared
cross-session endpoint.

## cgroup v2

`anvil-gateway -cgroup-root <dir>` enables a per-session cgroup:

1. Gateway calls `execrunner.CleanupStaleCgroups` for stale empty children.
2. Gateway creates a unique child under `<dir>`.
3. `sys.exec` passes `ANVIL_CGROUP_PATH` to `anvil-exec-runner`.
4. Runner writes `pids.max` and moves itself into the cgroup before exec.
5. Gateway removes the child on normal close; a failed removal is left for the
   next startup sweep.

The configured root must be writable by the gateway's user and have the `pids`
controller available. Choose one of these deployment options:

### Delegated systemd session

Run the editor/session under a systemd unit with pids delegation, then pass a
writable delegated subdirectory as `-cgroup-root`:

```ini
[Service]
Type=simple
ExecStart=/usr/bin/anvil
Delegate=pids
```

The editor must derive the delegated cgroup path from its own environment and
pass it explicitly. Do not point the gateway at a shared root unless the
workspace/session ownership model accepts that sharing.

### Root-created delegated root

For a fixed per-user root:

```bash
sudo mkdir -p /sys/fs/cgroup/anvil-exec
sudo chown "$USER" /sys/fs/cgroup/anvil-exec
```

Then start the gateway with:

```bash
anvil-gateway -cgroup-root /sys/fs/cgroup/anvil-exec ...
```

The parent cgroup must expose the `pids` controller; enabling
`cgroup.subtree_control` is a host setup responsibility. Do not run the
gateway as root merely to manage cgroups.

If `-cgroup-root` is omitted, `sys.exec` continues to work with the runner's
`RLIMIT_NPROC` fallback. That fallback is per-UID, not per-session, and can
interfere with other user processes.

## Token and permission bootstrap

Phase 0.6 still uses `-token-sha256` as a one-shot bootstrap verifier. The raw
token is sent only in `session.hello`; the first successful handshake consumes
it. Production deployment must replace this with editor control-socket
registration and inherited-fd token delivery before untrusted plugin hosts are
enabled.

`-allow-exec` and `-allow-fetch` are per-gateway grants. Because the gateway is
per-session, this is equivalent to a per-session grant in the current model.
Per-session permission negotiation remains a follow-up.

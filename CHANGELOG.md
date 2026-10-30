# Changelog

All notable changes to Anvil are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Project charter and Phase 0 security/performance baseline.
- Threat model covering path traversal, SSRF, plugin escape, IPC spoofing,
  resource exhaustion, cancellation, and manifest escalation.
- Architecture decision records for plugin isolation, the per-session Go
  gateway, lightweight IDL, text-model selection, exec policy, upstream
  dependencies, the JSON-RPC/token contract, and the selection-first
  interaction model.
- Repository governance files: security policy, contribution guide, code of
  conduct, editor configuration, license files, and CI skeleton.
- `anvil-ipc`: length-prefixed JSON-RPC framing with an 8 MiB limit, clean-EOF
  handling, truncation detection, I/O error preservation, Unix socket
  connections with `0600` socket permissions, `SO_PEERCRED` lookup, and
  validated `Request`/`Response`/`Meta`/`ErrorObject` envelopes.
- `go/internal/fsroot`: `openat2`-confined workspace reader with traversal,
  symlink-escape, regular-file, and size-limit tests.

### Security

- `sys.exec` is not considered safe until `anvil-exec-runner` applies
  Landlock/seccomp and a workspace symlink-race test passes.
- Untrusted plugins run outside the editor process; the Go gateway is a policy
  gateway, not the plugin sandbox.

### Known limitations

- No runnable editor yet.
- No stable plugin API or release.
- Landlock-backed untrusted plugins require Linux 5.13+.
- Smithy and Rope/piece-table decisions remain Phase 0 gates.

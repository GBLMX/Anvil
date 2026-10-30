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
  gateway, lightweight IDL, text-model selection, and exec policy.
- Repository governance files: security policy, contribution guide, code of
  conduct, editor configuration, license files, and CI skeleton.

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

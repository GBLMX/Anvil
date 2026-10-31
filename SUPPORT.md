# Support

Anvil is pre-alpha. There is no supported production release yet.

## What to use

- **Bug reports and feature discussion:** open a GitHub issue.
- **Security vulnerabilities:** do not open a public issue; follow
  [SECURITY.md](SECURITY.md).
- **Contribution questions:** read [CONTRIBUTING.md](CONTRIBUTING.md) and
  [GOVERNANCE.md](GOVERNANCE.md).
- **Architecture questions:** check `docs/architecture.md` and the ADRs before
  opening an issue.

## Supported versions

No stable version is supported yet. Until the first stable release:

- fixes target the current `main` branch;
- old `0.x` tags are historical snapshots, not supported branches;
- breaking changes may land without a deprecation window;
- Linux is the only supported platform for the sandboxed plugin path.

## Response expectations

Maintainers aim to triage reports, but this is a volunteer pre-alpha project.
There is no support SLA. Security reports follow the response targets in
`SECURITY.md`.

## Not supported

- Windows and macOS for v1;
- running untrusted plugins without a functioning Landlock/seccomp sandbox;
- bypassing the Go gateway with direct file/network/process access;
- performance or security claims not backed by a reproducible check in this
  repository.

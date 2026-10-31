# Maintainers

## Lead maintainer

- **GBLMX** — project lead, architecture, security boundaries, releases
  - GitHub: [@GBLMX](https://github.com/GBLMX)
  - Contact for conduct: use the private contact options on the GitHub profile

## Area ownership

| Area | Owner | Paths |
|---|---|---|
| Kernel and text model | GBLMX | `crates/anvil-core/`, `crates/anvil-editor/`, `crates/anvil-api/` |
| TUI and input | GBLMX | `crates/anvil-tui/` |
| IPC and plugin host | GBLMX | `crates/anvil-ipc/`, `crates/anvil-plugin-host/` |
| Go gateway and sandbox | GBLMX | `go/` |
| Release and CI | GBLMX | `.github/`, `RELEASE.md`, `Cargo.toml`, `go.mod` |

`CODEOWNERS` is the review-routing source of truth. This table explains the
intent; it is not a list of additional maintainers.

## Reviewers

No additional reviewers are listed yet. A reviewer is added when a contributor
has demonstrated sustained, scoped review judgment. Reviewers are not
maintainers until the process in `GOVERNANCE.md` is completed.

## Adding or removing ownership

Ownership changes require a pull request updating both this file and
`.github/CODEOWNERS`. A security-sensitive ownership change also requires
maintainer consensus and, when it changes the trust model, an ADR.

## Security reports

Do not contact maintainers through public issues for vulnerabilities. Follow
`SECURITY.md`.

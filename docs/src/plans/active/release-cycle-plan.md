# Release Cycle Plan

Status: Active; slices 1-5 landed, slice 6 tracks the first timeboxed milestone.
Owner: maintainers (BDFL).
Last reviewed: 2026-11-26

## Scope

Adopt the milestone and train model in
[ADR-0080](../../adr/0080-release-cadence-and-synchronization.md) and make each
synchronization rail executable in CI. The plan covers process and tooling only;
it does not define feature content for any release.

## Non-goals

- No public release date before ADR-0060 signing and provenance gates pass.
- No long-lived feature branches, release trains per crate, or LTS commitment.
- No changelog automation that invents user-facing wording from commit titles.
- No scheduled job that auto-upgrades dependencies; drift is reported, upgrades
  are deliberate.

## Slices

| Slice | Deliverable | Status |
|---|---|---|
| 1 | ADR-0080 accepted with cadence, freeze, support, and sync rails | Landed |
| 2 | Weekly scheduled checks for `cargo deny`, `govulncheck`, and dependency drift | Landed |
| 3 | Pull-request changelog gate with label/body escape | Landed |
| 4 | `scripts/release-checklist.sh` wired into the tag build | Landed |
| 5 | `Last reviewed` freshness for active and proposed plans in `docs-check` | Landed |
| 6 | Run the first 0.x milestone under the 8-week timebox and record the outcome | Pending |

## Gates

- Decision ADR: ADR-0080.
- Contract/spec: `docs/src/RELEASE.md`, `docs/src/ROADMAP.md`, and
  `CONTRIBUTING.md` link the cycle and the checklist script.
- Migration or compatibility: no version or tag changes; the 0.8.0 candidate
  keeps its existing gates.
- Security/trust: no new trust boundary; the scheduled jobs run with
  `contents: read`.
- Performance: the PR benchmark gate is unchanged.

## Verification

- Focused tests: `python3 scripts/docs-check.py`,
  `scripts/release-checklist.sh`, `scripts/check-changelog.sh` with a synthetic
  changed-file list.
- Workspace tests: not applicable; no Rust or Go code changes.
- Clippy/fmt/architecture: not applicable.
- Real surface smoke: `actionlint .github/workflows`, `shellcheck scripts/*.sh`,
  and one workflow-dispatch run of the scheduled checks.

## Links

- ADR: [ADR-0080](../../adr/0080-release-cadence-and-synchronization.md)
- SPEC: [Release process](../../RELEASE.md)
- Related plans: [Roadmap](../../ROADMAP.md)

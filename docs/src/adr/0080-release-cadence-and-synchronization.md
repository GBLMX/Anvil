# `ADR-0080`: Release Cadence and Synchronization

- Status: Accepted; PR benchmark gate amended by ADR-0081
- Version: 1.0.0
- Date: 2026-11-26
- Related: ADR-0014, ADR-0021, ADR-0060, ADR-0077, ADR-0081; `GOVERNANCE.md`,
  `docs/src/RELEASE.md`, `docs/src/ROADMAP.md`
- Supersedes:
- Superseded by:

## Context

Anvil has release gates but no cycle. `PROJECT_CHARTER.md` §12 defines delivery
phases, `docs/src/RELEASE.md` defines the 0.x gate list, and
`docs/src/ROADMAP.md` calls itself directional, but nothing says when a cycle
starts or ends, what may still merge after a scope decision, or how version,
changelog, contracts, docs, dependencies, and vendored upstreams stay in sync.
The result is a release that can drift indefinitely or ship a half-frozen tree.

The current state is a single-maintainer, pre-alpha project with workspace
version `0.8.0`; public tagging is blocked on production signing custody and
GitHub provenance (ADR-0060). The repository already proves most individual
gates:

- `scripts/check-frozen-version.sh` pins the workspace version in CI.
- `.github/workflows/release.yml` rejects a tag that differs from `Cargo.toml`.
- `scripts/api-surface-check.sh`, `cargo-semver-checks`, and the generated
  binding checks guard public contracts.
- `scripts/docs-check.py` guards links, anchors, chapters, plan locations, and
  the root documentation budget.
- The PR benchmark job rejects text p50 regressions above 10% plus a 100 µs
  absolute delta; p95/p99 remain reported and are measured on controlled runs
  (ADR-0081).

What is missing is the cycle around them: a freeze policy, a support window,
scheduled dependency drift checks, per-PR changelog enforcement, a scripted
release checklist, and plan review freshness.

External projects solve this differently. The Linux kernel uses a two- to
three-month rolling cycle with a ~2-week merge window followed by 6-10 weekly
release candidates, and a regression list that decides the release date
[1]. Go uses a six-month cycle with a four-month development window and a
three-month freeze, and supports the last two major releases [2]. Rust runs a
six-week train through nightly, beta, and stable, so a missed feature waits for
the next train instead of stretching the current one [3]. Kubernetes freezes
scope early, freezes code for ~2 weeks, and maintains a release branch for about
a year [4]. Helix publishes a written release checklist and Calendar Versioning
[5]. Neovim releases "often, but not early", plans at most two milestones ahead,
and lands fixes on `master` before cherry-picking to the release branch [6].
Zed ships a weekly stable channel beside a preview channel [7].

Anvil is smaller than all of them; the design borrows the cycle shape, not the
staffing.

## Decision

### 1. Pre-1.0 milestones are timeboxed and gate-bound

Each 0.x milestone gets an 8-week timebox, tracked in the active plan or
milestone issue. One 2-week extension is allowed and must record why in that
plan. The timebox never overrides a gate: a failing security invariant, release
check, or signature/provenance prerequisite blocks the release even after the
deadline.

### 2. Post-1.0 releases use an 8-week train

Weeks 1-4 accept feature work, week 5 is the freeze, weeks 6-7 are release
candidates, and week 8 is the release. Patch releases are cut on demand between
trains. Eight weeks is chosen over the six-week Rust train because a single
maintainer also owns signing, packaging, and four toolchains; it is chosen over
the six-month Go cycle because a pre-1.0 editor needs faster feedback.

### 3. Scope freezes before code freezes

Plan slices and ADRs for the next milestone are accepted by the start of week 1;
after that, new scope waits for the next cycle. From week 5 only bug fixes,
tests, and documentation merge. Freeze exceptions require BDFL approval recorded
in the pull request or issue. From the first release candidate only critical
fixes that landed on `master` may be cherry-picked.

### 4. Development stays on one branch

`main` is the only development branch. A release branch is created at the first
release candidate and receives cherry-picks only; there are no long-lived
feature branches. This keeps synchronization cost at one merge point instead of
N branch pairs.

### 5. Support the last two minor lines

Security, data-loss, and critical-regression fixes are backported to the current
and previous minor line. Fixes land on `main` first and are then cherry-picked.
Long-term support is not implied; selecting an LTS line requires its own ADR.

### 6. Synchronization runs on six rails

1. **Version.** `Cargo.toml` `workspace.package.version` is the single source.
   A version bump updates `scripts/check-frozen-version.sh`'s `expected`
   constant in the same commit, and the release tag `vX.Y.Z` must equal it.
2. **Changelog.** Every user-visible change adds an `[Unreleased]` entry.
   Pull requests touching shipping code fail CI without it unless labeled
   `skip-changelog` or marked `[skip changelog]` in the body.
3. **Contracts.** Public kernel declarations change only with a deliberate
   `api-surface.sha256` update; `cargo-semver-checks` and the generated binding
   checks stay green; IPC, plugin, settings, and index schemas remain versioned.
4. **Docs.** `scripts/docs-check.py` and the mdBook build run on every change;
   plans live under `plans/active`, `plans/proposed`, or `history/plans`.
5. **Dependencies and security.** A weekly scheduled workflow runs
   `cargo deny check`, `govulncheck ./...`, and reports available Rust and Go
   module updates. Upgrade work is batched at cycle start, not during a freeze.
6. **Vendored upstreams.** Luau, tree-sitter grammars, and protocol
   specifications record their upstream revision and local patch set; they are
   re-synced at cycle start. A new native dependency still needs an ADR.

### 7. Plan documents state when they were last reviewed

Active and proposed plans carry `Last reviewed: YYYY-MM-DD` within 180 days;
`scripts/docs-check.py` fails older or missing dates. Reviewing a plan means
confirming or updating its status line, not rewriting it.

## Consequences

Positive:

- A release date and a quality gate can no longer silently displace each other.
- Freeze rules are written down, so "just one more feature" is a recorded
  exception rather than an accident.
- Synchronization has named owners and scripts, so drift is visible in CI.
- The support window and backport rule match the plugin API promise.

Negative:

- Freeze weeks reduce feature throughput and add calendar pressure; the
  timebox cap limits how long that pressure lasts.
- The changelog gate will occasionally require a `skip-changelog` label on
  internal refactors.
- Weekly scheduled checks produce notification noise and cost CI minutes.
- A single maintainer still owns every rail; the cycle makes the work visible,
  not smaller.

## Verification

- `python3 scripts/docs-check.py` passes, including plan review freshness.
- `scripts/release-checklist.sh vX.Y.Z` fails before the changelog section and
  version freeze agree, and passes on a prepared release commit.
- `.github/workflows/ci.yml` runs the changelog gate on pull requests and the
  existing version, contract, docs, and benchmark gates on every push.
- `.github/workflows/scheduled-checks.yml` runs weekly and on demand.
- `actionlint .github/workflows/*.yml` and `shellcheck scripts/*.sh` pass.

## References

- Linux kernel development process:
  <https://docs.kernel.org/process/2.Process.html> — merge window, weekly
  release candidates, regression-driven release date, stable backports.
- Go release cycle: <https://go.dev/wiki/Go-Release-Cycle> — six-month cycle,
  freeze policy, two-release support window.
- Rust release channels:
  <https://doc.rust-lang.org/book/appendix-07-nightly-rust.html> — six-week
  train through nightly, beta, and stable.
- Kubernetes release cycle:
  <https://github.com/kubernetes/community/blob/main/contributors/devel/sig-release/release.md>
  — enhancement freeze, code freeze, release branch lifetime.
- Helix release process:
  <https://github.com/helix-editor/helix/blob/master/docs/releases.md> —
  Calendar Versioning and a scripted release checklist.
- Neovim maintainer notes:
  <https://github.com/neovim/neovim/blob/master/MAINTAIN.md> — "release often,
  but not early", two-milestone planning, cherry-pick maintenance.
- Zed releases: <https://zed.dev/releases/stable> — weekly stable releases
  beside a preview channel.

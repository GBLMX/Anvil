# Governance

Anvil is pre-alpha. This document defines how decisions are made while the
project is small; it is intentionally lighter than the governance of a mature
foundation project.

## Roles

### Contributor

Anyone who opens an issue, reviews a change, improves documentation, or submits
a pull request. Contributors do not need commit access.

### Reviewer

A contributor trusted to review a focused area, reproduce bugs, and leave
blocking review comments. Reviewers are listed in `MAINTAINERS.md` and may be
requested through `CODEOWNERS`.

### Maintainer

A maintainer can merge changes, manage releases, and speak for the project in
that area. Maintainers are responsible for:

- protecting the process and trust boundaries in `PROJECT_CHARTER.md`;
- reviewing security-sensitive changes;
- keeping CI and documented checks trustworthy;
- declining changes that add unmeasured security or compatibility claims;
- recording architectural decisions in `docs/adr/`.

The current maintainer and area ownership are listed in `MAINTAINERS.md`.

## Decision making

The default is **lazy consensus**:

1. Discuss the change in an issue or pull request.
2. Give maintainers and affected area owners a reasonable chance to object.
3. If no blocking concern remains, a maintainer may merge.

Consensus is not a vote. A maintainer must explain a blocking objection in
terms of correctness, security, compatibility, or maintenance cost.

### When an ADR is required

An ADR is required before implementation when a change affects:

- a process or trust boundary;
- the public Rust API, IPC contract, plugin contract, or capability model;
- the text model, selection model, or snapshot ownership;
- a security invariant or sandbox behavior;
- a release or compatibility promise.

ADR status values and the index are maintained under `docs/adr/` and
`PROJECT_CHARTER.md`.

### Security decisions

Security fixes may bypass the normal discussion period. A maintainer may merge
a narrowly scoped fix after review by another maintainer or security reviewer.
Disclosure coordination follows `SECURITY.md`.

### Deadlocks

If consensus cannot be reached:

1. Write down the competing technical tradeoffs.
2. Ask the area owner and one uninvolved maintainer to review.
3. If still blocked, the lead maintainer makes the final call and records the
   rationale.

The final call must not silently waive a test, threat-model boundary, or
supported-version policy.

## Becoming a maintainer

A contributor is eligible after sustained, scoped contributions such as:

- multiple reviewed changes in one area;
- careful reviews that caught real defects;
- security or performance work with reproducible evidence;
- documentation and release work that reduced maintenance cost.

A maintainer is added by consensus of the existing maintainers. The change is
recorded in `MAINTAINERS.md` and `CODEOWNERS`. Access is granted only to the
areas the person has demonstrated judgment in.

## Removing a maintainer

A maintainer may step down at any time. Access may be removed for inactivity
after a documented attempt to contact the person, or immediately for a security
or conduct violation. Security and conduct decisions are handled privately.

## Changes to this document

Changes to governance are reviewed like an architectural change. A material
change requires an ADR and a pull request that updates `MAINTAINERS.md` and
`CODEOWNERS` if ownership changes.

## Releases

Release authority and the release checklist live in `RELEASE.md`. A release
must pass the gates in `CONTRIBUTING.md`; no maintainer may waive a failing
security test for a release.

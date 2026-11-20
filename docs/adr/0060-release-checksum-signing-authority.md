# ADR-0060: Release Checksum Signing Authority

- Status: Accepted; applies to internal release candidates
- Version: 1.0.0
- Date: 2026-11-20
- Related: [ADR-0034](0034-release-signing-authority.md),
  [ADR-0023](0023-internal-capability-closure.md),
  [RELEASE.md](../../RELEASE.md)
- Supersedes:
- Superseded by:

## Context

ADR-0034 selects hardware-backed Ed25519 signing for public plugin packages
and explicitly leaves `SHA256SUMS` signing without a decision. The release
workflow currently signs `SHA256SUMS` with an exportable CI-held Ed25519 PEM.
That is acceptable for internal release candidates, but it is not sufficient
authority for a public release.

The `0.7.0` release candidate needs an explicit checksum-signing policy before
it can be exercised end to end. YubiKey custody and GitHub provenance remain
external release-time steps and must not be silently replaced by a CI secret.

## Decision

### 1. Internal candidates use the CI checksum key

For internal or non-public release candidates, the CI-held Ed25519 key in
`ANVIL_RELEASE_SIGNING_KEY` signs the exact `SHA256SUMS` bytes. The matching
public key is published as `release.pub.pem` beside the artifacts.

This permits automated candidate builds, signature verification, and release
rehearsals without claiming a public release.

### 2. Public releases require a separate hardware-backed authority

A public release must not use this CI checksum key as its sole authority. The
public checksum key must be hardware-backed and follow the offline ceremony
and key-continuity rules selected for public signing. That ceremony is not
authorized or implemented by this ADR.

### 3. Sign exact bytes only

The signing input is the exact `SHA256SUMS` file generated from the final
artifacts. Any rebuild, artifact replacement, or checksum edit invalidates the
signature and requires a new signing operation.

Release verification checks:

- `sha256sum -c SHA256SUMS`;
- the detached signature over `SHA256SUMS` with `release.pub.pem`;
- required archive entries and the SPDX SBOM;
- the key identifier and public key shipped with the artifacts.

### 4. Public-release gate remains

`0.7.0` may be prepared and tagged only through the internal-candidate process
until the public checksum and plugin signing ceremony is accepted. A public
release additionally requires build-provenance attestation and the production
key-continuity requirements from ADR-0034.

## Consequences

Positive:

- The checksum-signing decision is no longer implicit in a CI secret.
- Internal candidates can be signed, verified, and rehearsed automatically.
- Public release authority remains visibly separate and unclaimed.

Negative:

- The first public release still needs a checksum-key custody and rotation
  ceremony.
- Internal candidate signatures are not a public trust root.

## Verification

- The release workflow signs `SHA256SUMS`, publishes `release.pub.pem`, and
  verifies the detached signature before publication.
- `docs/release-dry-run-2026-11-20.md` records an end-to-end signature
  verification with ephemeral production-like keys.
- A public release is rejected until the production checksum authority and
  provenance gate are explicitly accepted.

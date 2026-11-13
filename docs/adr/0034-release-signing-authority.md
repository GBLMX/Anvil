# ADR-0034: Release and Plugin Signing Authority

- Status: Proposed; public-release direction selected, implementation deferred until the `0.6.0` freeze exits
- Date: 2026-11-13
- Related: [ADR-0021](0021-signed-plugin-index.md),
  [ADR-0020](0020-content-addressed-plugin-store.md),
  [RELEASE.md](../../RELEASE.md), [threat model](../threat-model.md)

## Context

Official plugin packages are detached-Ed25519 signatures over the SHA-256 digest
of the exact archive bytes. The current GitHub-hosted release workflow reads
`ANVIL_PLUGIN_SIGNING_KEY` from a repository secret containing an exportable PEM
private key. That is simple and automated, but a compromised workflow or runner
can exfiltrate the key and sign arbitrary packages.

A production signing key has now been generated on a YubiKey 5C in PIV slot 9c:

- algorithm: Ed25519;
- origin: generated on-device;
- PIN policy: always;
- touch policy: always;
- private key is non-exportable.

`scripts/package-official-plugin.sh` accepts `piv:SLOT` and signs the archive
digest through `yubico-piv-tool`. A real package was signed, verified with the
public key, imported by `anvil-gateway`, explicitly enabled, and used from the
managed store by a real TUI session.

A GitHub-hosted runner cannot access a locally attached YubiKey. Supporting the
hardware key therefore requires either an offline signing step or a
self-hosted runner. This ADR selects that authority model. It does not change
the frozen `0.6.0` release workflow yet.

## Decision

### 1. Authority classes

- **Internal/non-public builds:** may continue to use a CI-held PEM key while
  the internal-polish freeze is active.
- **Public releases:** must use an offline, hardware-backed Ed25519 key for
  plugin packages. A GitHub-hosted runner must never receive the production
  private key.
- **Release checksum signing (`SHA256SUMS`):** out of scope for this ADR. It
  must receive its own decision before the first public release.

### 2. Public-release flow

CI builds the release layout without production signing and emits the exact
archive checksums. An offline signing ceremony downloads or receives those exact
bytes, verifies the checksums, signs each digest with the YubiKey, and returns
detached signatures. A final publish job verifies every signature against the
trusted public keys before publishing.

The signing input is the digest of the exact CI artifact. Rebuilding after
signing is forbidden: a new build requires a new signature.

### 3. Self-hosted runners

A self-hosted runner with an attached YubiKey is rejected for now. It turns the
runner into a signing oracle and requires dedicated physical access control,
network isolation, manual approval, and protection from untrusted jobs. Revisit
only if release frequency makes offline signing unworkable and a dedicated
runner can be operated as production infrastructure.

### 4. Key management

- At least two YubiKeys with independently generated keys are required before
  the first public release.
- Private keys are never exported, copied, or stored as PEM files.
- A key identifier is `sha256:` plus the lowercase hex SHA-256 of the raw
  32-byte Ed25519 public key.
- During rotation, the previous and new public keys are both trusted for an
  overlap window. A package signature names the key it used.
- The backup key is stored offline and is used only for continuity or rotation,
  not routine signing.
- The current single `official.pub` file is insufficient for backup and
  rotation. The package layout and trust store must support multiple official
  keys before public release.

### 5. Verification gates

Public publication is rejected unless all of the following hold:

- every package signature verifies over the archive digest;
- every signing key is in the accepted official key set;
- no artifact digest changes between build and publish;
- signatures and checksums are complete for every official package;
- missing, extra, duplicated, or revoked key IDs fail closed;
- CI asserts that managed packages remain disabled until explicitly enabled.

## Alternatives

### CI-held PEM for all releases

Rejected for public releases because the production key is exportable and
available to the build runner. It remains acceptable for internal/non-public
builds during the freeze.

### Self-hosted runner with YubiKey

Rejected for now because the runner can trigger signing and expands the
attack surface to persistent release infrastructure.

### Cloud KMS or HSM

Not selected. It could provide non-exportable keys and automated signing, but
adds a provider dependency, cost, and an availability/account-compromise risk.
It may be reconsidered if offline signing becomes a release bottleneck.

## Consequences

- Public releases gain a manual signing ceremony and a longer release path.
- A lost or destroyed YubiKey must not block releases; the second key and
  rotation procedure are mandatory.
- Package trust publication must move from one `official.pub` to a keyed set
  with rotation and revocation support.
- The current hardware signing path remains local-only until the release
  workflow is redesigned.
- No production workflow changes are required during the `0.6.0` freeze.

## Implementation gates

1. Provision the second YubiKey and document the offline signing ceremony.
2. Extend official package publication to carry the signing key ID and publish
   multiple official public keys.
3. Implement the CI build → offline sign → verified publish flow.
4. Add tamper, wrong-key, missing-signature, revoked-key, and rotation-overlap
   tests.
5. Keep `ANVIL_PLUGIN_SIGNING_KEY=piv:9c` limited to local signing; the CI PEM
   path remains the internal-build path until the freeze exits.
6. Decide release-checksum signing separately before the first public release.

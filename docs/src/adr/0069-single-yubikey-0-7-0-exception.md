# ADR-0069: Single-YubiKey Exception for 0.7.0

- Status: Accepted
- Version: 1.0.0
- Date: 2026-11-22
- Related: [ADR-0034](0034-release-signing-authority.md),
  [ADR-0060](0060-release-checksum-signing-authority.md), [RELEASE.md](../RELEASE.md)
- Supersedes: ADR-0034 second-device requirement for `0.7.0` only
- Superseded by:

## Context

ADR-0034 requires at least two independently generated YubiKeys before a public
release. Only one YubiKey 5C NFC is currently available; funding for a second
device is deferred.

The available device already has an on-device Ed25519 plugin-signing key in PIV
slot `9c`. A separate on-device Ed25519 checksum key was generated in PIV slot
`9d` with PIN-always and touch-always policies. This preserves key separation
for the two artifact classes while accepting a single physical device.

## Decision

For `0.7.0` only:

- One YubiKey may carry both official plugin signing and release checksum
  signing.
- Plugin signing uses PIV slot `9c`; checksum signing uses PIV slot `9d`.
- The two keys must be independently generated and have distinct key IDs.
- The second physical YubiKey, multi-key trust publication, rotation overlap,
  and offline CI signing integration remain deferred.

This exception does not authorize the CI-held PEM path as a public trust root
and does not replace GitHub build-provenance verification.

## Consequences

Positive:

- The `0.7.0` release can use a hardware-backed plugin key and a separate
  hardware-backed checksum key without a second physical device.
- The public authority boundary remains visibly separate from CI secrets.

Negative:

- Loss or destruction of the single YubiKey can block release and rotation
  until a replacement is provisioned.
- There is no offline backup during the exception window.
- Multi-key trust and key-continuity rotation remain unfinished.

## Verification

- Plugin key ID:
  `sha256:f9417611718349316c304ee4e2262e7f6f42137608111d7918bad9085cc216c3`
- Checksum key ID:
  `sha256:3055cc47d2cc940d519e7eb370b8bb3049b59903c0c400e426de1b0bcb15ba8b`
- Plugin package signature verifies with the slot `9c` public key.
- `SHA256SUMS` verifies with the slot `9d` public key.
- Install, upgrade, uninstall, and installed `doctor` pass with the
  hardware-signed package.

## References

- [Release procedure](../RELEASE.md)
- [YubiKey rehearsal](../history/releases/release-yubikey-rehearsal-2026-11-22.md)

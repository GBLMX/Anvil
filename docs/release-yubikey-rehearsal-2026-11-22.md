# YubiKey Release Rehearsal — 2026-11-22

Status: local hardware-backed rehearsal passed. This does **not** authorize a
public release.

## Device and keys

- Device: YubiKey 5C NFC
- Plugin signing: PIV slot `9c`, Ed25519,
  `sha256:f9417611718349316c304ee4e2262e7f6f42137608111d7918bad9085cc216c3`
- Checksum signing: PIV slot `9d`, Ed25519,
  `sha256:3055cc47d2cc940d519e7eb370b8bb3049b59903c0c400e426de1b0bcb15ba8b`
- Both private keys remained on the YubiKey; no PEM export was created.
- ADR-0069 records the temporary single-device exception for `0.7.0`.

## Commands

```sh
scripts/release-yubikey.sh
scripts/release-yubikey.sh --verify-only dist/anvil-0.7.0
```

The script builds the `full` release layout, signs the official plugin through
PIV slot `9c`, runs the install/upgrade/uninstall/doctor lifecycle smoke,
creates the SPDX SBOM and tarball, signs the exact `SHA256SUMS` through PIV slot
`9d`, and verifies both signatures. It refuses a dirty tracked worktree or a
tag that does not point at `HEAD`.

## Artifact evidence

```text
package digest      594c23658c78d109d968efe7142bebba4d545baad64ebb1393a3ddb13744fc09
tarball             0d82f5f2112d6b81e4a1b1b450c6c2d6790fb85bb06ce524819f44c12e7de220
SBOM                e009cbaa45372179cf59f78db8a29a3af8a361e2e254b1ef8c2551a06cd2c950
SHA256SUMS          16e53685d978e4238530c3a544c9be3659acdde3d7d5abf1f5082ec04f1fd032
checksum public key 8dd26f932b6c23a946038db5c3f6f548197ded42c74554af838782856b39af24
signature           83c03ecb40cada6894ab56f492bb49b4accd780f37da08f422d4b87c2ed35123
```

Verification results:

```text
sha256sum -c SHA256SUMS: OK
openssl pkeyutl -verify (slot 9d): Signature Verified Successfully
SBOM: SPDX-2.3, 6 packages
plugin public key differs from the checksum public key
```

## Remaining public-release blockers

- Only one physical production YubiKey is provisioned. ADR-0069 accepts this
  single-device exception for `0.7.0`; the second device and rotation overlap
  remain deferred.
- The official package trust layout still carries a single `official.pub`;
  keyed multi-key trust and rotation support are not implemented.
- The GitHub release workflow still uses CI-held PEM secrets; the CI build to
  offline YubiKey signing flow is not implemented.
- GitHub build-provenance attestation must still be generated and verified.

No public tag was pushed from this rehearsal.

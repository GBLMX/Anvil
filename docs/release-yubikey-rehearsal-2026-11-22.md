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
rm -rf /tmp/anvil-yubikey-layout
ANVIL_PLUGIN_SIGNING_KEY=piv:9c \
  scripts/release-layout.sh /tmp/anvil-yubikey-layout
scripts/release-lifecycle-smoke.sh /tmp/anvil-yubikey-layout
syft /tmp/anvil-yubikey-layout \
  -o "spdx-json=/tmp/anvil-0.7.0-yubikey.sbom.spdx.json"
tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner \
  -czf /tmp/anvil-0.7.0-yubikey-linux-x86_64.tar.gz \
  -C /tmp/anvil-yubikey-layout .
sha256sum anvil-0.7.0-yubikey-linux-x86_64.tar.gz \
  anvil-0.7.0-yubikey.sbom.spdx.json release.pub.pem > SHA256SUMS
yubico-piv-tool -a verify-pin --sign -s 9d -A ED25519 \
  -i SHA256SUMS -o SHA256SUMS.sig
```

`release-layout.sh` signed the official plugin through PIV slot `9c`.
`yubico-piv-tool` signed the exact `SHA256SUMS` bytes through PIV slot `9d`.
`release-lifecycle-smoke.sh` completed install, upgrade, uninstall, and the
installed `doctor` profile with the hardware-signed package.

## Artifact evidence

```text
package digest      594c23658c78d109d968efe7142bebba4d545baad64ebb1393a3ddb13744fc09
tarball             0d82f5f2112d6b81e4a1b1b450c6c2d6790fb85bb06ce524819f44c12e7de220
SBOM                e142beb1553e28c3760af6ce98db1ea29228ba63a4bce35a84f5ad5a47222d90
SHA256SUMS          a6f72b1defea4d7e45aadd2ed34d7f494f4b952e70dceb5a2bc36fee96caa6ad
checksum public key 8dd26f932b6c23a946038db5c3f6f548197ded42c74554af838782856b39af24
signature           960e209edd305056736cc0349e267dcf9bb0d2a5ef15f94041af6c0645cdc6d5
```

Verification results:

```text
sha256sum -c SHA256SUMS: OK
openssl pkeyutl -verify (slot 9d): Signature Verified Successfully
SBOM: SPDX-2.3, 7 packages
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

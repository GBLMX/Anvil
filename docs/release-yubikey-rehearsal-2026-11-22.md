# YubiKey Release Rehearsal — 2026-11-22

Status: local hardware-backed rehearsal passed. This does **not** authorize a
public release.

## Device and key

- Device: YubiKey 5C NFC
- PIV slot: `9c`
- Algorithm: Ed25519
- Public key identifier:
  `sha256:f9417611718349316c304ee4e2262e7f6f42137608111d7918bad9085cc216c3`
- The private key remained on the YubiKey; no PEM export was created.

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
  anvil-0.7.0-yubikey.sbom.spdx.json > SHA256SUMS
yubico-piv-tool -a verify-pin --sign -s 9c -A ED25519 \
  -i SHA256SUMS -o SHA256SUMS.sig
```

`release-layout.sh` signed the official plugin through PIV slot `9c`.
`release-lifecycle-smoke.sh` completed install, upgrade, uninstall, and the
installed `doctor` profile with the hardware-signed package.

## Artifact evidence

```text
package digest 594c23658c78d109d968efe7142bebba4d545baad64ebb1393a3ddb13744fc09
tarball        0d82f5f2112d6b81e4a1b1b450c6c2d6790fb85bb06ce524819f44c12e7de220
SBOM           e142beb1553e28c3760af6ce98db1ea29228ba63a4bce35a84f5ad5a47222d90
SHA256SUMS     bd490669b31ef6ebfbd49658899cacd2a38569393cfc2a7d341c0699f0a08e2f
public key PEM 97e8340ecdb65ae9e98dc9eca4bf3263bb812496f41f2f0de51a5137eb142b1c
```

Verification results:

```text
sha256sum -c SHA256SUMS: OK
openssl pkeyutl -verify: Signature Verified Successfully
SBOM: SPDX-2.3, 7 packages
package public key matches the checksum public key
```

## Remaining public-release blockers

- The checksum rehearsal used the same PIV key as the plugin package. ADR-0034
  and ADR-0060 require a separate hardware-backed checksum authority for public
  release.
- Only one production YubiKey is currently provisioned; the policy requires two
  independently generated keys before a public release.
- The official package trust layout still carries a single `official.pub`;
  keyed multi-key trust and rotation support are not implemented.
- The GitHub release workflow still uses CI-held PEM secrets; the CI build to
  offline YubiKey signing flow is not implemented.

No public tag was pushed from this rehearsal.

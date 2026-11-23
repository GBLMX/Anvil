# Offline signing and verified publish

Release builds and signing are separate trust domains. CI builds unsigned
inputs with provenance; the maintainer signs them offline with the YubiKey; a
manual publish job verifies both before creating release assets.

## 1. Build inputs

Pushing a `v*` tag runs `Release build`. It uploads:

```text
anvil-<version>-linux-x86_64-inputs.tar.gz
anvil-<version>.sbom.spdx.json
anvil-<version>-inputs.sha256
```

The tarball carries GitHub build provenance. No production key is available to
this workflow.

## 2. Sign offline

Download the build-input artifact from the build run, then:

```bash
scripts/release-yubikey.sh \
  --inputs dist/build-inputs \
  --out "dist/anvil-0.7.0" \
  --expected-checksum-key-sha256 "$ANVIL_CHECKSUM_KEY_SHA256"
```

The script verifies the input checksums, extracts the CI layout, re-signs every
official plugin package with the PIV plugin key, regenerates the SBOM, creates
the release tarball, and signs `SHA256SUMS` with the PIV checksum key. The
signed output records the CI input checksum file.

## 3. Stage signed assets

Create a draft release and upload the signed files:

```bash
tag=v0.7.0
version="${tag#v}"
signed="dist/anvil-$version"

gh release create "$tag-offline" \
  --draft --target "$tag" \
  --title "$tag offline signing" \
  --notes "Offline-signed assets awaiting verification"

gh release upload "$tag-offline" \
  "$signed/anvil-$version-linux-x86_64.tar.gz" \
  "$signed/anvil-$version.sbom.spdx.json" \
  "$signed/anvil-$version-inputs.sha256" \
  "$signed/SHA256SUMS" \
  "$signed/SHA256SUMS.sig" \
  "$signed/release.pub.pem"
```

## 4. Publish

Run `Release publish` with:

- the release tag;
- the tag build run ID;
- the draft release tag from step 3;
- the expected SHA-256 fingerprint of `release.pub.pem`.

The job verifies build provenance for the CI input tarball, verifies the CI
input checksums, verifies `SHA256SUMS` and its Ed25519 signature against the
expected checksum key, and checks that the signed input checksum file matches
the CI artifact. It uploads only the verified files to the final release and
deletes the offline draft.

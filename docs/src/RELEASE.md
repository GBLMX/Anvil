# Release Process

Anvil is pre-alpha. This document describes the release procedure for the
current `0.x` line; it is not a compatibility guarantee.

## Release candidate

`0.8.0` is the current internal release candidate. Do not create a public
release tag until production plugin/checksum signing and GitHub provenance are
authorized. Internal candidates use the CI checksum key under
[ADR-0060](adr/0060-release-checksum-signing-authority.md).
`scripts/check-frozen-version.sh` keeps the workspace version pinned to the
chosen candidate version; change it only with the release version.

The `0.7.0` gate matrix and signed dry-run evidence remain in
[docs/src/history/releases/release-dry-run-2026-11-22.md](history/releases/release-dry-run-2026-11-22.md).
The `0.8.0` line carries the ADR-0077 kernel API break.

The `0.8.0` candidate is not publicly publishable until all of these current
gates hold:

- production plugin/checksum signing custody is authorized;
- GitHub build provenance is verified for the release inputs;
- `scripts/check-frozen-version.sh` passes for the workspace version;
- candidate work stays under `[Unreleased]` in `CHANGELOG.md`;
- the plugin API, gateway protocol, plugin index schema, and settings schema
  remain internal contracts;
- the release gates and checklist in this document pass for the candidate.
- P0/P1 internal polish is complete and dogfooded; no open P0 data-loss,
  corruption, or security issue remains;
- session restore/crash recovery and settings/LSP/Git/file lifecycle
  failure/cancellation tests pass;
- the reference performance baseline is recorded and re-run at the documented
  dependency/release boundaries;
- release layout, installer, plugin bootstrap, and the hardware-backed signing
  dry run pass with production-like keys.


## Versioning

- Releases use `vX.Y.Z` tags. Before `1.0`, version lines are release-series
  labels rather than a compatibility promise.
- `0.x` releases may contain breaking API changes; a public compatibility
  promise starts only when an interface is explicitly frozen.
- `0.4.x` is the post-`0.4.0` feature line. A release may add one bounded
  capability slice; `0.4.x` is not restricted to patch-only changes.
- Bugfix and security releases still use the same patch-tag format, but each
  release must state whether it is a fix slice or a feature slice.
- Rust kernel crates, the gateway protocol, and the plugin API are versioned
  separately once their boundaries are frozen.
- Windows and macOS are not release targets for v1.

## Release gates

A release candidate must pass the same checks as `CONTRIBUTING.md` plus:

```sh
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
GOTOOLCHAIN=go1.27.4 go test -race ./...
GOTOOLCHAIN=go1.27.4 go vet ./...
GOTOOLCHAIN=go1.27.4 golangci-lint run ./...
govulncheck ./...
```

Also verify:

- generated bindings are committed and unchanged;
- `CHANGELOG.md` has an entry under the release version;
- the changelog uses user-visible language and links relevant ADRs;
- no generated caches, profiles, or local benchmark output are committed;
- the gateway and exec-runner packaging paths are documented;
- security fixes have regression tests.

## Checklist

1. Create a release branch or freeze `main`.
2. Run the release gates on Linux.
3. Update `CHANGELOG.md`, the version fields, and the `expected` constant in
   `scripts/check-frozen-version.sh`.
4. Check the generated TypeScript bindings.
5. Build the full official composition and the gateway/exec-runner artifacts.
6. Run `scripts/release-checklist.sh vX.Y.Z`; create the signed/tagged release
   only after it and CI pass.
7. Publish release notes with breaking changes, security notes, and unsupported
   platforms.
8. Verify installation from the published artifact, not only from the build
   directory.

## Artifacts

Pushing a `vX.Y.Z` tag runs `.github/workflows/release.yml`. It builds a test
layout with an ephemeral CI key, verifies the installer and lifecycle, removes
the test-signed package directory, and uploads unsigned build inputs:

- `anvil-<version>-linux-x86_64-inputs.tar.gz`
- `anvil-<version>.sbom.spdx.json`
- `anvil-<version>-inputs.sha256`

The tarball carries a GitHub build-provenance attestation. The workflow does
not sign with production keys and does not create or publish a release.

For local hardware-backed signing, point `ANVIL_PLUGIN_SIGNING_KEY` at a PIV
slot (`piv:9c`) instead of a PEM path. `scripts/package-official-plugin.sh`
signs the SHA-256 digest through `yubico-piv-tool`; the tool prompts for the PIV
PIN and touch, and reads the matching public key from the same slot. Set
`YUBICO_PIV_TOOL` when the binary is not on `PATH`.

The local PIV signing rehearsal is recorded in
[docs/src/history/releases/release-yubikey-rehearsal-2026-11-22.md](history/releases/release-yubikey-rehearsal-2026-11-22.md);
it is evidence for the hardware path, not public release authority.

For `0.7.0`, ADR-0069 accepts one physical YubiKey with separate plugin and
checksum keys in PIV slots `9c` and `9d`; a second device is deferred.

The scripted local flow is `scripts/release-yubikey.sh`. It builds the `full`
layout, signs the plugin in slot `9c`, runs the lifecycle smoke, creates the
SBOM and tarball, signs `SHA256SUMS` in slot `9d`, and verifies both
signatures. `--verify-only <dir>` verifies an existing bundle without the key.
The script stops at offline signing; it does not upload or publish. It refuses
a dirty tracked worktree or a tag that does not point at `HEAD`.

It builds `scripts/release-layout.sh`, generates an SPDX JSON SBOM with
`syft`, creates a tarball, writes `SHA256SUMS`, and signs the exact checksum
file.

Public publication still requires a verified offline-signing handoff over the
CI build inputs, build-provenance verification, and explicit authorization.
The current local script rebuilds its bundle; it is not yet wired to consume
the workflow artifact.

Verification:

```bash
sha256sum -c SHA256SUMS
openssl pkeyutl -verify -pubin -inkey release.pub.pem \
  -rawin -in SHA256SUMS -sigfile SHA256SUMS.sig
gh attestation verify anvil-<version>-linux-x86_64.tar.gz --repo GBLMX/Anvil
```

Local packaging and signing dry-run evidence, including artifact hashes and
the installed-artifact check, is recorded in
[docs/src/history/releases/release-dry-run-2026-11-22.md](history/releases/release-dry-run-2026-11-22.md).

The tarball includes `install.sh`, which copies the layout and runs
`anvil-gateway -install-bundled` when signed packages are present. The package
public key is shipped as `share/anvil/packages/official.pub`.

The tarball also includes `uninstall.sh`, which removes only manifest-recorded
files. The release workflow runs `scripts/release-lifecycle-smoke.sh` on the
clean runner: install, manifest and doctor verification, upgrade replacing the
schema and cleaning stale files, then uninstall while preserving config and
runtime state.

The full official composition and third-party distributions use the same
kernel source boundary described in
[docs/src/adr/0013-kernel-distribution-boundary.md](adr/0013-kernel-distribution-boundary.md).

## Hotfixes

Security fixes may use an expedited branch. They still require:

- a regression test;
- a security review;
- a changelog entry;
- a patched release and, when applicable, an advisory.

## Rollback

A release with a confirmed security or data-loss regression may be marked
withdrawn. Do not delete tags or rewrite published history; publish a corrected
version and document the affected range.

## Offline signing and verified publish


Release builds and signing are separate trust domains. CI builds unsigned
inputs with provenance; the maintainer signs them offline with the YubiKey; a
manual publish job verifies both before creating release assets.

### 1. Build inputs

Pushing a `v*` tag runs `Release build`. It uploads:

```text
anvil-<version>-linux-x86_64-inputs.tar.gz
anvil-<version>.sbom.spdx.json
anvil-<version>-inputs.sha256
```

The tarball carries GitHub build provenance. No production key is available to
this workflow.

### 2. Sign offline

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

### 3. Stage signed assets

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

### 4. Publish

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

# Release Process

Anvil is pre-alpha. This document describes the release procedure for the
current `0.x` line; it is not a compatibility guarantee.

## Release candidate

`0.7.0` is the current internal release candidate. Do not create a public
release tag until production plugin/checksum signing and GitHub provenance are
authorized. Internal candidates use the CI checksum key under
[ADR-0060](docs/adr/0060-release-checksum-signing-authority.md).
`scripts/check-frozen-version.sh` keeps the workspace version pinned to the
chosen candidate version; change it only with the release version.

The `0.7.0` gate matrix and signed dry-run evidence are recorded in
[docs/release-dry-run-2026-11-22.md](docs/release-dry-run-2026-11-22.md).


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
GOTOOLCHAIN=go1.26.11 go test -race ./...
GOTOOLCHAIN=go1.26.11 go vet ./...
GOTOOLCHAIN=go1.26.11 golangci-lint run ./...
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
3. Update `CHANGELOG.md` and the version fields.
4. Check the generated TypeScript bindings.
5. Build the full official composition and the gateway/exec-runner artifacts.
6. Create the signed/tagged release only after CI passes.
7. Publish release notes with breaking changes, security notes, and unsupported
   platforms.
8. Verify installation from the published artifact, not only from the build
   directory.

## Artifacts

Pushing a `vX.Y.Z` tag runs `.github/workflows/release.yml`. The workflow
requires two repository secrets containing Ed25519 PEM private keys:

- `ANVIL_PLUGIN_SIGNING_KEY` signs official plugin packages.
- `ANVIL_RELEASE_SIGNING_KEY` signs `SHA256SUMS`.

For local hardware-backed signing, point `ANVIL_PLUGIN_SIGNING_KEY` at a PIV
slot (`piv:9c`) instead of a PEM path. `scripts/package-official-plugin.sh`
signs the SHA-256 digest through `yubico-piv-tool`; the tool prompts for the PIV
PIN and touch, and reads the matching public key from the same slot. Set
`YUBICO_PIV_TOOL` when the binary is not on `PATH`.

The local PIV signing rehearsal is recorded in
[docs/release-yubikey-rehearsal-2026-11-22.md](docs/release-yubikey-rehearsal-2026-11-22.md);
it is evidence for the hardware path, not public release authority.

For `0.7.0`, ADR-0069 accepts one physical YubiKey with separate plugin and
checksum keys in PIV slots `9c` and `9d`; a second device is deferred.

The scripted local flow is `scripts/release-yubikey.sh`. It builds the `full`
layout, signs the plugin in slot `9c`, runs the lifecycle smoke, creates the
SBOM and tarball, signs `SHA256SUMS` in slot `9d`, and verifies both
signatures. `--verify-only <dir>` verifies an existing bundle without the key.
The script stops at offline signing; it does not upload or publish.
Publication remains the tag-triggered workflow below. It refuses a dirty
tracked worktree or a tag that does not point at `HEAD`.

It builds `scripts/release-layout.sh`, generates an SPDX JSON SBOM with
`syft`, creates a tarball, writes `SHA256SUMS`, and signs the exact checksum
file. GitHub build-provenance attestation is attached to the release tarball.

For internal candidates this CI-held checksum key is allowed by ADR-0060.
For `0.7.0`, the checksum key is the separate on-device slot `9d`. Public
publication still requires the CI build-to-offline-sign-to-publish flow,
build-provenance verification, and explicit authorization. The local script
produces the signed bundle used by that flow; it does not create a release.

The workflow verifies the checksum file, signature, SPDX SBOM, and required
archive entries before publishing, then verifies the published provenance
attestation.

Verification:

```bash
sha256sum -c SHA256SUMS
openssl pkeyutl -verify -pubin -inkey release.pub.pem \
  -rawin -in SHA256SUMS -sigfile SHA256SUMS.sig
gh attestation verify anvil-<version>-linux-x86_64.tar.gz --repo GBLMX/Anvil
```

Local packaging and signing dry-run evidence, including artifact hashes and
the installed-artifact check, is recorded in
[docs/release-dry-run-2026-11-22.md](docs/release-dry-run-2026-11-22.md).

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
[docs/adr/0013-kernel-distribution-boundary.md](docs/adr/0013-kernel-distribution-boundary.md).

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

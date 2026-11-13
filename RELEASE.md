# Release Process

Anvil is pre-alpha. This document describes the release procedure for the
current `0.x` line; it is not a compatibility guarantee.

## Release freeze

`0.6.0` is frozen for long-term internal polish. Do not create release tags or
publish releases during this phase. `.github/workflows/release.yml` rejects a
`0.6.0` tag, and `scripts/check-frozen-version.sh` fails CI on an accidental
version bump. Unfreeze only by updating the version, freeze check, roadmap, and
internal-polish exit gates in one intentional change.


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
5. Build the standard edition and the gateway/exec-runner artifacts.
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

It builds `scripts/release-layout.sh`, generates an SPDX JSON SBOM with
`syft`, creates a tarball, writes `SHA256SUMS`, and signs the exact checksum
file. GitHub build-provenance attestation is attached to the release tarball.

Verification:

```bash
sha256sum -c SHA256SUMS
openssl pkeyutl -verify -pubin -inkey release.pub.pem \
  -rawin -in SHA256SUMS -sigfile SHA256SUMS.sig
gh attestation verify anvil-<version>-linux-x86_64.tar.gz --repo GBLMX/Anvil
```

The tarball includes `install.sh`, which copies the layout and runs
`anvil-gateway -install-bundled` when signed packages are present. The package
public key is shipped as `share/anvil/packages/official.pub`.

The standard edition and third-party distributions use the same kernel source
boundary described in `docs/adr/0013-kernel-distribution-boundary.md`.

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

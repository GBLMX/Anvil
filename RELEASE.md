# Release Process

Anvil is pre-alpha. This document describes the release procedure for the
current `0.x` line; it is not a compatibility guarantee.

## Versioning

- Releases use semantic versioning with a `v` tag: `v0.4.0`, `v0.4.1`.
- `0.x` releases may contain breaking API changes. A public compatibility
  promise starts only when an interface is explicitly frozen.
- Patch releases are for fixes and security updates; do not add new capability
  families in a patch release.
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
GOTOOLCHAIN=go1.26.10 go test -race ./...
GOTOOLCHAIN=go1.26.10 go vet ./...
GOTOOLCHAIN=go1.26.10 golangci-lint run ./...
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

Release artifacts are not yet published automatically. Until signing and SBOM
generation are implemented, a release must state that limitation rather than
implying a supply-chain guarantee.

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
